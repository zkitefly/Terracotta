use crate::scaffolding::{PacketResponse, TIMEOUT};
use socket2::{Domain, SockAddr, Socket, Type};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::{io, thread};

pub type Handlers = &'static [(
    &'static str,
    &'static str,
    fn(&[u8], Vec<u8>) -> io::Result<PacketResponse>,
)];

pub fn start(handlers: Handlers, port: u16) -> io::Result<u16> {
    let socket = Socket::new(Domain::IPV4, Type::STREAM, None).unwrap();
    socket
        .bind(&SockAddr::from(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port)))?;
    socket
        .set_read_timeout(Some(TIMEOUT))
        .unwrap();
    socket
        .set_write_timeout(Some(TIMEOUT))
        .unwrap();
    socket.listen(128)?;

    let port = socket.local_addr().unwrap().as_socket().unwrap().port();

    thread::spawn(move || {
        let listener: TcpListener = socket.into();
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else {
                continue;
            };

            thread::spawn(move || {
                let handle_connection = &mut |stream: &mut TcpStream| -> io::Result<()> {
                    let mut kind_size = [0u8; 1];
                    stream.read_exact(&mut kind_size)?;
                    let kind_size = kind_size[0] as usize;

                    let mut kind = vec![0u8; kind_size];
                    stream.read_exact(&mut kind)?;
                    let kind = String::from_utf8(kind)
                        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                    let kinds = kind.splitn(3, ':').collect::<Vec<_>>();
                    if kinds.len() != 2 {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "Invalid request kind.",
                        ));
                    }

                    let mut body_size = [0u8; 4];
                    stream.read_exact(&mut body_size)?;
                    let body_size = u32::from_be_bytes(body_size) as usize;

                    let mut body = vec![0u8; body_size];
                    stream.read_exact(&mut body)?;

                    let response = 'response: {
                        for (namespace, path, handle) in handlers {
                            if *namespace == kinds[0] && *path == kinds[1] {
                                break 'response match handle(&body, vec![0u8; 5]) {
                                    Ok(pkg) => {
                                        let (status, mut response) = match pkg {
                                            PacketResponse::Ok { data } => (0, data),
                                            PacketResponse::Fail { data, status } => (status, data),
                                        };

                                        response[0] = status;
                                        let response_size = (response.len() - 5) as u32;
                                        response[1..5].copy_from_slice(&response_size.to_be_bytes());

                                        response
                                    }
                                    Err(e) => {
                                        let mut response = vec![0u8; 5];
                                        write!(&mut response, "{:?}", e).unwrap();

                                        let len = response.len() - 5;
                                        response[0] = 255;
                                        response[1..5].copy_from_slice(&(len as u32).to_be_bytes());

                                        response
                                    }
                                };
                            }
                        }

                        static NOT_FOUND_MSG: &str =
                            "Requested protocol hasn't been implemented.";

                        let mut response = vec![0u8; 5 + NOT_FOUND_MSG.len()];
                        response[0] = 255;
                        response[1..5].copy_from_slice(&(NOT_FOUND_MSG.len() as u32).to_be_bytes());
                        response[5..].copy_from_slice(NOT_FOUND_MSG.as_bytes());

                        break 'response response;
                    };

                    stream.write_all(&response)?;
                    stream.flush()?;
                    Ok(())
                };

                loop {
                    if let Err(e) = handle_connection(&mut stream) {
                        logging!("ScaffoldingServer", "Connection closed: {:?}", e);
                        return;
                    }
                }
            });
        }
    });

    Ok(port)
}
