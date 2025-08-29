use std::fmt::{Debug, Formatter};
use crate::easytier::Easytier;
use crate::fakeserver::FakeServer;
use crate::scanning::MinecraftScanner;
use std::mem;
use std::sync::{Mutex, MutexGuard};
use std::time::SystemTime;
use crate::controller::Room;
use crate::scaffolding::profile::Profile;

pub enum AppState {
    Waiting,

    HostScanning {
        scanner: MinecraftScanner,
    },
    HostStarting {
        room: Room,
        port: u16,
    },
    HostOk {
        room: Room,
        port: u16,
        easytier: Easytier,
        profiles: Vec<(SystemTime, Profile)>,
    },

    GuestConnecting {
        room: Room,
    },
    GuestStarting {
        room: Room,
        easytier: Easytier,
    },
    GuestOk {
        room: Room,
        easytier: Easytier,
        server: FakeServer,

        profiles: Vec<Profile>,
    },
    Exception {
        kind: ExceptionType,
    },
}

impl Debug for AppState {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            AppState::Waiting => write!(f, "AppState::Waiting"),
            AppState::HostScanning { ..  } => {
                write!(f, "AppState::HostScanning {{ scanner: .. }}")
            }
            AppState::HostStarting { room, port } => {
                write!(f, "AppState::HostStarting {{ code: {:?}, port: {} }}", room.code, port)
            }
            AppState::HostOk { room, port, profiles, .. } => {
                write!(f, "AppState::HostOk {{ code: {:?}, port: {}, easytier: .., profiles: {:?} }}", room.code, port, profiles)
            }
            AppState::GuestConnecting { room } => {
                write!(f, "AppState::GuestConnecting {{ code: {:?} }}", room.code)
            }
            AppState::GuestStarting { room, .. } => {
                write!(f, "AppState::GuestStarting {{ code: {:?}, easytier: .. }}", room.code)
            }
            AppState::GuestOk { room, server, profiles, .. } => {
                write!(
                    f, "AppState::GuestOk {{ code: {:?}, server_port: {}, easytier: .., profiles: {:?} }}",
                    room.code, server.port, profiles
                )
            }
            AppState::Exception { kind } => {
                write!(f, "AppState::Exception {{ kind: {:?} }}", kind)
            }
        }
    }
}

#[derive(Debug)]
pub enum ExceptionType {
    PingHostFail,
    PingHostRst,
    GuestEasytierCrash,
    HostEasytierCrash,
    PingServerRst,
    ScaffoldingInvalidResponse,
}

pub struct AppStateContainer {
    state: MutexGuard<'static, (u32, AppState)>,
}

pub struct AppStateCapture {
    index: u32
}

impl AppState {
    pub fn acquire() -> AppStateContainer {
        static GLOBAL_STATE: Mutex<(u32, AppState)> = Mutex::new((0, AppState::Waiting));

        AppStateContainer { state: GLOBAL_STATE.lock().unwrap() }
    }
}

impl AppStateContainer {
    pub fn as_ref(&self) -> &AppState {
        &self.state.1
    }

    pub fn as_mut_ref(&mut self) -> &mut AppState {
        &mut self.state.1
    }

    pub fn index(&self) -> u32 {
        self.state.0
    }

    pub fn set(mut self, state: AppState) -> AppStateCapture {
        logging!("State", "Switch to {:?}", &state);

        self.state.0 += 1;
        self.state.1 = state;
        AppStateCapture { index: self.state.0 }
    }

    pub fn replace<F>(mut self, f: F) -> AppStateCapture
    where
        F: FnOnce(AppState) -> AppState
    {
        self.state.0 += 1;

        let legacy = mem::replace(&mut self.state.1, AppState::Waiting);
        let new = f(legacy);
        logging!("State", "Switch to {:?}", &new);
        let _ = mem::replace(&mut self.state.1, new);

        AppStateCapture { index: self.state.0 }
    }

    pub fn increase(mut self) -> AppStateCapture {
        self.state.0 += 1;
        logging!("State", "Switch to {:?}", &self.state.1);
        AppStateCapture { index: self.state.0 }
    }
}

impl AppStateCapture {
    pub fn try_capture(&self) -> Option<AppStateContainer> {
        let state = AppState::acquire();
        if state.index() == self.index {
            Some(state)
        }  else {
            None
        }
    }

    pub fn can_capture(&self) -> bool {
        let state = AppState::acquire();
        state.index() == self.index
    }
}