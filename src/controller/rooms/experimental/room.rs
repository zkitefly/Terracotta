use crate::controller::states::{AppState, AppStateCapture};
use crate::controller::{ExceptionType, Room, RoomKind, SCAFFOLDING_PORT};
use crate::easytier;
use crate::fakeserver::FakeServer;
use crate::scaffolding::client::ClientSession;
use crate::scaffolding::profile::{Profile, ProfileKind, ProfileSnapshot};
use crate::scaffolding::PacketResponse;
use rand_core::{OsRng, TryRngCore};
use serde_json::{json, Value};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, TcpListener};
use std::time::{Duration, SystemTime};
use std::thread;

use crate::controller::experimental::{MACHINE_ID, VENDOR};
use crate::controller::rooms::legacy;

static CHARS: &[u8] = "0123456789ABCDEFGHJKLMNPQRSTUVWXYZ".as_bytes();

fn lookup_char(char: char) -> Option<u8> {
    let char = match char {
        'I' => '1',
        'O' => '0',
        _ => char,
    };

    for (j, c) in CHARS.iter().enumerate() {
        if *c as char == char {
            return Some(j as u8);
        }
    }

    None
}

pub fn create_room(port: u16) -> Room {
    let value = {
        let mut bytes = [0u8; 16];
        OsRng.try_fill_bytes(&mut bytes).unwrap();
        u128::from_be_bytes(bytes)
    } % 34u128.pow(16);
    let value = (value & !0xFFFF00) | ((port as u128) << 8);
    let value = value - value % 7;

    let (code, network_name, network_secret) = from_value(value);

    Room {
        code,
        network_name,
        network_secret,
        kind: RoomKind::Experimental { scaffolding: *SCAFFOLDING_PORT.lock().unwrap() },
    }
}

pub fn parse(code: &str) -> Option<Room> {
    let code: Vec<char> = code.to_ascii_uppercase().chars().collect();
    if code.len() < "R/XXXX-XXXX-XXXX-XXXX".len() {
        return None;
    }

    let value: u128 = 'value: {
        for code in code.windows("R/XXXX-XXXX-XXXX-XXXX".len()) {
            let code = &code[2..];

            let mut value: u128 = 0;
            for i in (0.."XXXX-XXXX-XXXX-XXXX".len()).rev() {
                if i == 4 || i == 9 || i == 14 {
                    if code[i] != '-' {
                        return None;
                    }
                } else {
                    value = value * 34 + lookup_char(code[i])? as u128;
                }
            }
            if value.is_multiple_of(7) {
                break 'value value;
            }
        }
        return None;
    };

    let (code, network_name, network_secret) = from_value(value);
    let port = ((value & 0xFFFF00) >> 8) as u16;

    Some(Room {
        code,
        network_name,
        network_secret,
        kind: RoomKind::Experimental { scaffolding: port },
    })
}

fn from_value(value: u128) -> (String, String, String) {
    let mut code = String::with_capacity("R/XXXX-XXXX-XXXX-XXXX".len());
    code.push_str("R/");
    let mut network_name = String::with_capacity("terracotta-scaffolding-mc-XXXX-XXXX".len());
    network_name.push_str("terracotta-scaffolding-mc-");
    let mut network_secret = String::with_capacity("XXXX-XXXX".len());

    let mut value = value;
    for i in 0..16 {
        let v = CHARS[(value % 34) as usize] as char;
        value /= 34;

        if i == 4 || i == 8 || i == 12 {
            code.push('-');
        }
        code.push(v);

        if i < 8 {
            if i == 4 {
                network_name.push('-');
            }
            network_name.push(v);
        } else {
            if i == 12 {
                network_secret.push('-');
            }
            network_secret.push(v);
        }
    }

    assert_eq!(value, 0);
    assert_eq!(code.len(), "R/XXXX-XXXX-XXXX-XXXX".len());
    assert_eq!(network_name.len(), "terracotta-scaffolding-mc-XXXX-XXXX".len());
    assert_eq!(network_secret.len(), "XXXX-XXXX".len());

    (code, network_name, network_secret)
}

pub fn start_host(room: Room, port: u16, player: Option<String>, capture: AppStateCapture) {
    let mut args = compute_arguments(&room);
    args.push("--ipv4".to_string());
    args.push("10.144.144.1".to_string());
    args.push(format!(
        "--tcp-whitelist={}",
        match room.kind {
            RoomKind::Experimental { scaffolding, .. } => scaffolding,
            _ => unreachable!(),
        },
    ));
    args.push(format!("--tcp-whitelist={}", port));
    args.push("--udp-whitelist=0".to_string());

    let easytier = easytier::FACTORY.create(args);
    let capture = {
        let Some(state) = capture.try_capture() else {
            return;
        };
        state.set(AppState::HostOk {
            room, port, easytier,
            profiles: vec![(
                SystemTime::now(),
                ProfileSnapshot {
                    machine_id: MACHINE_ID.to_string(),
                    name: player.unwrap_or("Terracotta Anonymous Host".to_string()),
                    vendor: VENDOR.to_string(),
                    kind: ProfileKind::HOST
                }.into_profile()
            )]
        })
    };

    thread::spawn(move || {
        let mut counter = 0;
        loop {
            if legacy::check_mc_conn(port) {
                counter = 0;
            } else {
                counter += 1;
                if counter >= 3 {
                    let Some(state) = capture.try_capture() else {
                        return;
                    };
                    state.set(AppState::Exception { kind: ExceptionType::PingServerRst});
                    return;
                }
            }

            let Some(mut state) = capture.try_capture() else {
                return;
            };
            let AppState::HostOk { easytier, profiles, .. } = state.as_mut_ref() else {
                unreachable!();
            };

            if !easytier.is_alive() {
                state.set(AppState::Exception { kind: ExceptionType::HostEasytierCrash });
                return;
            }

            let mut changed = false;
            let now = SystemTime::now();
            for i in (1..profiles.len()).rev() {
                let (time, profile) = &profiles[i];
                if i != 0 && now.duration_since(*time).is_ok_and(|d| d >= Duration::from_secs(10)) {
                    logging!("RoomExperiment", "Removing guest {}: timeout.", profile.get_name());
                    profiles.remove(i);
                    changed = true;
                }
            }
            if changed {
                state.increase();
            }
        }
    });
}

pub fn start_guest(room: Room, player: Option<String>, capture: AppStateCapture) {
    let port = match room.kind {
        RoomKind::Experimental { scaffolding, .. } => scaffolding,
        _ => unreachable!(),
    };
    let local_port = if let Ok(socket) = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0))
        && let Ok(address) = socket.local_addr()
    {
        address.port()
    } else {
        35782
    };

    let mut args = compute_arguments(&room);
    args.push(format!("--port-forward=tcp://0.0.0.0:{}/10.144.144.1:{}", local_port, port));
    args.push("-d".to_string());
    let easytier = easytier::FACTORY.create(args.clone());

    let capture = {
        let Some(state) = capture.try_capture() else {
            return;
        };

        state.set(AppState::GuestStarting { room, easytier })
    };

    thread::sleep(Duration::from_secs(5));

    fn fail(capture: AppStateCapture) {
        let Some(state) = capture.try_capture() else {
            return;
        };
        state.set(AppState::Exception { kind: ExceptionType::PingHostFail });
    }

    let mut session = 'session: {
        for timeout in [4, 4, 8, 4, 8, 16] {
            thread::sleep(Duration::from_secs(timeout));
            if let Ok(session) = ClientSession::open(IpAddr::V4(Ipv4Addr::LOCALHOST), local_port) {
                break 'session session;
            }

            let Some(mut state) = capture.try_capture() else {
                return;
            };
            let AppState::GuestStarting { easytier, .. } = state.as_mut_ref() else {
                unreachable!();
            };
            if !easytier.is_alive() {
                state.set(AppState::Exception { kind: ExceptionType::GuestEasytierCrash });
                return;
            }
        }

        logging!("RoomExperiment", "Cannot connect to scaffolding server.");
        fail(capture);
        return;
    };

    let Some(response) = session.send_sync(("c", "server_port"), |_| {}) else {
        fail(capture);
        return;
    };

    let port = if let PacketResponse::Ok { data } = response
        && data.len() == 2
    {
        let mut p = [0u8; 2];
        p.copy_from_slice(data.as_slice());
        u16::from_be_bytes(p)
    } else {
        fail(capture);
        return;
    };

    let local_port = {
        let Some(mut state) = capture.try_capture() else {
            return;
        };
        let AppState::GuestStarting { easytier, .. } = state.as_mut_ref() else {
            unreachable!();
        };

        let local_port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .and_then(|socket| socket.local_addr())
            .map(|address| address.port())
            .unwrap_or(35783);

        if easytier.add_port_forward(
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            local_port,
            IpAddr::V4(Ipv4Addr::new(10, 144, 144, 1)),
            port,
        ) {
            easytier.add_port_forward(
                IpAddr::V6(Ipv6Addr::UNSPECIFIED),
                local_port,
                IpAddr::V4(Ipv4Addr::new(10, 144, 144, 1)),
                port,
            );
        } else {
            state.set(AppState::Exception { kind: ExceptionType::GuestEasytierCrash });
            return;
        }

        local_port
    };

    let local = ProfileSnapshot {
        machine_id: MACHINE_ID.to_string(),
        name: player.unwrap_or("Terracotta Anonymous Guest".to_string()),
        vendor: VENDOR.to_string(),
        kind: ProfileKind::LOCAL
    }.into_profile();

    let capture = {
        let Some(state) = capture.try_capture() else {
            return;
        };
        state.replace(|state| {
            let AppState::GuestStarting { room, easytier } = state else {
                unreachable!();
            };

            AppState::GuestOk {
                room,
                easytier,
                server: FakeServer::create(local_port, crate::MOTD),
                profiles: vec![local.clone()],
            }
        })
    };

    thread::spawn(move || {
        let mut capture = capture;

        loop {
            thread::sleep(Duration::from_secs(5));

            {
                let Some(_) = session.send_sync(("c", "player_ping"), |body| {
                    serde_json::to_writer(body, &json!({
                        "machine_id": local.get_machine_id(),
                        "name": local.get_name(),
                        "vendor": local.get_vendor()
                    })).unwrap();
                }) else {
                    fail(capture);
                    return;
                };
            }

            {
                let Some(mut server_profiles) = session.send_sync(("c", "player_profiles_list"), |_| {}).map(|response| {
                    let PacketResponse::Ok { data } = response else {
                        unreachable!();
                    };
                    data
                }).and_then(|data| {
                    let mut host = false;
                    let mut local = false;

                    let mut server_players: Vec<Profile> = vec![];
                    for (machine_id, item) in serde_json::from_slice::<Value>(data.as_slice()).ok()?.as_object()? {
                        let name = item.as_object()?.get("name")?.as_str()?;
                        let vendor = item.as_object()?.get("vendor")?.as_str()?;

                        let kind = if machine_id == *MACHINE_ID {
                            if local {
                                logging!("RoomExperiment", "API c:player_profiles_list invocation failed: Multiple local player, machine_id may have conflicted.");
                                return None;
                            }
                            local = true;

                            ProfileKind::LOCAL
                        } else {
                            match item.as_object()?.get("kind")?.as_str()? {
                                "HOST" if !host => {
                                    host = true;
                                    ProfileKind::HOST
                                }
                                "GUEST" => ProfileKind::GUEST,
                                _ => return None,
                            }
                        };

                        server_players.push(ProfileSnapshot {
                            machine_id: machine_id.to_string(),
                            name: name.to_string(),
                            vendor: vendor.to_string(),
                            kind
                        }.into_profile())
                    }
                    if !host {
                        logging!("RoomExperiment", "API c:player_profiles_list invocation failed: No host detected.");
                        return None;
                    }

                    server_players.sort_by_cached_key(|profile| profile.get_machine_id().to_string());
                    Some(server_players)
                }) else {
                    fail(capture);
                    return;
                };

                let Some(mut state) = capture.try_capture() else {
                    return;
                };
                let AppState::GuestOk { easytier, profiles, .. } = state.as_mut_ref() else {
                    unreachable!();
                };
                if !easytier.is_alive() {
                    state.set(AppState::Exception { kind: ExceptionType::GuestEasytierCrash });
                    return;
                }

                if server_profiles.binary_search_by_key(&*MACHINE_ID, |profile| profile.get_machine_id()).is_err() {
                    server_profiles.push(local.clone());
                }

                let mut used = vec![false; server_profiles.len()];
                let mut changed = false;
                for i in (0..profiles.len()).rev() {
                    let profile = &mut profiles[i];
                    match profile.get_kind() {
                        ProfileKind::HOST => match server_profiles.binary_search_by_key(&profile.get_machine_id(), |p| p.get_machine_id()) {
                            Ok(index) if !used[index] && *server_profiles[index].get_kind() == ProfileKind::HOST => {
                                used[index] = true;
                                if profile.get_name() != server_profiles[index].get_name() {
                                    profile.set_name(server_profiles[index].get_name().to_string());
                                    changed = true;
                                }
                            },
                            _ => {
                                logging!("RoomExperiment", "API c:player_profiles_list invocation failed: Host Profile is consumed or invalid, machine_id may have conflict.");
                                state.set(AppState::Exception { kind: ExceptionType::ScaffoldingInvalidResponse });
                                return;
                            }
                        },
                        ProfileKind::LOCAL => {},
                        ProfileKind::GUEST => match server_profiles.binary_search_by_key(&profile.get_machine_id(), |p| p.get_machine_id()) {
                            Ok(index) if used[index] && *server_profiles[index].get_kind() == ProfileKind::GUEST => {
                                profiles.remove(i);
                                changed = true;
                            }
                            Ok(index) if *server_profiles[index].get_kind() == ProfileKind::GUEST => {
                                used[index] = true;
                                if profile.get_name() != server_profiles[index].get_name() {
                                    profile.set_name(server_profiles[index].get_name().to_string());
                                    changed = true;
                                }
                            }
                            Ok(_) => {
                                logging!("RoomExperiment", "API c:player_profiles_list invocation failed: Guest Profile type is changed, machine_id may have conflict.");
                                state.set(AppState::Exception { kind: ExceptionType::ScaffoldingInvalidResponse });
                                return;
                            }
                            Err(_) => {
                                profiles.remove(i);
                                changed = true;
                            }
                        },
                    }
                }
                for i in (0..server_profiles.len()).rev() {
                    let profile = server_profiles.pop().unwrap();
                    if !used[i] && *profile.get_kind() != ProfileKind::LOCAL {
                        profiles.push(profile);
                        changed = true;
                    }
                }
                if changed {
                    capture = state.increase();
                }
            }
        }
    });
}

fn compute_arguments(room: &Room) -> Vec<String> {
    static REPLAY_SERVERS: [&str; 10] = [
        "tcp://public.easytier.top:11010",
        "tcp://ah.nkbpal.cn:11010",
        "tcp://turn.hb.629957.xyz:11010",
        "tcp://turn.js.629957.xyz:11012",
        "tcp://sh.993555.xyz:11010",
        "tcp://turn.bj.629957.xyz:11010",
        "tcp://et.sh.suhoan.cn:11010",
        "tcp://et-hk.clickor.click:11010",
        "tcp://et.01130328.xyz:11010",
        "tcp://et.gbc.moe:11011",
    ];
    static DEFAULT_ARGUMENTS: [&str; 5] = [
        "--no-tun",
        "--compression=zstd",
        "--multi-thread",
        "--latency-first",
        "--enable-kcp-proxy",
    ];

    let mut args = vec![
        "--network-name".to_string(),
        room.network_name.clone(),
        "--network-secret".to_string(),
        room.network_secret.clone(),
    ];

    for replay in REPLAY_SERVERS.iter() {
        args.push("-p".to_string());
        args.push(replay.to_string());
    }
    for arg in DEFAULT_ARGUMENTS.iter() {
        args.push(arg.to_string());
    }
    args
}