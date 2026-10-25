//! Sony VISCA Protocol Encoder and Types

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanDir {
    Left = 1,
    Right = 2,
    Stop = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TiltDir {
    Up = 1,
    Down = 2,
    Stop = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoomDir {
    Tele,
    Wide,
    Stop,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViscaCmd {
    PanTilt {
        pan: PanDir,
        tilt: TiltDir,
        pan_spd: u8,
        tilt_spd: u8,
    },
    PanTiltStop,
    Home,
    Reset,
    Zoom {
        dir: ZoomDir,
        speed: u8,
    },
    ZoomStop,
    PresetSet(u8),    // 1..=6
    PresetRecall(u8), // 1..=6
    Power(bool),
    Mute(bool),
    AutoExposure,
    Freeze(bool),
    IfClear,
    AddressSet,
    PowerInq,
    Raw(Vec<u8>),
}

impl ViscaCmd {
    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            ViscaCmd::PanTilt {
                pan,
                tilt,
                pan_spd,
                tilt_spd,
            } => {
                let pspd = pan_spd.clamp(&1, &24);
                let tspd = tilt_spd.clamp(&1, &20);
                vec![0x81, 0x01, 0x06, 0x01, *pspd, *tspd, *pan as u8, *tilt as u8, 0xFF]
            }
            ViscaCmd::PanTiltStop => {
                vec![0x81, 0x01, 0x06, 0x01, 0x08, 0x08, 0x03, 0x03, 0xFF]
            }
            ViscaCmd::Home => vec![0x81, 0x01, 0x06, 0x04, 0xFF],
            ViscaCmd::Reset => vec![0x81, 0x01, 0x06, 0x05, 0xFF],
            ViscaCmd::Zoom { dir, speed } => {
                let spd = speed.clamp(&0, &7);
                match dir {
                    ZoomDir::Tele => vec![0x81, 0x01, 0x04, 0x07, 0x20 | spd, 0xFF],
                    ZoomDir::Wide => vec![0x81, 0x01, 0x04, 0x07, 0x30 | spd, 0xFF],
                    ZoomDir::Stop => vec![0x81, 0x01, 0x04, 0x07, 0x00, 0xFF],
                }
            }
            ViscaCmd::ZoomStop => vec![0x81, 0x01, 0x04, 0x07, 0x00, 0xFF],
            ViscaCmd::PresetSet(num) => {
                let idx = (num.clamp(&1, &6)) - 1;
                vec![0x81, 0x01, 0x04, 0x3F, 0x01, idx, 0xFF]
            }
            ViscaCmd::PresetRecall(num) => {
                let idx = (num.clamp(&1, &6)) - 1;
                vec![0x81, 0x01, 0x04, 0x3F, 0x02, idx, 0xFF]
            }
            ViscaCmd::Power(on) => {
                let op = if *on { 0x02 } else { 0x03 };
                vec![0x81, 0x01, 0x04, 0x00, op, 0xFF]
            }
            ViscaCmd::Mute(mute) => {
                let op = if *mute { 0x02 } else { 0x03 };
                vec![0x81, 0x01, 0x04, 0x75, op, 0xFF]
            }
            ViscaCmd::AutoExposure => vec![0x81, 0x01, 0x04, 0x39, 0x00, 0xFF],
            ViscaCmd::Freeze(freeze) => {
                let op = if *freeze { 0x02 } else { 0x03 };
                vec![0x81, 0x01, 0x04, 0x62, op, 0xFF]
            }
            ViscaCmd::IfClear => vec![0x88, 0x01, 0x00, 0x01, 0xFF],
            ViscaCmd::AddressSet => vec![0x88, 0x30, 0x01, 0xFF],
            ViscaCmd::PowerInq => vec![0x81, 0x09, 0x04, 0x00, 0xFF],
            ViscaCmd::Raw(bytes) => bytes.clone(),
        }
    }
}

/// Wraps a raw VISCA command into standard Sony/PTZOptics VISCA-over-IP UDP encapsulation.
/// Format: [Payload Type: 2B][Payload Length: 2B][Sequence: 4B][Raw VISCA]
pub fn wrap_visca_ip(visca_bytes: &[u8], seq: u32) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(8 + visca_bytes.len());
    let is_inquiry = visca_bytes.len() >= 2 && visca_bytes[1] == 0x09;
    let payload_type: u16 = if is_inquiry { 0x0110 } else { 0x0100 };
    pkt.extend_from_slice(&payload_type.to_be_bytes());
    pkt.extend_from_slice(&(visca_bytes.len() as u16).to_be_bytes());
    pkt.extend_from_slice(&seq.to_be_bytes());
    pkt.extend_from_slice(visca_bytes);
    pkt
}

/// Extracts raw VISCA bytes from a VISCA-over-IP UDP reply packet.
pub fn extract_visca_ip_payload(packet: &[u8]) -> &[u8] {
    if packet.len() >= 8 && (packet[0] == 0x01 || packet[0] == 0x02) {
        let len = u16::from_be_bytes([packet[2], packet[3]]) as usize;
        let end = (8 + len).min(packet.len());
        &packet[8..end]
    } else {
        packet
    }
}
