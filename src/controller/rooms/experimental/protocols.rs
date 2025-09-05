use crate::controller::states::AppState;
use crate::scaffolding::profile::{ProfileKind, ProfileSnapshot};
use crate::scaffolding::server::Handlers;
use crate::scaffolding::PacketResponse;
use serde_json::{json, Serializer, Value};
use std::io;
use std::time::SystemTime;
use serde::ser::SerializeSeq;
use serde::Serializer as _;

fn parse<F, R>(f: F) -> io::Result<R>
where
    F: FnOnce() -> Option<R>,
{
    f().ok_or(io::Error::from(io::ErrorKind::InvalidInput))
}

pub static HANDLERS: Handlers = &[
    ("c", "ping", |request: &[u8], mut response: Vec<u8>| -> io::Result<PacketResponse> {
        response.extend_from_slice(request);

        PacketResponse::ok(response)
    }),
    ("c", "protocols", |_: &[u8], mut response: Vec<u8>| -> io::Result<PacketResponse> {
        for (i, handler) in HANDLERS.iter().enumerate() {
            response.extend_from_slice(handler.0.as_bytes());
            response.push(b':');
            response.extend_from_slice(handler.1.as_bytes());

            if i != HANDLERS.len() - 1 {
                response.push(b'\0');
            }
        }

        PacketResponse::ok(response)
    }),
    ("c", "server_port", |_: &[u8], mut response: Vec<u8>| -> io::Result<PacketResponse> {
        if let Some(port) = {
            let state = AppState::acquire();
            match state.as_ref() {
                AppState::HostOk { port, .. } => Some(*port),
                _ => None,
            }
        } {
            response.extend_from_slice(&port.to_be_bytes());
            PacketResponse::ok(response)
        } else {
            PacketResponse::fail(32, response)
        }
    }),
    ("c", "player_ping", |request: &[u8], response: Vec<u8>| -> io::Result<PacketResponse> {
        let value: Value = serde_json::from_str(&String::from_utf8_lossy(request))?;

        let name = parse(|| value.as_object()?.get("name")?.as_str())?;
        let machine_id = parse(|| value.as_object()?.get("machine_id")?.as_str())?;
        let vendor = parse(|| value.as_object()?.get("vendor")?.as_str())?;

        let mut container = AppState::acquire();
        let AppState::HostOk { profiles, .. } = container.as_mut_ref() else {
            return Err(io::Error::other("IllegalStateException: Expecting HostOk."));
        };
        match profiles.iter().position(|profile| profile.1.get_machine_id() == machine_id) {
            Some(i) if i >= 1 => {
                profiles[i].0 = SystemTime::now();

                if profiles[i].1.get_name() != name {
                    profiles[i].1.set_name(name.to_string());
                    container.increase_shared();
                }
            }
            Some(_) => return Err(io::Error::other("IllegalStateException: Cannot modify host, machine_id may conflict.")),
            None => {
                profiles.push((SystemTime::now(), ProfileSnapshot {
                    machine_id: machine_id.to_string(),
                    name: name.to_string(),
                    vendor: vendor.to_string(),
                    kind: ProfileKind::GUEST
                }.into_profile()));
                container.increase_shared();
            }
        }

        PacketResponse::ok(response)
    }),
    ("c", "player_profiles_list", |_: &[u8], mut response: Vec<u8>| -> io::Result<PacketResponse> {
        let mut value = Serializer::new(&mut response);

        let container = AppState::acquire();
        let AppState::HostOk { profiles, .. } = container.as_ref() else {
            return Err(io::Error::other("IllegalStateException: Expecting HostOk."));
        };

        let mut sequence = value.serialize_seq(Some(profiles.len()))?;
        for (_, profile) in profiles {
            sequence.serialize_element(&json!({
                "name": profile.get_name(),
                "machine_id": profile.get_machine_id(),
                "vendor": profile.get_vendor(),
                "kind": match profile.get_kind() {
                    ProfileKind::HOST => "HOST",
                    ProfileKind::GUEST => "GUEST",
                    ProfileKind::LOCAL => unreachable!(),
                }
            }))?;
        }
        sequence.end()?;

        PacketResponse::ok(response)
    }),
];