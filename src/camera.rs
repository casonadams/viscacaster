//! Universal VISCA Transport Manager (Serial RS-232/USB & VISCA-over-IP UDP)

use std::io::{Read, Write};
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use serde::{Deserialize, Serialize};

use crate::visca::{extract_visca_ip_payload, wrap_visca_ip, ViscaCmd};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectionTarget {
    Serial(String),
    Ip { host: String, port: u16 },
}

impl ConnectionTarget {
    pub fn display_label(&self) -> String {
        match self {
            ConnectionTarget::Serial(p) => format!("Serial: {}", p),
            ConnectionTarget::Ip { host, port } => format!("IP: {}:{}", host, port),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortInfo {
    pub port_name: String,
    pub display_label: String,
    pub is_known_adapter: bool,
}

/// Lists all serial ports on the system with detailed hardware metadata
pub fn list_serial_ports() -> Vec<PortInfo> {
    match serialport::available_ports() {
        Ok(ports) => {
            let mut list: Vec<PortInfo> = ports
                .into_iter()
                .filter(|p| {
                    // On Linux, filter out dummy unpopulated motherboard ttyS1..31
                    #[cfg(target_os = "linux")]
                    if p.port_name.starts_with("/dev/ttyS") && p.port_name != "/dev/ttyS0" {
                        return false;
                    }
                    true
                })
                .map(|p| {
                    let mut desc = p.port_name.clone();
                    let mut is_known = false;

                    if let serialport::SerialPortType::UsbPort(usb) = &p.port_type {
                        let mut details = Vec::new();
                        if let Some(prod) = &usb.product {
                            details.push(prod.trim().to_string());
                        } else if let Some(mfg) = &usb.manufacturer {
                            details.push(mfg.trim().to_string());
                        }

                        // Known USB serial converter VIDs
                        // 0x06cd: Keyspan (USA-19HS)
                        // 0x0403: FTDI
                        // 0x10c4: Silicon Labs CP210x
                        // 0x1a86: WCH CH340
                        // 0x067b: Prolific PL2303
                        if [0x06cd, 0x0403, 0x10c4, 0x1a86, 0x067b].contains(&usb.vid) {
                            is_known = true;
                        }

                        if !details.is_empty() {
                            desc = format!("{} ({})", p.port_name, details.join(" "));
                        }
                    }

                    PortInfo {
                        port_name: p.port_name,
                        display_label: desc,
                        is_known_adapter: is_known,
                    }
                })
                .collect();

            // Prioritize USB adapters over legacy motherboard ports
            list.sort_by(|a, b| {
                let a_is_usb = a.is_known_adapter || a.port_name.contains("USB") || a.port_name.contains("usb");
                let b_is_usb = b.is_known_adapter || b.port_name.contains("USB") || b.port_name.contains("usb");
                b_is_usb
                    .cmp(&a_is_usb)
                    .then(b.is_known_adapter.cmp(&a.is_known_adapter))
                    .then(a.port_name.cmp(&b.port_name))
            });
            list
        }
        Err(_) => Vec::new(),
    }
}

/// Probes a specific serial port with a quick VISCA handshake
pub fn probe_visca_camera(port: &str) -> bool {
    let mut p = match serialport::new(port, 9600)
        .data_bits(serialport::DataBits::Eight)
        .parity(serialport::Parity::None)
        .stop_bits(serialport::StopBits::One)
        .flow_control(serialport::FlowControl::None)
        .timeout(Duration::from_millis(120))
        .open()
    {
        Ok(port) => port,
        Err(_) => return false,
    };

    // Send IF_Clear + AddressSet + PowerInq
    let _ = p.write_all(&ViscaCmd::IfClear.to_bytes());
    thread::sleep(Duration::from_millis(40));
    let _ = p.write_all(&ViscaCmd::AddressSet.to_bytes());
    thread::sleep(Duration::from_millis(40));
    let _ = p.write_all(&ViscaCmd::PowerInq.to_bytes());

    let mut buf = [0u8; 32];
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(250) {
        if let Ok(n) = p.read(&mut buf) {
            if n > 0 && buf[..n].contains(&0xFF) {
                return true;
            }
        }
        thread::sleep(Duration::from_millis(10));
    }
    false
}

/// Automatically finds the best camera serial port:
/// 1. Probes candidate ports for active VISCA responses
/// 2. If no response (camera asleep), picks known USB serial adapter (Keyspan, FTDI, etc.)
/// 3. Falls back to first available port
pub fn auto_detect_camera_port() -> Option<String> {
    let ports = list_serial_ports();
    if ports.is_empty() {
        return None;
    }

    // Step 1: Active probe
    for p in &ports {
        if probe_visca_camera(&p.port_name) {
            return Some(p.port_name.clone());
        }
    }

    // Step 2: Recognized hardware adapter (e.g. Keyspan USA-19HS) or USB serial
    for p in &ports {
        if p.is_known_adapter || p.port_name.contains("USB") || p.port_name.contains("usb") {
            return Some(p.port_name.clone());
        }
    }

    // Step 3: Default to first available port
    ports.first().map(|p| p.port_name.clone())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredCamera {
    pub ip: String,
    pub port: u16,
    pub display_name: String,
}

/// Scans the local network for IP PTZ cameras using VISCA broadcast and ONVIF discovery
pub fn discover_ip_cameras() -> Vec<DiscoveredCamera> {
    let mut found = Vec::new();
    let mut seen_ips = std::collections::HashSet::new();

    // 1. VISCA-over-IP UDP broadcast inquiry on ports 52381 and 1259
    if let Ok(socket) = UdpSocket::bind("0.0.0.0:0") {
        let _ = socket.set_broadcast(true);
        let _ = socket.set_read_timeout(Some(Duration::from_millis(250)));

        let inq_packet = wrap_visca_ip(&ViscaCmd::PowerInq.to_bytes(), 1);
        for target in ["255.255.255.255:52381", "255.255.255.255:1259"] {
            let _ = socket.send_to(&inq_packet, target);
        }

        let mut buf = [0u8; 128];
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(350) {
            if let Ok((n, src)) = socket.recv_from(&mut buf) {
                if n > 0 {
                    let ip = src.ip().to_string();
                    if seen_ips.insert(ip.clone()) {
                        found.push(DiscoveredCamera {
                            ip: ip.clone(),
                            port: src.port(),
                            display_name: format!("VISCA Camera ({}:{})", ip, src.port()),
                        });
                    }
                }
            }
        }
    }

    // 2. WS-Discovery (ONVIF Multicast Probe on 239.255.255.250:3702)
    if let Ok(socket) = UdpSocket::bind("0.0.0.0:0") {
        let _ = socket.set_broadcast(true);
        let _ = socket.set_read_timeout(Some(Duration::from_millis(300)));

        let ws_probe = r#"<?xml version="1.0" encoding="utf-8"?>
<Envelope xmlns="http://schemas.xmlsoap.org/ws/2004/08/addressing" xmlns:d="http://schemas.xmlsoap.org/ws/2005/04/discovery">
  <Header>
    <Action>http://schemas.xmlsoap.org/ws/2005/04/discovery/Probe</Action>
    <MessageID>urn:uuid:12345678-1234-5678-1234-567812345678</MessageID>
    <To>urn:schemas-xmlsoap-org:ws:2005:04:discovery</To>
  </Header>
  <Body>
    <d:Probe/>
  </Body>
</Envelope>"#;

        let _ = socket.send_to(ws_probe.as_bytes(), "239.255.255.250:3702");

        let mut buf = [0u8; 2048];
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(350) {
            if let Ok((n, src)) = socket.recv_from(&mut buf) {
                if n > 0 {
                    let ip = src.ip().to_string();
                    if seen_ips.insert(ip.clone()) {
                        found.push(DiscoveredCamera {
                            ip: ip.clone(),
                            port: 52381,
                            display_name: format!("Network Camera ({})", ip),
                        });
                    }
                }
            }
        }
    }

    found
}

pub struct CameraManager {
    cmd_sender: Sender<ViscaCmd>,
    connected: Arc<AtomicBool>,
    power_on: Arc<AtomicBool>,
    pub _target: ConnectionTarget,
}

impl CameraManager {
    pub fn new(target: ConnectionTarget) -> Self {
        let (tx, rx) = mpsc::channel();
        let connected = Arc::new(AtomicBool::new(false));
        let power_on = Arc::new(AtomicBool::new(true));

        let connected_clone = Arc::clone(&connected);
        let power_on_clone = Arc::clone(&power_on);
        let target_clone = target.clone();

        thread::Builder::new()
            .name("visca-worker".into())
            .spawn(move || {
                worker_thread(target_clone, rx, connected_clone, power_on_clone);
            })
            .expect("Failed to spawn VISCA worker thread");

        Self {
            cmd_sender: tx,
            connected,
            power_on,
            _target: target,
        }
    }

    pub fn send(&self, cmd: ViscaCmd) {
        let _ = self.cmd_sender.send(cmd);
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    pub fn is_power_on(&self) -> bool {
        self.power_on.load(Ordering::Relaxed)
    }

    pub fn toggle_power(&self) -> bool {
        let current = self.is_power_on();
        let new_state = !current;
        self.power_on.store(new_state, Ordering::Relaxed);
        if new_state {
            self.send(ViscaCmd::IfClear);
            self.send(ViscaCmd::AddressSet);
            self.send(ViscaCmd::Power(true));
            self.send(ViscaCmd::Mute(false));
            self.send(ViscaCmd::AutoExposure);
        } else {
            self.send(ViscaCmd::Power(false));
        }
        new_state
    }

    pub fn unmute(&self) {
        self.send(ViscaCmd::Mute(false));
        self.send(ViscaCmd::AutoExposure);
        self.send(ViscaCmd::Freeze(false));
    }
}

fn worker_thread(
    target: ConnectionTarget,
    rx: Receiver<ViscaCmd>,
    connected: Arc<AtomicBool>,
    power_on: Arc<AtomicBool>,
) {
    match target {
        ConnectionTarget::Serial(port_name) => {
            serial_worker_loop(port_name, rx, connected, power_on);
        }
        ConnectionTarget::Ip { host, port } => {
            ip_worker_loop(host, port, rx, connected, power_on);
        }
    }
}

fn serial_worker_loop(
    port_name: String,
    rx: Receiver<ViscaCmd>,
    connected: Arc<AtomicBool>,
    power_on: Arc<AtomicBool>,
) {
    let mut port: Option<Box<dyn serialport::SerialPort>> = None;
    let mut read_buf = [0u8; 64];

    loop {
        if port.is_none() {
            match serialport::new(&port_name, 9600)
                .data_bits(serialport::DataBits::Eight)
                .parity(serialport::Parity::None)
                .stop_bits(serialport::StopBits::One)
                .flow_control(serialport::FlowControl::None)
                .timeout(Duration::from_millis(50))
                .open()
            {
                Ok(mut p) => {
                    connected.store(true, Ordering::Relaxed);

                    // Initial handshake
                    let _ = p.write_all(&ViscaCmd::IfClear.to_bytes());
                    thread::sleep(Duration::from_millis(50));
                    let _ = p.write_all(&ViscaCmd::AddressSet.to_bytes());
                    thread::sleep(Duration::from_millis(50));
                    let _ = p.write_all(&ViscaCmd::Power(true).to_bytes());
                    thread::sleep(Duration::from_millis(200));
                    let _ = p.write_all(&ViscaCmd::Mute(false).to_bytes());
                    let _ = p.write_all(&ViscaCmd::AutoExposure.to_bytes());

                    port = Some(p);
                }
                Err(_) => {
                    connected.store(false, Ordering::Relaxed);
                    thread::sleep(Duration::from_millis(800));
                }
            }
        }

        if let Some(p) = port.as_mut() {
            while let Ok(cmd) = rx.try_recv() {
                let bytes = cmd.to_bytes();
                if let Err(_) = p.write_all(&bytes) {
                    port = None;
                    connected.store(false, Ordering::Relaxed);
                    break;
                }
            }

            if let Some(p) = port.as_mut() {
                match p.read(&mut read_buf) {
                    Ok(n) if n > 0 => {
                        if n >= 4 && read_buf[0] == 0x90 && read_buf[1] == 0x50 {
                            if read_buf[2] == 0x02 {
                                power_on.store(true, Ordering::Relaxed);
                            } else if read_buf[2] == 0x03 {
                                power_on.store(false, Ordering::Relaxed);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        thread::sleep(Duration::from_millis(10));
    }
}

fn ip_worker_loop(
    host: String,
    port: u16,
    rx: Receiver<ViscaCmd>,
    connected: Arc<AtomicBool>,
    power_on: Arc<AtomicBool>,
) {
    let target_addr = format!("{}:{}", host, port);
    let socket = match UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => {
            let _ = s.set_read_timeout(Some(Duration::from_millis(50)));
            s
        }
        Err(e) => {
            eprintln!("[VISCA-IP] Failed to bind local UDP socket: {}", e);
            connected.store(false, Ordering::Relaxed);
            return;
        }
    };

    let mut seq: u32 = 1;
    let mut read_buf = [0u8; 128];

    // Sony VISCA-over-IP control message: Reset sequence number
    let reset_seq_pkt = [0x02, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x01];
    let _ = socket.send_to(&reset_seq_pkt, &target_addr);
    thread::sleep(Duration::from_millis(50));

    // Send IF_Clear + AddressSet + Power(true) over IP
    let _ = socket.send_to(&wrap_visca_ip(&ViscaCmd::IfClear.to_bytes(), seq), &target_addr);
    seq += 1;
    thread::sleep(Duration::from_millis(40));
    let _ = socket.send_to(&wrap_visca_ip(&ViscaCmd::AddressSet.to_bytes(), seq), &target_addr);
    seq += 1;
    thread::sleep(Duration::from_millis(40));
    let _ = socket.send_to(&wrap_visca_ip(&ViscaCmd::Power(true).to_bytes(), seq), &target_addr);
    seq += 1;

    connected.store(true, Ordering::Relaxed);

    loop {
        // Drain commands
        while let Ok(cmd) = rx.try_recv() {
            let raw = cmd.to_bytes();
            let ip_packet = wrap_visca_ip(&raw, seq);
            seq = seq.wrapping_add(1);
            if let Err(_) = socket.send_to(&ip_packet, &target_addr) {
                connected.store(false, Ordering::Relaxed);
            }
        }

        // Drain responses
        if let Ok((n, _)) = socket.recv_from(&mut read_buf) {
            if n > 0 {
                connected.store(true, Ordering::Relaxed);
                let inner = extract_visca_ip_payload(&read_buf[..n]);
                if inner.len() >= 4 && inner[0] == 0x90 && inner[1] == 0x50 {
                    if inner[2] == 0x02 {
                        power_on.store(true, Ordering::Relaxed);
                    } else if inner[2] == 0x03 {
                        power_on.store(false, Ordering::Relaxed);
                    }
                }
            }
        }

        thread::sleep(Duration::from_millis(10));
    }
}
