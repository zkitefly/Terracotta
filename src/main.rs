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
    env, fs,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::mpsc,
    thread::spawn,
};

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

lazy_static::lazy_static! {
    static ref ADDRESSES: Vec<IpAddr> = {
        let mut addresses: Vec<IpAddr> = vec![];

        if let Ok(networks) = local_ip_address::list_afinet_netifas() {
            for (_, address) in networks.into_iter() {
                match address {
                    IpAddr::V4(ip) => {
                        let parts = ip.octets();
                        if !(parts[0] == 10 && parts[1] == 144 && parts[2] == 144) && ip != Ipv4Addr::LOCALHOST && ip != Ipv4Addr::UNSPECIFIED {
                            addresses.push(address);
                        }
                    },
                    IpAddr::V6(ip) => {
                        if ip != Ipv6Addr::LOCALHOST && ip != Ipv6Addr::UNSPECIFIED {
                            addresses.push(address);
                        }
                    }
                };
            }
        }

        addresses.push(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        addresses.push(IpAddr::V6(Ipv6Addr::UNSPECIFIED));

        addresses.sort_by(|ip1, ip2| ip2.cmp(ip1));
        logging!("UI", "Local IP Addresses: {:?}", addresses);
        addresses
    };
}

lazy_static! {
    static ref LOGGING_FILE: std::path::PathBuf = {
        let base = if cfg!(target_os = "macos")
            && let Ok(home) = env::var("HOME")
        {
            std::path::Path::new(&home).to_owned()
        } else {
            std::path::Path::new(&env::temp_dir()).to_owned()
        };

        use chrono::{Datelike, Timelike};
        let now = chrono::Local::now();
        base.join(format!(
            "terracotta-log/{:04}-{:02}-{:02}-{:02}-{:02}-{:02}.log",
            now.year(),
            now.month(),
            now.day(),
            now.hour(),
            now.minute(),
            now.second()
        ))
    };
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
    let is_enable = env::args().into_iter().any(|e| &e == "--redirect-std=yes");
    let is_disable = env::args().into_iter().any(|e| &e == "--redirect-std=no");

    if if is_enable != is_disable {
        is_disable
    } else {
        cfg!(debug_assertions)
    } {
        logging!("UI", "Log redirection is disabled.");
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

    let Ok(logging_file) = fs::File::create(file.clone()) else {
        return;
    };

    logging!(
        "UI",
        "Logs will be saved to {}. There will be not information on the console.",
        file.to_str().unwrap()
    );

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

    Box::leak(Box::new(logging_file));
}
