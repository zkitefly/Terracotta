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
use lazy_static::lazy_static;

use std::{
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

static WEB_STATIC: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/__webstatics.7z"));

enum AppState {
    Waiting {
        begin: Instant,
    },
    Scanning {
        begin: Instant,
        scanner: Scanning,
    },
    Hosting {
        _easytier: Easytier,
        room: Room,
    },
    Guesting {
        _easytier: Easytier,
        _entry: FakeServer,
        room: Room,
    },
}

lazy_static! {
    static ref GLOBAL_STATE: Mutex<AppState> = Mutex::new(AppState::Waiting {
        begin: Instant::now(),
    });
}

fn access_state() -> std::sync::MutexGuard<'static, AppState> {
    let mut guard = GLOBAL_STATE.lock().unwrap();
    match &mut *guard {
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
    return match &*access_state() {
        AppState::Waiting { .. } => Json(serde_json::json!({"state": "waiting"})),
        AppState::Scanning { .. } => Json(serde_json::json!({"state": "scanning"})),
        AppState::Hosting { room, .. } => Json(serde_json::json!({
            "state": "hosting",
            "room": room.code
        })),
        AppState::Guesting { room, .. } => Json(serde_json::json!({
            "state": "guesting",
            "url": ["127.0.0.1:35781", format!("10.144.144.1:{}", room.port)]
        })),
    };
}

#[get("/state/ide")]
fn set_state_ide() -> Status {
    logging!("UI", "Setting Server to state IDE.");

    let mut state = access_state();
    *state = AppState::Waiting {
        begin: Instant::now(),
    };
    return Status::Ok;
}

#[get("/state/scanning")]
fn set_state_scanning() -> Status {
    logging!("UI", "Setting Server to state SCANNING.");

    let mut state = access_state();
    *state = AppState::Scanning {
        begin: Instant::now(),
        scanner: scanning::create(|motd| {
            !motd.eq("§6§lTerracotta | 陶瓦 联机大厅（请关闭代理软件 否则无法进服）")
        }),
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

        let mut state = access_state();
        *state = AppState::Guesting {
            _easytier: room.start(),
            _entry: {
                let s = fakeserver::create(
                    "§6§lTerracotta | 陶瓦 联机大厅（请关闭代理软件 否则无法进服）".to_string(),
                );
                s.set_port(room.port);
                s
            },
            room: room,
        };
        return Status::Ok;
    }

    return Status::BadRequest;
}

#[rocket::main]
async fn main() {
    let holder = single_instance::SingleInstance::new("terracotta-rs-easytier").unwrap();
    if !holder.is_single() {
        let _ = open::that("http://127.0.0.1:8000/");
        return;
    }

    let rocket = rocket::custom(rocket::Config {
        log_level: rocket::log::LogLevel::Critical,
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
    .attach(AdHoc::on_liftoff("Open Browser", |_| {
        Box::pin(async {
            let _ = open::that("http://127.0.0.1:8000/");
            let _unused = access_state();
        })
    }))
    .ignite()
    .await
    .unwrap();

    let shutdown = rocket.shutdown();
    let (tx, rx): (Sender<()>, Receiver<()>) = mpsc::channel();
    std::thread::spawn(move || {
        loop {
            if let Ok(_) = rx.recv_timeout(Duration::from_millis(200)) {
                return;
            }

            let mut state = GLOBAL_STATE.lock().unwrap();
            match &*state {
                AppState::Waiting { begin } => {
                    if Instant::now().duration_since(*begin).as_millis() >= if cfg!(debug_assertions) {5000} else {10000} {
                        logging!("UI", "Server has been in IDE state for 10s. Shutting down.");
                        shutdown.notify();
                        return;
                    }
                }
                AppState::Scanning { begin, scanner } => {
                    if Instant::now().duration_since(*begin).as_millis() >= 10000 {
                        logging!(
                            "UI",
                            "Server has been in SCANNING state for 10s. Shutting down."
                        );
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

                        *state = AppState::Hosting {
                            _easytier: room.start(),
                            room: room,
                        };
                    }
                }
                AppState::Hosting { .. } => {}
                AppState::Guesting { .. } => {}
            };

            thread::sleep(Duration::from_millis(200));
        }
    });

    let _ = rocket.launch().await;
    let _ = tx.send(());
}

// fn main() {
//     let args: Vec<String> = std::env::args().collect();
//     logging!("UI", "Program Arguments: {:?}", args);

//     match args.len() {
//         2 if args[1] == "--host" => {
//             logging!("UI", "Scanning servers.");
//             let scanning: Scanning = scanning::create();

//             let port = loop {
//                 let ports = scanning.get_port();
//                 if let Some(port) = ports.get(0) {
//                     break *port;
//                 }
//             };

//             let room = Room::create(port);
//             let factory: EasytierFactory = easytier::create_factory().unwrap();
//             let easytier: Easytier = room.start(factory);

//             logging!("UI", "Room has started. Code={:?}", room.code);
//             let mut string = String::from("");
//             let _ = stdin().read_line(&mut string);

//             easytier.kill();
//         }
//         3 if args[1] == "--guest" => {
//             let code = Room::from(&args[2]);
//             match code {
//                 Ok(room) => {
//                     logging!("UI", "Joining room. {:#?}", room);

//                     let factory: EasytierFactory = easytier::create_factory().unwrap();
//                     let easytier: Easytier = room.start(factory);
//                     let server: FakeServer = fakeserver::create(String::from(
//                         "§6§lTerracotta | 陶瓦 联机大厅（请关闭代理软件 否则无法进服）",
//                     ));
//                     server.set_port(room.port);

//                     logging!("UI", "Room has started. Code={:?}", room.code);

//                     let mut string = String::from("");
//                     let _ = stdin().read_line(&mut string);

//                     easytier.kill();
//                 }
//                 Err(reason) => {
//                     panic!("Cannot parse room: {}", reason);
//                 }
//             }
//         }
//         _ => {
//             println!("Terracotta Usage");
//             println!("--host: Automatically detect the local server and start a room.");
//             println!("--guest <room code>: Join the room.");
//         }
//     }
// }
