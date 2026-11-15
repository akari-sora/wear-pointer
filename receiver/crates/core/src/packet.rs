//! Protocol v1 codec. Layout is specified in `protocol/SPEC.md`.

use std::fmt;

pub const VERSION: u8 = 1;
pub const SERVICE_UUID: &str = "7a1e0001-4c3b-4e2a-9f10-6d5c4b3a2910";
pub const POINTER_UUID: &str = "7a1e0002-4c3b-4e2a-9f10-6d5c4b3a2910";
pub const CONTROL_UUID: &str = "7a1e0003-4c3b-4e2a-9f10-6d5c4b3a2910";
pub const MAX_PACKET_LEN: usize = 20;
pub const HEADER_LEN: usize = 8;
pub const SENSOR_HZ: u32 = 100;
pub const CURSOR_HZ: u32 = 240;
pub const HOLD_TO_DRAG_MS: u32 = 350;
pub const BASE_DPI: u16 = 1000;
pub const DEFAULT_DPI: u16 = 1600;
pub const MIN_DPI: u16 = 400;
pub const MAX_DPI: u16 = 3200;
pub const DPI_STEP: u16 = 200;

pub const KIND_AIM: u8 = 0;
pub const KIND_LEFT_CLICK: u8 = 1;
pub const KIND_RIGHT_CLICK: u8 = 2;
pub const KIND_SCROLL: u8 = 3;
pub const KIND_CENTER: u8 = 4;
pub const KIND_PAUSE: u8 = 5;
pub const KIND_INFO: u8 = 6;
pub const KIND_DRAG: u8 = 7;
pub const KIND_STATS: u8 = 8;
pub const KIND_SENSITIVITY: u8 = 9;

pub const CONTROL_REQUEST_STATS: u8 = 0x01;

#[derive(Clone, Debug, PartialEq)]
pub struct Header {
    pub version: u8,
    pub kind: u8,
    pub sequence: u16,
    pub timestamp_us: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Packet {
    Aim {
        header: Header,
        left_cdeg: i16,
        up_cdeg: i16,
    },
    LeftClick {
        header: Header,
    },
    RightClick {
        header: Header,
    },
    Scroll {
        header: Header,
        delta_px: i16,
    },
    Center {
        header: Header,
    },
    Pause {
        header: Header,
        paused: bool,
    },
    Info {
        header: Header,
        battery_pct: u8,
        charging: bool,
        sensor_hz_x10: u16,
        name: String,
    },
    Drag {
        header: Header,
        down: bool,
    },
    Stats {
        header: Header,
        samples: u16,
        notifies_ok: u16,
        notifies_drop: u16,
        window_ms: u16,
    },
    Sensitivity {
        header: Header,
        dpi: u16,
    },
    Unknown {
        header: Header,
    },
}

impl Packet {
    pub fn header(&self) -> &Header {
        match self {
            Packet::Aim { header, .. }
            | Packet::LeftClick { header }
            | Packet::RightClick { header }
            | Packet::Scroll { header, .. }
            | Packet::Center { header }
            | Packet::Pause { header, .. }
            | Packet::Info { header, .. }
            | Packet::Drag { header, .. }
            | Packet::Stats { header, .. }
            | Packet::Sensitivity { header, .. }
            | Packet::Unknown { header } => header,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PacketError {
    TooShort,
    BadVersion(u8),
    Truncated { kind: u8, len: usize },
}

impl fmt::Display for PacketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PacketError::TooShort => write!(f, "packet shorter than {HEADER_LEN} bytes"),
            PacketError::BadVersion(v) => write!(f, "unsupported protocol version {v}"),
            PacketError::Truncated { kind, len } => {
                write!(f, "kind {kind} truncated at {len} bytes")
            }
        }
    }
}

impl std::error::Error for PacketError {}

pub fn encode(packet: &Packet) -> Vec<u8> {
    let header = packet.header();
    let mut out = Vec::with_capacity(MAX_PACKET_LEN);
    out.push(header.version);
    out.push(header.kind);
    out.extend_from_slice(&header.sequence.to_le_bytes());
    out.extend_from_slice(&header.timestamp_us.to_le_bytes());
    match packet {
        Packet::Aim {
            left_cdeg, up_cdeg, ..
        } => {
            out.extend_from_slice(&left_cdeg.to_le_bytes());
            out.extend_from_slice(&up_cdeg.to_le_bytes());
        }
        Packet::Scroll { delta_px, .. } => out.extend_from_slice(&delta_px.to_le_bytes()),
        Packet::Pause { paused, .. } => out.push(u8::from(*paused)),
        Packet::Drag { down, .. } => out.push(u8::from(*down)),
        Packet::Info {
            battery_pct,
            charging,
            sensor_hz_x10,
            name,
            ..
        } => {
            out.push(*battery_pct);
            out.push(u8::from(*charging));
            out.extend_from_slice(&sensor_hz_x10.to_le_bytes());
            out.extend_from_slice(&truncate_utf8(name, 8));
        }
        Packet::Stats {
            samples,
            notifies_ok,
            notifies_drop,
            window_ms,
            ..
        } => {
            out.extend_from_slice(&samples.to_le_bytes());
            out.extend_from_slice(&notifies_ok.to_le_bytes());
            out.extend_from_slice(&notifies_drop.to_le_bytes());
            out.extend_from_slice(&window_ms.to_le_bytes());
        }
        Packet::Sensitivity { dpi, .. } => {
            out.extend_from_slice(&dpi.to_le_bytes());
        }
        Packet::LeftClick { .. } | Packet::RightClick { .. } | Packet::Center { .. } => {}
        Packet::Unknown { .. } => {}
    }
    debug_assert!(out.len() <= MAX_PACKET_LEN);
    out
}

pub fn decode(bytes: &[u8]) -> Result<Packet, PacketError> {
    if bytes.len() < HEADER_LEN {
        return Err(PacketError::TooShort);
    }
    let header = Header {
        version: bytes[0],
        kind: bytes[1],
        sequence: u16::from_le_bytes([bytes[2], bytes[3]]),
        timestamp_us: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
    };
    if header.version != VERSION {
        return Err(PacketError::BadVersion(header.version));
    }
    let need = |n: usize| -> Result<(), PacketError> {
        if bytes.len() < n {
            Err(PacketError::Truncated {
                kind: header.kind,
                len: bytes.len(),
            })
        } else {
            Ok(())
        }
    };
    Ok(match header.kind {
        KIND_AIM => {
            need(12)?;
            Packet::Aim {
                header,
                left_cdeg: i16::from_le_bytes([bytes[8], bytes[9]]),
                up_cdeg: i16::from_le_bytes([bytes[10], bytes[11]]),
            }
        }
        KIND_LEFT_CLICK => Packet::LeftClick { header },
        KIND_RIGHT_CLICK => Packet::RightClick { header },
        KIND_SCROLL => {
            need(10)?;
            Packet::Scroll {
                header,
                delta_px: i16::from_le_bytes([bytes[8], bytes[9]]),
            }
        }
        KIND_CENTER => Packet::Center { header },
        KIND_PAUSE => {
            need(9)?;
            Packet::Pause {
                header,
                paused: bytes[8] != 0,
            }
        }
        KIND_INFO => {
            need(12)?;
            let name = String::from_utf8_lossy(&bytes[12..bytes.len().min(MAX_PACKET_LEN)])
                .trim_matches('\0')
                .to_string();
            Packet::Info {
                header,
                battery_pct: bytes[8],
                charging: bytes[9] & 0x01 != 0,
                sensor_hz_x10: u16::from_le_bytes([bytes[10], bytes[11]]),
                name,
            }
        }
        KIND_DRAG => {
            need(9)?;
            Packet::Drag {
                header,
                down: bytes[8] != 0,
            }
        }
        KIND_STATS => {
            need(16)?;
            Packet::Stats {
                header,
                samples: u16::from_le_bytes([bytes[8], bytes[9]]),
                notifies_ok: u16::from_le_bytes([bytes[10], bytes[11]]),
                notifies_drop: u16::from_le_bytes([bytes[12], bytes[13]]),
                window_ms: u16::from_le_bytes([bytes[14], bytes[15]]),
            }
        }
        KIND_SENSITIVITY => {
            need(10)?;
            Packet::Sensitivity {
                header,
                dpi: u16::from_le_bytes([bytes[8], bytes[9]]),
            }
        }
        _ => Packet::Unknown { header },
    })
}

pub fn control(command: u8) -> [u8; 1] {
    [command]
}

fn truncate_utf8(text: &str, max: usize) -> Vec<u8> {
    let mut end = text.len().min(max);
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.as_bytes()[..end].to_vec()
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn header(kind: u8, sequence: u16, timestamp_us: u32) -> Header {
    Header {
        version: VERSION,
        kind,
        sequence,
        timestamp_us,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aim_packet() -> Packet {
        Packet::Aim {
            header: header(KIND_AIM, 1, 1000),
            left_cdeg: 1234,
            up_cdeg: -50,
        }
    }

    #[test]
    fn aim_vector_matches_the_spec() {
        let bytes = encode(&aim_packet());
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, "01000100e8030000d204ceff");
        assert_eq!(decode(&bytes).unwrap(), aim_packet());
        assert!(bytes.len() <= MAX_PACKET_LEN);
    }

    #[test]
    fn constants_match_protocol_json() {
        let raw = include_str!("../../../../protocol/v1.json");
        let json: serde_json::Value = serde_json::from_str(raw).unwrap();
        assert_eq!(json["version"], VERSION);
        assert_eq!(json["service_uuid"], SERVICE_UUID);
        assert_eq!(json["pointer_uuid"], POINTER_UUID);
        assert_eq!(json["control_uuid"], CONTROL_UUID);
        assert_eq!(json["sensor_hz"], SENSOR_HZ);
        assert_eq!(json["cursor_hz"], CURSOR_HZ);
        assert_eq!(json["hold_to_drag_ms"], HOLD_TO_DRAG_MS);
        assert_eq!(json["base_dpi"], BASE_DPI);
        assert_eq!(json["default_dpi"], DEFAULT_DPI);
        assert_eq!(json["min_dpi"], MIN_DPI);
        assert_eq!(json["max_dpi"], MAX_DPI);
        assert_eq!(json["dpi_step"], DPI_STEP);
        assert_eq!(json["max_packet_bytes"], MAX_PACKET_LEN);
        assert_eq!(json["aim_vector_hex"], "01000100e8030000d204ceff");
        assert_eq!(json["kinds"]["aim"], KIND_AIM);
        assert_eq!(json["kinds"]["stats"], KIND_STATS);
        assert_eq!(json["kinds"]["sensitivity"], KIND_SENSITIVITY);
        assert_eq!(
            json["control_commands"]["request_stats"],
            CONTROL_REQUEST_STATS
        );
    }

    #[test]
    fn round_trip_every_kind() {
        let samples = vec![
            Packet::LeftClick {
                header: header(KIND_LEFT_CLICK, 2, 5),
            },
            Packet::RightClick {
                header: header(KIND_RIGHT_CLICK, 3, 6),
            },
            Packet::Scroll {
                header: header(KIND_SCROLL, 4, 7),
                delta_px: -12,
            },
            Packet::Center {
                header: header(KIND_CENTER, 5, 8),
            },
            Packet::Pause {
                header: header(KIND_PAUSE, 6, 9),
                paused: true,
            },
            Packet::Drag {
                header: header(KIND_DRAG, 7, 10),
                down: true,
            },
            Packet::Info {
                header: header(KIND_INFO, 8, 11),
                battery_pct: 80,
                charging: true,
                sensor_hz_x10: 1000,
                name: "Watch7".into(),
            },
            Packet::Stats {
                header: header(KIND_STATS, 9, 12),
                samples: 100,
                notifies_ok: 97,
                notifies_drop: 3,
                window_ms: 1000,
            },
            Packet::Sensitivity {
                header: header(KIND_SENSITIVITY, 10, 13),
                dpi: 1600,
            },
        ];
        for packet in samples {
            let bytes = encode(&packet);
            assert!(bytes.len() <= MAX_PACKET_LEN);
            assert_eq!(decode(&bytes).unwrap(), packet);
        }
    }

    #[test]
    fn rejects_short_and_foreign_versions_and_keeps_unknown_kinds() {
        assert_eq!(decode(&[1, 0, 0]).unwrap_err(), PacketError::TooShort);
        assert!(matches!(
            decode(&[2, 0, 0, 0, 0, 0, 0, 0]).unwrap_err(),
            PacketError::BadVersion(2)
        ));
        let unknown = decode(&[1, 99, 4, 0, 0, 0, 0, 0, 1, 2]).unwrap();
        assert!(
            matches!(unknown, Packet::Unknown { header } if header.kind == 99 && header.sequence == 4)
        );
    }

    #[test]
    fn info_name_stays_inside_twenty_bytes() {
        let packet = Packet::Info {
            header: header(KIND_INFO, 1, 1),
            battery_pct: 255,
            charging: false,
            sensor_hz_x10: 0,
            name: "a very long watch name".into(),
        };
        let bytes = encode(&packet);
        assert!(bytes.len() <= MAX_PACKET_LEN);
        let decoded = decode(&bytes).unwrap();
        match decoded {
            Packet::Info { name, .. } => assert!(name.len() <= 8),
            other => panic!("unexpected {other:?}"),
        }
    }
}
