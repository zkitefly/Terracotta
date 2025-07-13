#[macro_export]
macro_rules! logging {
    ($prefix:expr, $($arg:tt)*) => {
        std::println!(
            "[{}]: {}",
            $prefix,
            std::format_args!($($arg)*)
        );
    };
}

#[macro_use]
extern crate rocket;
use interprocess::local_socket as pipe;
use interprocess::local_socket::{
    ToNsName,
    traits::{ListenerExt, Stream},
};
use lazy_static::lazy_static;

use std::{
    io::{Read, Write},
    sync::{
        Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, Instant},
};

pub mod fakeserver;
use fakeserver::FakeServer;
pub mod scanning;
use rocket::{
    fairing::AdHoc,
    http::Status,
    response::content::RawHtml,
    serde::json::{Json, serde_json},
};
use scanning::Scanning;
pub mod easytier;
use easytier::Easytier;
pub mod code;
use code::Room;

static WEB_STATIC: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/webstatics.7z"));

enum AppState {
    Waiting {
        begin: Instant,
    },
    Scanning {
        begin: Instant,
        scanner: Scanning,
    },
    Hosting {
        easytier: Easytier,
        room: Room,
    },
    Guesting {
        easytier: Easytier,
        _entry: FakeServer,
        _room: Room,
    },
}

lazy_static! {
    static ref GLOBAL_STATE: Mutex<(u32, AppState)> = Mutex::new((
        0,
        AppState::Waiting {
            begin: Instant::now(),
        }
    ));
}

fn access_state() -> std::sync::MutexGuard<'static, (u32, AppState)> {
    let mut guard = GLOBAL_STATE.lock().unwrap();
    match &mut (*guard).1 {
        AppState::Waiting { begin } => {
            *begin = Instant::now();
        }
        AppState::Scanning { begin, .. } => {
            *begin = Instant::now();
        }
        _ => {}
    }

    return guard;
}

#[get("/")]
fn index() -> Result<RawHtml<&'static str>, Status> {
    lazy_static! {
        static ref MAIN_PAGE: String = {
            let mut reader = sevenz_rust2::ArchiveReader::new(
                std::io::Cursor::new(WEB_STATIC),
                sevenz_rust2::Password::empty(),
            )
            .unwrap();

            String::from_utf8(reader.read_file("_.html").unwrap()).unwrap()
        };
    }

    return Ok(RawHtml(&MAIN_PAGE));
}

#[get("/state")]
fn get_state() -> Json<serde_json::Value> {
    let v = &mut *access_state();
    return match &v.1 {
        AppState::Waiting { .. } => Json(serde_json::json!({"state": "waiting", "index": v.0})),
        AppState::Scanning { .. } => Json(serde_json::json!({"state": "scanning", "index": v.0})),
        AppState::Hosting { room, .. } => Json(serde_json::json!({
            "state": "hosting",
            "index": v.0,
            "room": room.code
        })),
        AppState::Guesting { .. } => Json(serde_json::json!({
            "state": "guesting",
            "index": v.0,
            "url": format!("127.0.0.1:{}", code::LOCAL_PORT)
        })),
    };
}

#[get("/state/ide")]
fn set_state_ide() -> Status {
    logging!("UI", "Setting Server to state IDE.");

    let state = &mut *access_state();
    state.0 += 1;
    state.1 = AppState::Waiting {
        begin: Instant::now(),
    };
    return Status::Ok;
}

#[get("/state/scanning")]
fn set_state_scanning() -> Status {
    logging!("UI", "Setting Server to state SCANNING.");

    let state = &mut *access_state();
    state.0 += 1;
    state.1 = AppState::Scanning {
        begin: Instant::now(),
        scanner: scanning::create(|motd| motd != code::MOTD),
    };
    return Status::Ok;
}

#[get("/state/guesting?<room>")]
fn set_state_guesting(room: Option<String>) -> Status {
    if let Some(room) = room
        && let Ok(room) = Room::from(&room)
    {
        logging!(
            "UI",
            "Setting Server to state GUESTING, room = {}.",
            room.code
        );

        let state = &mut *access_state();
        state.0 += 1;
        let (easytier, entry) = room.start();
        state.1 = AppState::Guesting {
            easytier: easytier,
            _entry: entry.unwrap(),
            _room: room,
        };
        return Status::Ok;
    }

    return Status::BadRequest;
}

#[rocket::main]
async fn main() {
    let socket = "terracotta-rs.sock"
        .to_ns_name::<pipe::GenericNamespaced>()
        .unwrap();
    match pipe::ListenerOptions::new()
        .name(socket.clone())
        .create_sync()
    {
        Ok(socket) => main_server(socket).await,
        Err(_) => main_client(pipe::Stream::connect(socket).unwrap()).await,
    };
}

async fn main_client(mut socket: pipe::Stream) {
    socket.write(&[1]).unwrap();
}

async fn main_server(socket: pipe::Listener) {
    let (tx1, rx): (Sender<()>, Receiver<()>) = mpsc::channel();
    let tx2: Sender<()> = tx1.clone();

    let rocket = rocket::custom(rocket::Config {
        log_level: rocket::log::LogLevel::Critical,
        port: 0,
        ..rocket::Config::default()
    })
    .mount(
        "/",
        routes![
            index,
            get_state,
            set_state_ide,
            set_state_scanning,
            set_state_guesting
        ],
    )
    .attach(AdHoc::on_liftoff("Open Browser", move |rocket| {
        Box::pin(async move {
            let port = rocket.config().port;

            let _ = open::that(format!("http://127.0.0.1:{}/", port));
            let _unused = access_state();
            let _ = tx2.send(());

            std::thread::spawn(move || {
                for conn in socket.incoming() {
                    if let Ok(conn) = conn {
                        let mut buf: [u8; 1024] = [0; 1024];
                        if let Ok(size) = std::io::BufReader::new(conn).read(&mut buf)
                            && size >= 1
                        {
                            match buf[0] {
                                1 => {
                                    let _ = open::that(format!("http://127.0.0.1:{}/", port));
                                }
                                _ => {}
                            }
                        }
                    }
                }
            });
        })
    }))
    .ignite()
    .await
    .unwrap();

    let shutdown: rocket::Shutdown = rocket.shutdown();
    std::thread::spawn(move || {
        rx.recv().unwrap();

        loop {
            fn handle_offline(time: &Instant) -> bool {
                const TIMEOUT: u128 = if cfg!(debug_assertions) { 3000 } else { 10000 };

                let timeout = Instant::now().duration_since(*time).as_millis();
                if timeout >= TIMEOUT {
                    logging!(
                        "UI",
                        "Server has been in IDE state for {}s. Shutting down.",
                        TIMEOUT / 1000
                    );
                    return true;
                }
                return false;
            }

            if let Ok(_) = rx.recv_timeout(Duration::from_millis(200)) {
                return;
            }

            let state = &mut *GLOBAL_STATE.lock().unwrap();
            match &mut state.1 {
                AppState::Waiting { begin } => {
                    if handle_offline(begin) {
                        shutdown.notify();
                        return;
                    }
                }
                AppState::Scanning { begin, scanner } => {
                    if handle_offline(begin) {
                        shutdown.notify();
                        return;
                    }

                    let ports = scanner.get_ports();
                    if let Some(port) = ports.get(0) {
                        let room = Room::create(*port);
                        logging!(
                            "UI",
                            "Setting Server to state HOSTING, port = {}, room = {}.",
                            port,
                            room.code
                        );

                        state.0 += 1;
                        state.1 = AppState::Hosting {
                            easytier: room.start().0,
                            room: room,
                        };
                    }
                }
                AppState::Hosting { easytier, .. } => {
                    if !easytier.is_alive() {
                        logging!("UI", "Easytier has been dead.");
                        state.0 += 1;
                        state.1 = AppState::Waiting {
                            begin: Instant::now(),
                        };
                    }
                }
                AppState::Guesting { easytier, .. } => {
                    if !easytier.is_alive() {
                        logging!("UI", "Easytier has been dead.");
                        state.0 += 1;
                        state.1 = AppState::Waiting {
                            begin: Instant::now(),
                        };
                    }
                }
            };

            thread::sleep(Duration::from_millis(200));
        }
    });

    let _ = rocket.launch().await;
    let _ = tx1.send(());
}
