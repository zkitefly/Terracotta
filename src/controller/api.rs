use crate::MOTD;
use crate::controller::{ExceptionType, Room, SCAFFOLDING_PORT};
use crate::controller::states::AppState;
use crate::scanning::MinecraftScanner;
use serde_json::{Value, json};
use std::thread;
use std::time::{Duration, SystemTime};
use rocket::serde::Serialize;
use serde::ser::SerializeSeq;
use serde::Serializer;
use crate::scaffolding::profile::Profile;

pub fn get_state() -> Value {
    let state = AppState::acquire();
    let index = state.index();

    match state.as_ref() {
        AppState::Waiting => {
            json!({"state": "waiting", "index": index})
        }

        AppState::HostScanning { .. } => {
            json!({"state": "host-scanning", "index": index})
        }
        AppState::HostStarting { room, .. } => {
            json!({"state": "host-starting", "index": index, "room": room.code})
        }
        AppState::HostOk { room, profiles, .. } => {
            struct Holder<'a>(&'a Vec<(SystemTime, Profile)>);
            impl<'a> Serialize for Holder<'a> {
                fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
                where
                    S: Serializer,
                {
                    let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
                    for (_, profile) in self.0 {
                        sequence.serialize_element(profile)?;
                    }
                    sequence.end()
                }
            }

            json!({"state": "host-ok", "index": index, "room": room.code, "profiles": Holder(profiles)})
        }

        AppState::GuestConnecting { room, .. } => {
            json!({"state": "guest-connecting", "index": index, "room": room.code})
        }
        AppState::GuestStarting { room, .. } => {
            json!({"state": "guest-starting", "index": index, "room": room.code})
        }
        AppState::GuestOk { server, profiles, .. } => {
            json!({"state": "guest-ok", "index": index, "url": format!("127.0.0.1:{}", server.port), "profiles": profiles})
        }
        AppState::Exception { kind, .. } => json!({
            "state": "exception",
            "index": index,
            "type": match kind {
                ExceptionType::PingHostFail => 0,
                ExceptionType::PingHostRst => 1,
                ExceptionType::GuestEasytierCrash => 2,
                ExceptionType::HostEasytierCrash => 3,
                ExceptionType::PingServerRst => 4,
                ExceptionType::ScaffoldingInvalidResponse => 5,
            }
        }),
    }
}

pub fn set_waiting() {
    logging!("Core", "Setting to state WAITING.");

    let state = AppState::acquire();
    if matches!(state.as_ref(), AppState::Waiting) {
        return;
    }
    state.set(AppState::Waiting);
}

pub fn set_scanning(player: Option<String>) {
    let capture = {
        let state = AppState::acquire();
        if !matches!(state.as_ref(), AppState::Waiting { .. }) {
            return;
        }

        state.set(AppState::HostScanning {
            scanner: MinecraftScanner::create(|m| m != MOTD),
        })
    };
    logging!("Core", "Setting to state SCANNING.");

    thread::spawn(move || {
        let (room, port, capture) = loop {
            let Some(state) = capture.try_capture() else {
                return;
            };
            let AppState::HostScanning { scanner, .. } = state.as_ref() else {
                unreachable!()
            };

            if let Some(port) = scanner.get_ports().first() {
                let room = Room::create({
                    let port = SCAFFOLDING_PORT.lock().unwrap();
                    *port
                });
                break (room.clone(), *port, state.set(AppState::HostStarting { room, port: *port }));
            }

            thread::sleep(Duration::from_millis(200));
        };

        room.start_host(port, player, capture);
    });
}

pub fn set_guesting(room: Room, player: Option<String>) -> bool {
    let capture = {
        let state = AppState::acquire();
        if !matches!(state.as_ref(), AppState::Waiting { .. }) {
            return false;
        }
        state.set(AppState::GuestConnecting { room: room.clone() })
    };
    logging!("Core", "Connecting to room, code={}", room.code);
    thread::spawn(move || room.start_guest(capture, player));

    true
}
