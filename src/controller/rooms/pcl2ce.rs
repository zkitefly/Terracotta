use crate::controller::{Room, RoomKind};
pub fn parse(code: &str) -> Option<Room> {
    let chars: Vec<char> = code.to_ascii_uppercase().chars().collect();

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
    if s.len() != 14 && s.len() != 15 {
        return None;
    }
    Some(Room {
        code: chars.iter().collect(),
        network_name: String::from("PCLCELobby") + &s[0..8],
        network_secret: String::from("PCLCEETLOBBY2025") + &s[8..10],
        kind: RoomKind::PCL2CE {
            mc_port: match s.len() {
                14 => value % 10000,
                15 => {
                    let v = value % 100000;
                    if v >= 65536 {
                        return None;
                    }
                    v
                }
                _ => return None,
            } as u16,
        },
    })
}
