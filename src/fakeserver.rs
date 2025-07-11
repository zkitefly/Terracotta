use std::io::Result;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::str::FromStr;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

const SIG_TERMINAL: u32 = 65536 + 1;
const SIG_PARSE: u32 = 65536 + 2;

pub struct FakeServer {
    signal: Sender<u32>,
}

pub fn create(motd: String) -> FakeServer {
    let (tx, rx): (Sender<u32>, Receiver<u32>) = mpsc::channel();
    thread::spawn(move || {
        let result = run(motd, rx);

        match result {
            Ok(_) => {}
            Err(err) => {
                panic!("{}", err);
            }
        }
    });

    return FakeServer { signal: tx };
}

fn run(motd: String, signal: Receiver<u32>) -> Result<()> {
    lazy_static::lazy_static! {
        static ref ADDRESSES: (Ipv4Addr, Ipv6Addr) = {
            let mut v4 = Ipv4Addr::UNSPECIFIED;
            let mut v6 = Ipv6Addr::UNSPECIFIED;
            for (_, ip) in local_ip_address::list_afinet_netifas().unwrap().iter() {
                match ip {
                    IpAddr::V4(ip) => {
                        let parts = ip.octets();
                        if !(parts[0] == 10 && parts[1] == 144 && parts[2] == 144) && *ip != Ipv4Addr::LOCALHOST && *ip != Ipv4Addr::UNSPECIFIED {
                            v4 = ip.clone();
                        }
                    },
                    IpAddr::V6(ip) => {
                        if *ip != Ipv6Addr::LOCALHOST && *ip != Ipv6Addr::UNSPECIFIED {
                            v6 = ip.clone();
                        }
                    }
                };
            }

            logging!("Fake Server", "Local IP: v4={}, v6={}", v4, v6);
            (v4, v6)
        };
    }

    let sockets: [(UdpSocket, SocketAddr); 2] = [
        {
            let socket = UdpSocket::bind((ADDRESSES.0, 0))?;
            socket.set_broadcast(true)?;
            socket.set_multicast_ttl_v4(4)?;
            socket.set_multicast_loop_v4(true)?;

            (
                socket,
                SocketAddr::new(IpAddr::V4(Ipv4Addr::from_str("224.0.2.60").unwrap()), 4445),
            )
        },
        {
            let socket = UdpSocket::bind((ADDRESSES.1, 0))?;
            socket.set_multicast_loop_v6(true)?;
            (
                socket,
                SocketAddr::new(
                    IpAddr::V6(Ipv6Addr::from_str("FF75:230::60").unwrap()),
                    4445,
                ),
            )
        },
    ];

    let mut message: String = "".to_owned();
    let mut message_bytes = message.as_bytes();

    loop {
        if let Ok(value) = signal.recv_timeout(Duration::from_millis(1000)) {
            match value {
                1..=65536 => {
                    logging!(
                        "Server Faking",
                        "Faking server with PORT={}, MOTD={}",
                        value as u16,
                        motd
                    );
                    message = format!("[MOTD]{}[/MOTD][AD]{}[/AD]", motd, value as u16);
                    message_bytes = message.as_bytes();
                }
                SIG_TERMINAL => {
                    logging!("Server Faking", "Stopped");
                    return Ok(());
                }
                SIG_PARSE => {
                    logging!("Server Faking", "Paused");
                    message = "".to_owned();
                    message_bytes = message.as_bytes();
                }
                _ => panic!("Unknown signal {}.", value),
            }
        }

        if message_bytes.len() > 0 {
            for (socket, address) in sockets.iter() {
                let _ = socket.send_to(message_bytes, address);
            }
        }
    }
}

impl FakeServer {
    pub fn set_port(&self, port: u16) {
        let _ = self.signal.send(port as u32);
    }

    pub fn stop_broadcast(&self) {
        let _ = self.signal.send(SIG_PARSE);
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        let _ = self.signal.send(SIG_TERMINAL);
    }
}
