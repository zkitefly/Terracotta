use std::{
    mem::MaybeUninit,
    net::{Ipv4Addr, SocketAddrV4},
    sync::{Mutex, MutexGuard},
    thread,
    time::{Duration, SystemTime},
};

use crate::{MOTD, code::Room, easytier::Easytier, fakeserver::FakeServer, scanning::Scanning};

use serde_json::{Value, json};
use socket2::{Domain, SockAddr, Socket, Type};

fn check_conn(port: u16) -> bool {
    let start = SystemTime::now();

    let socket = Socket::new(Domain::IPV4, Type::STREAM, None).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    socket
        .set_write_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    if let Ok(_) = socket.connect_timeout(
        &SockAddr::from(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), port)),
        Duration::from_secs(5),
    ) {
        if let Ok(_) = socket.send(&[0xFE]) {
            let mut buf: [MaybeUninit<u8>; 1] = unsafe { MaybeUninit::uninit().assume_init() };

            if let Ok(size) = socket.recv(&mut buf)
                && size >= 1
                && unsafe { buf[0].assume_init() } == 0xFF
            {
                return true;
            }
        }
    }

    thread::sleep(
        (start + Duration::from_secs(5))
            .duration_since(SystemTime::now())
            .unwrap_or(Duration::ZERO),
    );
    return false;
}

enum AppState {
    Waiting {
        begin: SystemTime,
    },
    Scanning {
        begin: SystemTime,
        scanner: Scanning,
    },
    Hosting {
        easytier: Easytier,
        room: Room,
    },
    Guesting {
        easytier: Easytier,
        server: FakeServer,
        ok: bool,
    },
    Exception {
        kind: ExceptionType,
    },
}

pub enum ExceptionType {
    PingHostFail,
    PingHostRst,
    GuestEasytierCrash,
    HostEasytierCrash,
    PingServerRst,
}

impl ExceptionType {
    pub fn register_hook(f: fn(&ExceptionType)) {
        ExceptionType::acquire().push(f);
    }

    fn fire(exception: &ExceptionType) {
        for hook in ExceptionType::acquire().iter() {
            hook(&exception);
        }
    }

    fn acquire() -> MutexGuard<'static, Vec<fn(&ExceptionType)>> {
        lazy_static::lazy_static! {
            static ref HOOKS: Mutex<Vec<fn(&ExceptionType)>> = Mutex::new(vec![]);
        }

        return HOOKS.lock().unwrap();
    }
}

struct AppStateContainer {
    state: MutexGuard<'static, (u32, AppState)>,
}

impl AppState {
    fn touch() -> AppStateContainer {
        let mut guard = AppState::acquire();
        match guard.as_mut_ref() {
            AppState::Waiting { begin } | AppState::Scanning { begin, .. } => {
                *begin = SystemTime::now();
            }
            _ => {}
        }

        return guard;
    }

    fn acquire() -> AppStateContainer {
        lazy_static::lazy_static! {
            static ref GLOBAL_STATE: Mutex<(u32, AppState)> = Mutex::new((
                0,
                AppState::Waiting {
                    begin: SystemTime::now(),
                }
            ));
        }

        let guard = GLOBAL_STATE.lock().unwrap();
        return AppStateContainer { state: guard };
    }
}

impl AppStateContainer {
    fn as_ref(&self) -> &AppState {
        return &(*self.state).1;
    }

    fn as_mut_ref(&mut self) -> &mut AppState {
        return &mut (*self.state).1;
    }

    fn index(&self) -> u32 {
        return self.state.0;
    }

    fn set(mut self, state: AppState) -> u32 {
        self.state.0 += 1;
        self.state.1 = state;
        if let AppState::Exception { kind } = &self.state.1 {
            ExceptionType::fire(kind);
        }

        return self.state.0;
    }

    fn update(mut self, f: fn(&mut AppState)) -> u32 {
        self.state.0 += 1;
        f(&mut self.state.1);
        if let AppState::Exception { kind } = &self.state.1 {
            ExceptionType::fire(kind);
        }

        return self.state.0;
    }

    fn is(&self, index: u32) -> bool {
        return self.state.0 == index;
    }
}

pub fn get_state() -> Value {
    let state = AppState::touch();
    let index = state.index();
    return match state.as_ref() {
        AppState::Waiting { .. } => json!({"state": "waiting", "index": index}),
        AppState::Scanning { .. } => json!({"state": "scanning", "index": index}),
        AppState::Hosting { room, .. } => json!({
            "index": index,
            "state": "hosting",
            "room": room.code
        }),
        AppState::Guesting { server, ok, .. } => json!({
            "state": "guesting",
            "index": index,
            "url": format!("127.0.0.1:{}", server.port),
            "ok": ok
        }),
        AppState::Exception { kind, .. } => json!({
            "index": index,
            "state": "exception",
            "type": match kind {
                ExceptionType::PingHostFail => 0,
                ExceptionType::PingHostRst => 1,
                ExceptionType::GuestEasytierCrash => 2,
                ExceptionType::HostEasytierCrash => 3,
                ExceptionType::PingServerRst => 4,
            }
        }),
    };
}

pub fn set_waiting() {
    logging!("Core", "Setting to state WAITING.");

    let state = AppState::touch();
    if matches!(state.as_ref(), AppState::Waiting { .. }) {
        return;
    }
    state.set(AppState::Waiting {
        begin: SystemTime::now(),
    });
}

pub fn set_scanning() {
    logging!("Core", "Setting to state SCANNING.");

    let state = AppState::touch();
    if matches!(state.as_ref(), AppState::Scanning { .. }) {
        return;
    }

    let index = state.set(AppState::Scanning {
        begin: SystemTime::now(),
        scanner: Scanning::create(|m| m != MOTD),
    });

    thread::spawn(move || {
        loop {
            let state = AppState::acquire();
            if !state.is(index) {
                return;
            }

            let AppState::Scanning { scanner, .. } = state.as_ref() else {
                panic!("State has been changed without increasing index.");
            };

            if let Some(port) = scanner.get_ports().get(0) {
                let room = Room::create(*port);
                logging!(
                    "Core",
                    "Setting Server to state HOSTING, port = {}, room = {}.",
                    port,
                    room.code
                );

                let index = state.set(AppState::Hosting {
                    easytier: room.start(MOTD).0,
                    room: room,
                });

                let port = *port;
                thread::spawn(move || {
                    let mut count: u8 = 0;

                    loop {
                        if check_conn(port) {
                            count = 0;
                        } else {
                            count += 1;

                            if count >= 3 {
                                let state = AppState::acquire();
                                if !state.is(index) {
                                    return;
                                }

                                state.set(AppState::Exception { kind: ExceptionType::PingServerRst });
                            }
                        }

                        let mut state = AppState::acquire();
                        if !state.is(index) {
                            return;
                        }

                        match state.as_mut_ref() {
                            AppState::Hosting { easytier, .. } => {
                                if !easytier.is_alive() {
                                    state.set(AppState::Exception {
                                        kind: ExceptionType::HostEasytierCrash,
                                    });
                                } else {
                                    drop(state);
                                }
                            }
                            _ => panic!("State has been changed without increasing index."),
                        }

                        thread::sleep(Duration::from_millis(200));
                    }
                });
            } else {
                drop(state);
            }

            thread::sleep(Duration::from_millis(200));
        }
    });
}

pub fn set_guesting(room: Room) {
    logging!("Core", "Connecting to room, code={}", room.code);

    let state = AppState::touch();

    let (easytier, server) = room.start(MOTD);
    let server = server.unwrap();
    let port = server.port;

    let index = state.set(AppState::Guesting {
        easytier: easytier,
        server: server,
        ok: false,
    });

    thread::spawn(move || {
        fn check_easytier() -> bool {
            let mut state = AppState::acquire();
            if let AppState::Guesting { easytier, .. } = state.as_mut_ref() {
                if !easytier.is_alive() {
                    state.set(AppState::Exception {
                        kind: ExceptionType::GuestEasytierCrash,
                    });
                    return true;
                }
            }

            return false;
        }

        let mut ok = false;
        for _ in 0..5 {
            if check_conn(port) {
                ok = true;
                break;
            }

            if check_easytier() {
                return;
            }
        }

        let state = AppState::touch();
        if !state.is(index) {
            return;
        }

        if !ok {
            logging!("Core", "Cannot connect to room, port = {}.", port);
            state.set(AppState::Exception {
                kind: ExceptionType::PingHostFail,
            });
            return;
        }

        let index = state.update(|state| {
            if let AppState::Guesting { server, ok, .. } = state {
                server.activate();
                *ok = true;
            } else {
                panic!("State has been changed without increasing index.");
            }
        });

        logging!("Core", "Room is ready, port = {}.", port);

        let mut count: u8 = 0;
        loop {
            if check_easytier() {
                return;
            }

            if check_conn(port) {
                count = 0;

                let state = AppState::touch();
                if !state.is(index) {
                    return;
                }
            } else {
                count += 1;

                if count >= 3 {
                    let state = AppState::touch();
                    if !state.is(index) {
                        return;
                    }

                    logging!("Core", "Connection to room has lost, port = {}.", port);
                    state.set(AppState::Exception {
                        kind: ExceptionType::PingHostRst,
                    });
                    return;
                }
            }
        }
    });
}

pub fn get_waiting_time() -> Option<Duration> {
    let state = AppState::acquire();
    match state.as_ref() {
        AppState::Waiting { begin } | AppState::Scanning { begin, .. } => {
            return match SystemTime::now().duration_since(*begin) {
                Ok(d) => Some(d),
                Err(_) => None,
            };
        }
        _ => {
            return None;
        }
    }
}
