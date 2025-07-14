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

use std::{env, fs, process, sync::mpsc, thread::spawn};

pub mod code;
pub mod easytier;
pub mod fakeserver;
pub mod scanning;
pub mod server;

#[cfg(target_family = "windows")]
pub mod lock_windows;
#[cfg(target_family = "windows")]
use lock_windows::State as lock;
#[cfg(target_family = "unix")]
pub mod lock_unix;
#[cfg(target_family = "unix")]
use lock_unix::State as lock;

lazy_static! {
    static ref LOGGING_FILE: std::path::PathBuf = std::path::Path::join(
        &env::temp_dir(),
        format!("terracotta-log/{}.log", process::id()),
    );
}

#[rocket::main]
async fn main() {
    let state = lock::get_state();
    match &state {
        lock::Single { .. } => {
            logging!("UI", "Running in server mode.");

            let (tx, rx) = mpsc::channel::<u16>();
            let tx2 = tx.clone();

            let future = main_server(tx);
            spawn(move || {
                let port = rx.recv().unwrap();
                if port != 0 {
                    state.set_port(port);
                }
            });

            future.await;
            let _ = tx2.send(0);
        }
        lock::Secondary { port } => {
            logging!("UI", "Running in secondary mode, port={}.", port);

            let _ = open::that(format!("http://127.0.0.1:{}/", port));
        }
        lock::Unknown => {
            logging!(
                "UI",
                "Cannot determin application mode. Fallback to server mode."
            );

            let (tx, _) = mpsc::channel::<u16>();
            main_server(tx).await;
        }
    };
}

async fn main_server(port: mpsc::Sender<u16>) {
    redirect_std(&*LOGGING_FILE);

    let future = server::server_main(port);
    let _ = Box::new(&*easytier::FACTORY);
    future.await;
}

fn redirect_std(file: &std::path::PathBuf) {
    if cfg!(debug_assertions) {
        return;
    }

    let Some(parent) = file.parent() else {
        return;
    };

    if !fs::metadata(parent).is_ok() {
        if !fs::create_dir_all(parent).is_ok() {
            return;
        }
    }

    let Ok(logging_file) = fs::File::create((*LOGGING_FILE).clone()) else {
        return;
    };

    #[cfg(target_family = "unix")]
    {
        use std::os::unix::io::AsRawFd;
        unsafe {
            libc::dup2(logging_file.as_raw_fd(), libc::STDOUT_FILENO);
            libc::dup2(logging_file.as_raw_fd(), libc::STDERR_FILENO);
        }
    }
    #[cfg(target_family = "windows")]
    {
        use std::os::windows::io::AsRawHandle;
        unsafe {
            let _ = winapi::um::processenv::SetStdHandle(
                winapi::um::winbase::STD_OUTPUT_HANDLE,
                logging_file.as_raw_handle() as _,
            );
            let _ = winapi::um::processenv::SetStdHandle(
                winapi::um::winbase::STD_ERROR_HANDLE,
                logging_file.as_raw_handle() as _,
            );
        }
    }

    logging!(
        "UI",
        "Logs will be saved to {}. There will be not information on the console.",
        file.to_str().unwrap()
    );
}
