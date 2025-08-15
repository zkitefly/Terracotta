use core::panic;
use std::{
    net::{Ipv4Addr, UdpSocket},
};

use num_bigint::BigUint;
use rand_core::{OsRng, TryRngCore};

use crate::{
    easytier::{self, Easytier},
    fakeserver::{self, FakeServer},
};

#[derive(Debug)]
pub struct Room {
    pub code: String,
    pub port: u16,
    network_name: String,
    network_secret: String,
    kind: RoomKind,
}

#[derive(Debug)]
#[derive(PartialEq)]
enum RoomKind {
    Terracotta,
    PCL2CE,
}

static CHARS: &[u8] = "0123456789ABCDEFGHJKLMNPQRSTUVWXYZ".as_bytes();

fn rem64(value: &BigUint) -> usize {
    return (value % 34u32).try_into().unwrap();
}

impl Room {
    pub fn create(port: u16) -> Room {
        let mut buffer: [u8; 15] = [0; 15];
        OsRng.try_fill_bytes(&mut buffer).unwrap();

        let mut value = BigUint::ZERO;
        for i in 0..buffer.len() {
            value = (value << 8) + buffer[i];
        }

        value = value / (65536u32) * (65536u32) + port;

        let mut name: [u8; 15] = [0; 15];
        let mut secret: [u8; 10] = [0; 10];
        let mut checking: usize = 0;
        for i in 0..15 {
            name[i] = CHARS[rem64(&value)];
            checking = (rem64(&value) + checking) % 34;
            value /= 34u32;
        }
        for i in 0..9 {
            secret[i] = CHARS[rem64(&value)];
            checking = (rem64(&value) + checking) % 34;
            value /= 34u32;
        }
        secret[9] = CHARS[checking];

        if value != BigUint::ZERO {
            panic!("Cannot generate code: There's {} remained.", value);
        }

        let mut code: [u8; 29] = [0; 29];
        code[0..5].copy_from_slice(&name[0..5]);
        code[5] = b'-';
        code[6..11].copy_from_slice(&name[5..10]);
        code[11] = b'-';
        code[12..17].copy_from_slice(&name[10..15]);
        code[17] = b'-';
        code[18..23].copy_from_slice(&secret[0..5]);
        code[23] = b'-';
        code[24..29].copy_from_slice(&secret[5..10]);

        name.make_ascii_lowercase();
        secret.make_ascii_lowercase();

        let room = Room {
            code: String::from_utf8(code.to_vec()).unwrap(),
            port: port,

            network_name: "terracotta-mc-".to_string() + str::from_utf8(&name).unwrap(),
            network_secret: String::from_utf8(secret.to_vec()).unwrap(),
            kind: RoomKind::Terracotta,
        };

        return room;
    }

    pub fn from(code: &String) -> Option<Room> {
        let chars: Vec<char> = code.to_ascii_uppercase().chars().collect();

        if chars.len() >= 29 {
            for start in 0..=(chars.len() - 29) {
                if let Some(room) = Self::from_terracotta(&chars[start..start + 29]) {
                    return Some(room);
                }
            }
        }

        if let Some(room) = Self::from_pcl2ce(&chars) {
            return Some(room);
        }

        return None;
    }

    fn from_terracotta(chars: &[char]) -> Option<Room> {
        fn lookup_char(char: char) -> Option<u8> {
            let char = match char {
                'I' => '1',
                'O' => '0',
                _ => char,
            };

            for j in 0..34 {
                if CHARS[j] as char == char {
                    return Some(j as u8);
                }
            }

            return None;
        }

        let mut array: [u8; 25] = [0; 25];
        for i in 0..5 {
            for j in 0..5 {
                if let Some(char) = lookup_char(chars[i * 6 + j]) {
                    array[i * 5 + j] = char;
                } else {
                    return None;
                }
            }

            if i != 4 && chars[i * 6 + 5] != '-' {
                return None;
            }
        }

        let mut checking: u8 = 0;
        for i in 0..24 {
            checking = (checking + array[i]) % 34;
        }
        if checking != array[24] {
            return None;
        }

        return Some(Room {
            code: {
                let mut code = String::with_capacity(29);
                for i in 0..25 {
                    code.push(CHARS[array[i] as usize] as char);
                    if i == 4 || i == 9 || i == 14 || i == 19 {
                        code.push('-');
                    }
                }
                code
            },
            port: {
                let mut value = BigUint::ZERO;
                for i in 0..25 {
                    // floor(log(34, 65536)) = 4
                    value += BigUint::from(34 as u8).pow(i as u32) * array[i];
                }

                (value % (65536 as u32)).try_into().unwrap()
            },
            network_name: {
                let mut name: [u8; 15] = [0; 15];
                for i in 0..15 {
                    name[i] = CHARS[array[i] as usize];
                }
                name.make_ascii_lowercase();
                "terracotta-mc-".to_string() + str::from_utf8(&name).unwrap()
            },
            network_secret: {
                let mut secret: [u8; 10] = [0; 10];
                for i in 0..10 {
                    secret[i] = CHARS[array[i + 15] as usize];
                }
                secret.make_ascii_lowercase();
                String::from_utf8(secret.to_vec()).unwrap()
            },
            kind: RoomKind::Terracotta,
        });
    }

    fn from_pcl2ce(chars: &[char]) -> Option<Room> {
        if chars.len() > 10 {
            return None;
        }
        let mut value = 0u64;
        for i in 0..chars.len() {
            let char = chars[i];

            value = value * 32
                + if char >= '2' && char <= '9' {
                    (char as u64) - ('2' as u64)
                } else if char >= 'A' && char <= 'H' {
                    (char as u64) - ('A' as u64) + 8
                } else if char >= 'J' && char <= 'N' {
                    (char as u64) - ('J' as u64) + 16
                } else if char >= 'P' && char <= 'Z' {
                    (char as u64) - ('P' as u64) + 21
                } else {
                    return None;
                };
        }

        println!("3: {}", value);
        if value >= 99999999_99_65536u64 {
            return None;
        }

        let s = value.to_string();
        return Some(Room {
            code: chars.iter().collect(),
            port: match s.len() {
                14 => value % 10000,
                15 => {
                    let v = value % 100000;
                    if v >= 65536 {
                        return None;
                    }
                    v
                },
                _ => return None,
            } as u16,
            network_name: String::from("PCLCELobby") + &s[0..8],
            network_secret: String::from("PCLCEETLOBBY2025") + &s[8..10],
            kind: RoomKind::PCL2CE,
        });
    }

    fn compute_network_arguments(room: &Room) -> Vec<String> {
        static REPLAY_SERVERS: [&'static str; 10] = [
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
        static DEFAULT_ARGUMENTS: [&'static str; 6] = [
            "--no-tun",
            "--compression",
            "zstd",
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

        if room.kind == RoomKind::PCL2CE {
            args.push("-p".to_string());
            args.push("tcp://43.139.42.188:11010".to_string())
        }

        for replay in REPLAY_SERVERS.iter() {
            args.push("-p".to_string());
            args.push(replay.to_string());
        }
        for arg in DEFAULT_ARGUMENTS.iter() {
            args.push(arg.to_string());
        }

        return args;
    }

    pub fn start_host(&self) -> Easytier {
        let mut args = Self::compute_network_arguments(&self);
        args.push("--ipv4".to_string());
        args.push("10.144.144.1".to_string());

        return easytier::FACTORY.create(args);
    }

    pub fn start_guest(&self, motd: &'static str) -> (Easytier, FakeServer) {
        let mut args = Self::compute_network_arguments(&self);

        let local_port = if let Ok(socket) = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
            && let Ok(address) = socket.local_addr()
        {
            address.port()
        } else {
            35781
        };

        let host_ip = match self.kind {
            RoomKind::Terracotta => "10.144.144.1",
            RoomKind::PCL2CE => "10.114.51.41",
        };

        args.push("-d".to_string());
        args.push(format!(
            "--port-forward=tcp://[::0]:{}/{}:{}",
            local_port, host_ip, self.port
        ));
        args.push(format!(
            "--port-forward=tcp://0.0.0.0:{}/{}:{}",
            local_port, host_ip, self.port
        ));

        return (
            easytier::FACTORY.create(args),
            fakeserver::create(local_port, motd),
        );
    }
}
