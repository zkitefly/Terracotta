use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use rocket::http::Status;
use rocket::serde::json::Json;
use serde_json::{Value, json};

use crate::code::Room;
use crate::{LOGGING_FILE, core};

static WEB_STATIC: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/webstatics.7z"));

pub struct MemoryFile(PathBuf, &'static [u8]);

impl<'r> rocket::response::Responder<'r, 'static> for MemoryFile {
    fn respond_to(self, req: &'r rocket::Request<'_>) -> rocket::response::Result<'static> {
        let mut response = self.1.respond_to(req)?;
        if let Some(ext) = self.0.extension() {
            if let Some(ct) = rocket::http::ContentType::from_extension(&ext.to_string_lossy()) {
                response.set_header(ct);
            }
        }

        Ok(response)
    }
}

#[get("/<path..>")]
fn static_files(mut path: PathBuf) -> Result<MemoryFile, Status> {
    lazy_static::lazy_static! {
        static ref MAIN_PAGE: Vec<(PathBuf, Box<[u8]>)> = {
            let mut reader = sevenz_rust2::ArchiveReader::new(
                std::io::Cursor::new(WEB_STATIC),
                sevenz_rust2::Password::empty(),
            )
            .unwrap();
            let mut pages: Vec<(PathBuf, Box<[u8]>)> = vec![];
            let _ = reader.for_each_entries(|entry, reader| {
                if entry.is_directory() {
                    return Ok(true);
                }

                let mut buffer: Vec<u8> = vec![];
                reader.read_to_end(&mut buffer).unwrap();
                buffer.shrink_to_fit();
                pages.push((PathBuf::from(entry.name()), buffer.into_boxed_slice()));

                return Ok(true);
            });

            #[cfg(debug_assertions)] {
                let mut msg = String::from("Loading static files: ");
                for (path, data) in pages.iter() {
                    msg.push_str("\n- ");
                    msg.push_str(path.as_os_str().to_str().unwrap());
                    msg.push_str(": ");
                    msg.push_str(&data.len().to_string());
                    msg.push_str(" bytes");
                }
                logging!("UI", "{}", msg);
            }

            pages.shrink_to_fit();
            pages
        };
    }

    if path.as_os_str().is_empty() {
        path = PathBuf::from("_.html");
    }

    return match MAIN_PAGE.iter().find(|(entry, _)| *entry == path) {
        Some((_, data)) => Ok(MemoryFile(path, data)),
        None => Err(Status { code: 404 }),
    };
}

#[get("/state")]
fn get_state() -> Json<Value> {
    return Json(core::get_state());
}

#[get("/state/ide")]
fn set_state_ide() -> Status {
    core::set_waiting();
    return Status::Ok;
}

#[get("/state/scanning")]
fn set_state_scanning() -> Status {
    core::set_scanning();
    return Status::Ok;
}

#[get("/state/guesting?<room>")]
fn set_state_guesting(room: Option<String>) -> Status {
    if let Some(room) = room
        && let Ok(room) = Room::from(&room)
    {
        core::set_guesting(room);
        return Status::Ok;
    }

    return Status::BadRequest;
}

#[get("/log")]
fn download_log() -> std::fs::File {
    return std::fs::File::open((*LOGGING_FILE).clone()).unwrap();
}

#[get("/meta")]
fn get_meta() -> Json<Value> {
    return Json(json!({
        "version": env!("TERRACOTTA_VERSION"),
        "easytier_version": env!("TERRACOTTA_ET_VERSION"),
        "target_tuple": format!(
            "{}-{}-{}-{}",
            env!("CARGO_CFG_TARGET_ARCH"),
            env!("CARGO_CFG_TARGET_VENDOR"),
            env!("CARGO_CFG_TARGET_OS"),
            env!("CARGO_CFG_TARGET_ENV"),
         ),
        "target_arch": env!("CARGO_CFG_TARGET_ARCH"),
        "target_vendor": env!("CARGO_CFG_TARGET_VENDOR"),
        "target_os": env!("CARGO_CFG_TARGET_OS"),
        "target_env": env!("CARGO_CFG_TARGET_ENV"),
    }));
}

pub async fn server_main(port_callback: mpsc::Sender<u16>, daemon: bool) {
    core::ExceptionType::register_hook(|_| {
        // TODO: Send system notifications.
    });

    let _ = rocket::custom(rocket::Config {
        log_level: rocket::log::LogLevel::Critical,
        port: if cfg!(debug_assertions) { 8080 } else { 0 },
        ..rocket::Config::default()
    })
    .mount(
        "/",
        routes![
            get_state,
            set_state_ide,
            set_state_scanning,
            set_state_guesting,
            download_log,
            static_files,
            get_meta,
        ],
    )
    .attach(rocket::fairing::AdHoc::on_liftoff(
        "Open Browser",
        move |rocket| {
            Box::pin(async move {
                let port = rocket.config().port;
                logging!(
                    ":",
                    "{}",
                    json!({
                        "version": 1, 
                        "url": format!("http://127.0.0.1:{}/", port)}
                    )
                );

                if !cfg!(debug_assertions) && !daemon {
                    let _ = open::that(format!("http://127.0.0.1:{}/", port));
                }
                let _ = port_callback.send(port);

                let shutdown = rocket.shutdown();
                thread::spawn(move || {
                    loop {
                        if let Some(duration) = core::get_waiting_time()
                            && duration > Duration::from_secs(600)
                        {
                            shutdown.notify();
                            return;
                        }

                        thread::sleep(Duration::from_millis(200));
                    }
                });
            })
        },
    ))
    .launch()
    .await;
}
