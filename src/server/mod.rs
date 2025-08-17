use std::sync::mpsc;

use rocket::{http::Status, serde::json::Json};
use serde_json::{Value, json};

use crate::{LOGGING_FILE, core};

mod states;
mod statics;
mod yggdrasils;

cfg_if::cfg_if! {
    if #[cfg(target_os = "macos")] {
        #[get("/log")]
        fn download_log() -> Status {
            use std::process::Command;
            return match Command::new("open")
                .arg((*LOGGING_FILE).parent().unwrap())
                .spawn() {
                    Ok(_) => Status::Ok,
                    Err(e) => {
                        logging!("Core", "Cannot open logging file: {:?}", e);
                        Status::InternalServerError
                    }
                };
        }
    } else {
        #[get("/log")]
        fn download_log() -> std::fs::File {
            return std::fs::File::open((*LOGGING_FILE).clone()).unwrap();
        }
    }
}

#[get("/panic?<peaceful>")]
fn panic(peaceful: Option<bool>) {
    if peaceful.unwrap_or(false) {
        logging!("Core", "Closed by web API. Shutting down.");
        std::process::exit(0);
    } else {
        panic!();
    }
}

#[get("/meta")]
fn get_meta() -> Json<Value> {
    return Json(json!({
        "version": env!("TERRACOTTA_VERSION"),
        "compile_timestamp": timestamp::compile_time!().to_string(),
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

#[get("/.well-known/appspecific/com.chrome.devtools.json")]
fn devtools() -> Status {
    Status::NotFound
}

pub async fn server_main(port_callback: mpsc::Sender<u16>) {
    core::ExceptionType::register_hook(|_| {
        // TODO: Send system notifications.
    });

    let rocket = rocket::custom(rocket::Config {
        log_level: rocket::log::LogLevel::Critical,
        port: if cfg!(debug_assertions) { 8080 } else { 0 },
        workers: 2,
        ..rocket::Config::default()
    });

    let rocket = states::configure(rocket);
    let rocket = statics::configure(rocket);
    let rocket = yggdrasils::configure(rocket);

    rocket
        .mount("/", routes![download_log, get_meta, panic, devtools])
        .attach(rocket::fairing::AdHoc::on_liftoff(
            "Invoke Port Callback",
            move |rocket| {
                Box::pin(async move {
                    let _ = port_callback.send(rocket.config().port);
                })
            },
        ))
        .launch()
        .await
        .unwrap();
}
