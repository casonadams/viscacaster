//! Cross-Platform Broadcast GUI for Sony VISCA PTZ Camera & Church Presentation Overlays

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};
use eframe::egui::{self, Color32, CornerRadius, Key, RichText, Sense, Stroke, Vec2};

use crate::camera::{auto_detect_camera_port, discover_ip_cameras, list_serial_ports, CameraManager, ConnectionTarget, DiscoveredCamera, PortInfo};
use crate::config::{load_config, save_config, AppConfig};
use crate::hymns::{get_hymn_lyrics, HymnCatalog, HymnEntry, HymnLyrics};
use crate::overlay::{HymnOverlayData, LowerThirdData, OverlayServer, TickerData};
use crate::visca::{PanDir, TiltDir, ViscaCmd, ZoomDir};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppTab {
    Camera,
    Hymns,
    Overlays,
    ObsSetup,
}

pub struct PTZApp {
    config: AppConfig,
    camera: Option<CameraManager>,
    available_ports: Vec<PortInfo>,
    selected_port: String,
    connection_target: ConnectionTarget,
    ip_host: String,
    ip_port: u16,
    use_ip_mode: bool,
    active_tab: AppTab,

    // Runtime state - PTZ
    active_pan_state: (PanDir, TiltDir),
    active_zoom_state: ZoomDir,
    editing_preset: Option<u8>,
    edit_buf: String,
    action_text: String,
    action_expiry: Option<Instant>,
    last_ports_refresh: Instant,
    marked_flash: Option<(u8, Instant)>,

    // Church Presentation & Overlays
    overlay_server: OverlayServer,
    hymn_catalog: HymnCatalog,
    hymn_search: String,
    loaded_hymn: Option<HymnLyrics>,
    current_verse_idx: usize,
    hymn_live: bool,
    hymn_loading: bool,
    hymn_rx: Receiver<Result<HymnLyrics, String>>,
    hymn_tx: Sender<Result<HymnLyrics, String>>,

    // Lower Thirds
    speaker_name: String,
    sermon_title: String,
    lt_live: bool,
    auto_lt_on_preset1: bool,
    lt_auto_hide_at: Option<Instant>,

    // Ticker
    ticker_text: String,
    ticker_live: bool,
    discovered_ip_cameras: Vec<DiscoveredCamera>,
    ip_scanning: bool,
    ip_scan_rx: Receiver<Vec<DiscoveredCamera>>,
    ip_scan_tx: Sender<Vec<DiscoveredCamera>>,
}

impl PTZApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(18, 18, 24);
        visuals.window_fill = Color32::from_rgb(18, 18, 24);
        visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(26, 26, 34);
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(42, 42, 54));
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(70, 70, 88);
        visuals.widgets.inactive.bg_stroke = Stroke::NONE;
        visuals.widgets.inactive.corner_radius = CornerRadius::same(6);
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(56, 189, 248);
        visuals.widgets.hovered.bg_stroke = Stroke::NONE;
        visuals.widgets.hovered.corner_radius = CornerRadius::same(6);
        visuals.widgets.active.bg_fill = Color32::from_rgb(37, 99, 235);
        visuals.widgets.active.corner_radius = CornerRadius::same(6);
        cc.egui_ctx.set_visuals(visuals);

        let config = load_config();
        let available_ports = list_serial_ports();
        let ip_host = config.ip_host.clone();
        let ip_port = config.ip_port;

        let connection_target = if let Some(t) = &config.connection_target {
            t.clone()
        } else {
            let port = auto_detect_camera_port().unwrap_or_else(|| {
                available_ports.first().map(|p| p.port_name.clone()).unwrap_or_default()
            });
            ConnectionTarget::Serial(port)
        };

        let use_ip_mode = matches!(connection_target, ConnectionTarget::Ip { .. });
        let selected_port = match &connection_target {
            ConnectionTarget::Serial(p) => p.clone(),
            ConnectionTarget::Ip { .. } => available_ports.first().map(|p| p.port_name.clone()).unwrap_or_default(),
        };

        let camera = Some(CameraManager::new(connection_target.clone()));

        let overlay_server = OverlayServer::start(8080);
        let (ip_scan_tx, ip_scan_rx) = mpsc::channel();
        let hymn_catalog = HymnCatalog::load_embedded();
        let (hymn_tx, hymn_rx) = mpsc::channel();

        Self {
            config,
            camera,
            available_ports,
            selected_port,
            connection_target,
            ip_host,
            ip_port,
            use_ip_mode,
            active_tab: AppTab::Camera,

            active_pan_state: (PanDir::Stop, TiltDir::Stop),
            active_zoom_state: ZoomDir::Stop,
            editing_preset: None,
            edit_buf: String::new(),
            action_text: "Ready".into(),
            action_expiry: None,
            last_ports_refresh: Instant::now(),
            marked_flash: None,

            overlay_server,
            hymn_catalog,
            hymn_search: String::new(),
            loaded_hymn: None,
            current_verse_idx: 0,
            hymn_live: false,
            hymn_loading: false,
            hymn_rx,
            hymn_tx,

            speaker_name: "Speaker Name".into(),
            sermon_title: "Speaker".into(),
            lt_live: false,
            auto_lt_on_preset1: true,
            lt_auto_hide_at: None,

            ticker_text: "Welcome to our worship service • Sacrament Meeting • Youth Activities Wednesday at 7:00 PM".into(),
            ticker_live: false,
            discovered_ip_cameras: Vec::new(),
            ip_scanning: false,
            ip_scan_rx,
            ip_scan_tx,
        }
    }

    fn set_action(&mut self, text: &str, duration_secs: u64) {
        self.action_text = text.to_string();
        self.action_expiry = Some(Instant::now() + Duration::from_secs(duration_secs));
    }

    fn change_target(&mut self, new_target: ConnectionTarget) {
        self.connection_target = new_target.clone();
        self.config.connection_target = Some(new_target.clone());
        save_config(&self.config);
        let label = new_target.display_label();
        self.camera = Some(CameraManager::new(new_target));
        self.set_action(&format!("Connecting to {}...", label), 2);
    }

    fn change_port(&mut self, new_port: &str) {
        self.selected_port = new_port.to_string();
        self.change_target(ConnectionTarget::Serial(new_port.to_string()));
    }

    fn connect_ip(&mut self) {
        self.config.ip_host = self.ip_host.clone();
        self.config.ip_port = self.ip_port;
        save_config(&self.config);
        self.change_target(ConnectionTarget::Ip {
            host: self.ip_host.clone(),
            port: self.ip_port,
        });
    }

    fn auto_detect(&mut self) {
        self.set_action("Scanning serial ports for camera...", 2);
        if let Some(port) = auto_detect_camera_port() {
            self.change_port(&port);
            self.set_action(&format!("✓ Camera connected on {}", port), 3);
        } else {
            self.set_action("No camera responded (select port manually)", 3);
        }
    }

    fn refresh_ports(&mut self) {
        self.available_ports = list_serial_ports();
        if !self.available_ports.iter().any(|p| p.port_name == self.selected_port) {
            if let Some(first) = self.available_ports.first() {
                self.change_port(&first.port_name.clone());
            }
        }
    }

    fn send_pan_tilt(&mut self, pan: PanDir, tilt: TiltDir) {
        if self.active_pan_state != (pan, tilt) {
            self.active_pan_state = (pan, tilt);
            if let Some(cam) = &self.camera {
                if pan == PanDir::Stop && tilt == TiltDir::Stop {
                    cam.send(ViscaCmd::PanTiltStop);
                } else {
                    cam.send(ViscaCmd::PanTilt {
                        pan,
                        tilt,
                        pan_spd: self.config.pan_speed,
                        tilt_spd: self.config.tilt_speed,
                    });
                }
            }

            let desc = match (pan, tilt) {
                (PanDir::Left, TiltDir::Up) => "Moving Up-Left...",
                (PanDir::Right, TiltDir::Up) => "Moving Up-Right...",
                (PanDir::Left, TiltDir::Down) => "Moving Down-Left...",
                (PanDir::Right, TiltDir::Down) => "Moving Down-Right...",
                (_, TiltDir::Up) => "Moving Up...",
                (_, TiltDir::Down) => "Moving Down...",
                (PanDir::Left, _) => "Moving Left...",
                (PanDir::Right, _) => "Moving Right...",
                (PanDir::Stop, TiltDir::Stop) => "Stopped",
            };
            self.set_action(desc, 2);
        }
    }

    fn send_zoom(&mut self, dir: ZoomDir) {
        if self.active_zoom_state != dir {
            self.active_zoom_state = dir;
            if let Some(cam) = &self.camera {
                match dir {
                    ZoomDir::Tele => {
                        cam.send(ViscaCmd::Zoom {
                            dir: ZoomDir::Tele,
                            speed: self.config.zoom_speed,
                        });
                        self.set_action("Zooming In...", 2);
                    }
                    ZoomDir::Wide => {
                        cam.send(ViscaCmd::Zoom {
                            dir: ZoomDir::Wide,
                            speed: self.config.zoom_speed,
                        });
                        self.set_action("Zooming Out...", 2);
                    }
                    ZoomDir::Stop => {
                        cam.send(ViscaCmd::ZoomStop);
                        self.set_action("Stopped", 1);
                    }
                }
            }
        }
    }

    fn emergency_stop(&mut self) {
        self.send_pan_tilt(PanDir::Stop, TiltDir::Stop);
        self.send_zoom(ZoomDir::Stop);
        if let Some(cam) = &self.camera {
            cam.send(ViscaCmd::PanTiltStop);
            cam.send(ViscaCmd::ZoomStop);
        }
        self.set_action("Emergency Stop", 2);
    }

    fn sync_overlay_state(&self) {
        let mut st = self.overlay_server.state.write().unwrap();
        st.lower_third = LowerThirdData {
            title: self.speaker_name.clone(),
            subtitle: self.sermon_title.clone(),
            visible: self.lt_live,
        };

        if let Some(h) = &self.loaded_hymn {
            let section = h.sections.get(self.current_verse_idx).cloned().unwrap_or_default();
            let section_title = if section.starts_with("[Chorus]") {
                format!("Hymn #{} • Chorus", h.num)
            } else {
                format!("Hymn #{} • Verse {}", h.num, self.current_verse_idx + 1)
            };
            st.hymn = HymnOverlayData {
                hymn_num: h.num,
                title: h.title.clone(),
                section_title,
                text: section,
                visible: self.hymn_live,
            };
        } else {
            st.hymn.visible = false;
        }

        st.ticker = TickerData {
            text: self.ticker_text.clone(),
            visible: self.ticker_live,
        };
    }

    fn load_hymn_async(&mut self, hymn: HymnEntry) {
        self.hymn_loading = true;
        self.set_action(&format!("Loading Hymn #{} ({}) lyrics...", hymn.num, hymn.title), 3);
        let tx = self.hymn_tx.clone();
        thread::spawn(move || {
            let res = get_hymn_lyrics(&hymn);
            let _ = tx.send(res);
        });
    }

    fn trigger_preset(&mut self, num: u8) {
        if let Some(cam) = &self.camera {
            cam.send(ViscaCmd::PresetRecall(num));
            let name = self.config.presets.get(&num).cloned().unwrap_or_default();
            self.set_action(&format!("Going to #{} ({})...", num, name), 3);

            // Smart Director: If Preset 1 is Podium, auto-trigger Lower Third for 8s
            if num == 1 && self.auto_lt_on_preset1 {
                self.lt_live = true;
                self.lt_auto_hide_at = Some(Instant::now() + Duration::from_secs(8));
                self.sync_overlay_state();
            }
        }
    }

    fn scan_ip_network(&mut self) {
        self.ip_scanning = true;
        self.set_action("Scanning LAN for VISCA & ONVIF cameras...", 3);
        let tx = self.ip_scan_tx.clone();
        thread::spawn(move || {
            let found = discover_ip_cameras();
            let _ = tx.send(found);
        });
    }
}

impl eframe::App for PTZApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Poll async hymn loader
        if let Ok(res) = self.hymn_rx.try_recv() {
            self.hymn_loading = false;
            match res {
                Ok(lyrics) => {
                    self.set_action(&format!("✓ Hymn #{} Loaded ({} verses)", lyrics.num, lyrics.sections.len()), 3);
                    self.loaded_hymn = Some(lyrics);
                    self.current_verse_idx = 0;
                    self.sync_overlay_state();
                }
                Err(err) => {
                    self.set_action(&format!("Error loading hymn: {}", err), 3);
                }
            }
        }
        // Poll async IP camera discovery
        if let Ok(cams) = self.ip_scan_rx.try_recv() {
            self.ip_scanning = false;
            let count = cams.len();
            if let Some(first) = cams.first() {
                self.ip_host = first.ip.clone();
                self.ip_port = first.port;
                self.connect_ip();
                self.set_action(&format!("✓ Found {} camera(s) — Connected to {}", count, first.ip), 4);
            } else {
                self.set_action("No IP cameras responded to LAN broadcast", 3);
            }
            self.discovered_ip_cameras = cams;
        }


        // Auto-hide lower third timer
        if let Some(hide_at) = self.lt_auto_hide_at {
            if Instant::now() > hide_at {
                self.lt_live = false;
                self.lt_auto_hide_at = None;
                self.sync_overlay_state();
            }
        }

        // Periodic ports refresh
        if self.last_ports_refresh.elapsed() > Duration::from_secs(3) {
            self.last_ports_refresh = Instant::now();
            let new_list = list_serial_ports();
            if new_list != self.available_ports {
                self.available_ports = new_list;
            }
        }

        // Action text expiry
        if let Some(exp) = self.action_expiry {
            if Instant::now() > exp && self.active_pan_state == (PanDir::Stop, TiltDir::Stop) && self.active_zoom_state == ZoomDir::Stop {
                self.action_text = "Ready".into();
                self.action_expiry = None;
            }
        }

        // Clear marked flash
        if let Some((_, time)) = self.marked_flash {
            if time.elapsed() > Duration::from_millis(1500) {
                self.marked_flash = None;
            }
        }

        // ---------------------------------------------------------------------
        // Global Keyboard Shortcuts
        // ---------------------------------------------------------------------
        let is_editing = self.editing_preset.is_some();
        if !is_editing {
            ctx.input(|i| {
                // Emergency stop
                if i.key_pressed(Key::Space) {
                    self.emergency_stop();
                    return;
                }

                // Home
                if i.key_pressed(Key::H) || i.key_pressed(Key::Home) {
                    if let Some(cam) = &self.camera {
                        cam.send(ViscaCmd::Home);
                        self.set_action("Centering (Home)...", 2);
                    }
                }

                // Power & Unmute
                if i.key_pressed(Key::P) {
                    if let Some(cam) = &self.camera {
                        let on = cam.toggle_power();
                        self.set_action(if on { "Power: ON" } else { "Power: STANDBY" }, 2);
                    }
                }
                if i.key_pressed(Key::M) {
                    if let Some(cam) = &self.camera {
                        cam.unmute();
                        self.set_action("Video Unmuted (AE Auto)", 2);
                    }
                }

                // Presets (1-6)
                let num_keys = [
                    (Key::Num1, 1), (Key::Num2, 2), (Key::Num3, 3),
                    (Key::Num4, 4), (Key::Num5, 5), (Key::Num6, 6),
                ];
                for (key, num) in num_keys {
                    if i.key_pressed(key) {
                        let name = self.config.presets.get(&num).cloned().unwrap_or_else(|| format!("Preset {}", num));
                        if i.modifiers.shift || i.modifiers.ctrl {
                            if let Some(cam) = &self.camera {
                                cam.send(ViscaCmd::PresetSet(num));
                                self.marked_flash = Some((num, Instant::now()));
                                self.set_action(&format!("✓ Saved Position as #{} ({})", num, name), 2);
                            }
                        } else {
                            self.trigger_preset(num);
                        }
                    }
                }

                // Pan & Tilt (Arrows)
                let has_up = i.key_down(Key::ArrowUp);
                let has_down = i.key_down(Key::ArrowDown);
                let has_left = i.key_down(Key::ArrowLeft);
                let has_right = i.key_down(Key::ArrowRight);

                let pan = if has_left && !has_right {
                    PanDir::Left
                } else if has_right && !has_left {
                    PanDir::Right
                } else {
                    PanDir::Stop
                };

                let tilt = if has_up && !has_down {
                    TiltDir::Up
                } else if has_down && !has_up {
                    TiltDir::Down
                } else {
                    TiltDir::Stop
                };

                self.send_pan_tilt(pan, tilt);

                // Zoom (Page Up / Page Down)
                let has_pgup = i.key_down(Key::PageUp);
                let has_pgdn = i.key_down(Key::PageDown);

                let zoom = if has_pgup && !has_pgdn {
                    ZoomDir::Tele
                } else if has_pgdn && !has_pgup {
                    ZoomDir::Wide
                } else {
                    ZoomDir::Stop
                };

                self.send_zoom(zoom);
            });
        }

        // ---------------------------------------------------------------------
        // GUI Dashboard Layout (No Scrollbars - Fits Screen Cleanly)
        // ---------------------------------------------------------------------
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);

            // 1. Row 1: Header Branding & Toggles
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("VISCACASTER").strong().size(18.0).color(Color32::from_rgb(56, 189, 248)));
                    ui.label(RichText::new("UNIVERSAL VISCA PTZ & BROADCAST DIRECTOR").size(10.0).color(Color32::from_rgb(148, 163, 184)));
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Power toggle
                    let pwr_on = self.camera.as_ref().map(|c| c.is_power_on()).unwrap_or(false);
                    let (pwr_text, pwr_color) = if pwr_on {
                        ("POWER ON", Color32::from_rgb(34, 197, 94))
                    } else {
                        ("STANDBY", Color32::from_rgb(239, 68, 68))
                    };
                    if ui.button(RichText::new(pwr_text).size(12.0).strong().color(pwr_color)).on_hover_text("Toggle Standby / Power (P)").clicked() {
                        if let Some(cam) = &self.camera {
                            let on = cam.toggle_power();
                            self.set_action(if on { "Power: ON" } else { "Power: STANDBY" }, 2);
                        }
                    }

                    // Video Unmute
                    if ui.button(RichText::new("UNMUTE VIDEO").size(12.0).strong()).on_hover_text("Unmute Video & Reset Auto-Exposure (M)").clicked() {
                        if let Some(cam) = &self.camera {
                            cam.unmute();
                            self.set_action("Video Unmuted (AE Full Auto)", 2);
                        }
                    }
                });
            });

            // 2. Row 2: Connection Bar & Toolbar Card
            card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let connected = self.camera.as_ref().map(|c| c.is_connected()).unwrap_or(false);

                    let (dot_rect, _) = ui.allocate_exact_size(Vec2::new(12.0, 12.0), Sense::hover());
                    let dot_color = if connected {
                        Color32::from_rgb(34, 197, 94)
                    } else {
                        Color32::from_rgb(239, 68, 68)
                    };
                    ui.painter().circle_filled(dot_rect.center(), 4.5_f32, dot_color);

                    let target_desc = self.connection_target.display_label();
                    if connected {
                        ui.label(RichText::new(format!("ONLINE: {}", target_desc)).strong().size(12.0).color(Color32::from_rgb(226, 232, 240)));
                    } else {
                        ui.label(RichText::new(format!("OFFLINE ({})", target_desc)).strong().size(12.0).color(Color32::from_rgb(248, 113, 113)));
                    }

                    // Mode switch toggle: Serial vs IP
                    let serial_btn = egui::Button::new(RichText::new("Serial (USB)").size(11.0))
                        .fill(if !self.use_ip_mode { Color32::from_rgb(37, 99, 235) } else { Color32::from_rgb(34, 34, 46) });
                    if ui.add(serial_btn).clicked() && self.use_ip_mode {
                        self.use_ip_mode = false;
                        let p = self.selected_port.clone();
                        self.change_port(&p);
                    }

                    let ip_btn = egui::Button::new(RichText::new("Network (IP)").size(11.0))
                        .fill(if self.use_ip_mode { Color32::from_rgb(37, 99, 235) } else { Color32::from_rgb(34, 34, 46) });
                    if ui.add(ip_btn).clicked() && !self.use_ip_mode {
                        self.use_ip_mode = true;
                        self.connect_ip();
                    }

                    // Right controls
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if !self.use_ip_mode {
                            if ui.button(RichText::new("Auto-Detect").size(12.0)).on_hover_text("Scan serial ports to locate camera").clicked() {
                                self.auto_detect();
                            }
                            if ui.button(RichText::new("Refresh").size(12.0)).on_hover_text("Refresh Serial Port List").clicked() {
                                self.refresh_ports();
                            }

                            let mut port_to_change = None;
                            let selected_label = self
                                .available_ports
                                .iter()
                                .find(|p| p.port_name == self.selected_port)
                                .map(|p| p.display_label.clone())
                                .unwrap_or_else(|| self.selected_port.clone());

                            egui::ComboBox::from_id_salt("port_select")
                                .selected_text(RichText::new(&selected_label).size(12.0))
                                .width(150.0)
                                .show_ui(ui, |ui| {
                                    for port in &self.available_ports {
                                        let is_selected = self.selected_port == port.port_name;
                                        if ui.selectable_label(is_selected, &port.display_label).clicked() {
                                            port_to_change = Some(port.port_name.clone());
                                        }
                                    }
                                });
                            if let Some(p) = port_to_change {
                                self.change_port(&p);
                            }
                        } else {
                            if ui.button(RichText::new("Connect IP").size(12.0).strong()).clicked() {
                                self.connect_ip();
                            }

                            let scan_text = if self.ip_scanning { "⏳ Scanning..." } else { "🔍 Scan LAN" };
                            if ui.button(RichText::new(scan_text).size(12.0)).on_hover_text("Scan local network for VISCA and ONVIF cameras").clicked() && !self.ip_scanning {
                                self.scan_ip_network();
                            }

                            if !self.discovered_ip_cameras.is_empty() {
                                let mut selected_ip = None;
                                egui::ComboBox::from_id_salt("ip_cams_combo")
                                    .selected_text(format!("{} Found ▼", self.discovered_ip_cameras.len()))
                                    .width(130.0)
                                    .show_ui(ui, |ui| {
                                        for cam in &self.discovered_ip_cameras {
                                            if ui.selectable_label(self.ip_host == cam.ip, &cam.display_name).clicked() {
                                                selected_ip = Some((cam.ip.clone(), cam.port));
                                            }
                                        }
                                    });
                                if let Some((ip, port)) = selected_ip {
                                    self.ip_host = ip;
                                    self.ip_port = port;
                                    self.connect_ip();
                                }
                            }

                            let mut port_str = self.ip_port.to_string();
                            if ui.add_sized(Vec2::new(50.0, 24.0), egui::TextEdit::singleline(&mut port_str).hint_text("52381")).lost_focus() {
                                if let Ok(p) = port_str.parse::<u16>() {
                                    self.ip_port = p;
                                }
                            }
                            ui.label("Port:");
                            ui.add_sized(Vec2::new(110.0, 24.0), egui::TextEdit::singleline(&mut self.ip_host).hint_text("192.168.1.100"));
                            ui.label("IP:");
                        }
                    });
                });
            });

            // 3. Tab Bar (Clean 25% equal width, zero emoji tofu boxes, zero collision!)
            ui.horizontal(|ui| {
                let avail_w = ui.available_width();
                let tab_w = ((avail_w - 24.0) / 4.0).max(80.0);

                let tab_btn = |ui: &mut egui::Ui, name: &str, active: bool| -> bool {
                    let mut btn = egui::Button::new(RichText::new(name).size(12.0).strong());
                    if active {
                        btn = btn.fill(Color32::from_rgb(37, 99, 235)).stroke(Stroke::new(1.0_f32, Color32::from_rgb(96, 165, 250)));
                    } else {
                        btn = btn.fill(Color32::from_rgb(28, 28, 38)).stroke(Stroke::new(1.0_f32, Color32::from_rgb(42, 42, 54)));
                    }
                    ui.add_sized(Vec2::new(tab_w, 32.0), btn).clicked()
                };

                if tab_btn(ui, "CAM PTZ", self.active_tab == AppTab::Camera) {
                    self.active_tab = AppTab::Camera;
                }
                if tab_btn(ui, "HYMNS & LYRICS", self.active_tab == AppTab::Hymns) {
                    self.active_tab = AppTab::Hymns;
                }
                if tab_btn(ui, "LOWER THIRDS", self.active_tab == AppTab::Overlays) {
                    self.active_tab = AppTab::Overlays;
                }
                if tab_btn(ui, "OBS SETUP", self.active_tab == AppTab::ObsSetup) {
                    self.active_tab = AppTab::ObsSetup;
                }
            });
            // 4. Tab Body Content
            match self.active_tab {
                AppTab::Camera => self.render_camera_tab(ui),
                AppTab::Hymns => self.render_hymns_tab(ui),
                AppTab::Overlays => self.render_overlays_tab(ui),
                AppTab::ObsSetup => self.render_obs_setup_tab(ui),
            }

            ui.separator();

            // Footer Shortcuts Info
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("Arrow Keys Move  •  PgUp / PgDn Zoom  •  1-6 Go To  •  Shift+1-6 Mark  •  H Center  •  Space Stop").size(11.0).color(Color32::from_rgb(100, 116, 139)));
            });
        });

        // Request continuous repaint if moving or timer active
        if self.active_pan_state != (PanDir::Stop, TiltDir::Stop) || self.active_zoom_state != ZoomDir::Stop || self.marked_flash.is_some() || self.lt_auto_hide_at.is_some() {
            ctx.request_repaint();
        }
    }
}

// -----------------------------------------------------------------------------
// Sub-view Tab Renderers
// -----------------------------------------------------------------------------
impl PTZApp {
    fn render_camera_tab(&mut self, ui: &mut egui::Ui) {
        // Directional D-Pad Section (Vector Rendered)
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            let btn_size = Vec2::new(82.0, 56.0);
            let grid_width = btn_size.x * 3.0 + 16.0;
            let left_pad = ((ui.available_width() - grid_width) / 2.0).max(0.0);

            // Row 1: UpLeft, Up, UpRight
            ui.horizontal(|ui| {
                ui.add_space(left_pad);
                if dpad_vector_button(ui, DPadDir::UpLeft, btn_size, self.active_pan_state == (PanDir::Left, TiltDir::Up)) {
                    self.send_pan_tilt(PanDir::Left, TiltDir::Up);
                }
                if dpad_vector_button(ui, DPadDir::Up, btn_size, self.active_pan_state == (PanDir::Stop, TiltDir::Up)) {
                    self.send_pan_tilt(PanDir::Stop, TiltDir::Up);
                }
                if dpad_vector_button(ui, DPadDir::UpRight, btn_size, self.active_pan_state == (PanDir::Right, TiltDir::Up)) {
                    self.send_pan_tilt(PanDir::Right, TiltDir::Up);
                }
            });

            // Row 2: Left, Center Home, Right
            ui.horizontal(|ui| {
                ui.add_space(left_pad);
                if dpad_vector_button(ui, DPadDir::Left, btn_size, self.active_pan_state == (PanDir::Left, TiltDir::Stop)) {
                    self.send_pan_tilt(PanDir::Left, TiltDir::Stop);
                }
                if dpad_vector_button(ui, DPadDir::Home, btn_size, false) {
                    if let Some(cam) = &self.camera {
                        cam.send(ViscaCmd::Home);
                        self.set_action("Centering (Home)...", 2);
                    }
                }
                if dpad_vector_button(ui, DPadDir::Right, btn_size, self.active_pan_state == (PanDir::Right, TiltDir::Stop)) {
                    self.send_pan_tilt(PanDir::Right, TiltDir::Stop);
                }
            });

            // Row 3: DownLeft, Down, DownRight
            ui.horizontal(|ui| {
                ui.add_space(left_pad);
                if dpad_vector_button(ui, DPadDir::DownLeft, btn_size, self.active_pan_state == (PanDir::Left, TiltDir::Down)) {
                    self.send_pan_tilt(PanDir::Left, TiltDir::Down);
                }
                if dpad_vector_button(ui, DPadDir::Down, btn_size, self.active_pan_state == (PanDir::Stop, TiltDir::Down)) {
                    self.send_pan_tilt(PanDir::Stop, TiltDir::Down);
                }
                if dpad_vector_button(ui, DPadDir::DownRight, btn_size, self.active_pan_state == (PanDir::Right, TiltDir::Down)) {
                    self.send_pan_tilt(PanDir::Right, TiltDir::Down);
                }
            });
        });

        // Zoom Controls Section
        card_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                let zm_width = (ui.available_width() - 8.0) / 2.0;
                let zm_btn_size = Vec2::new(zm_width, 42.0);

                let zout_held = zoom_button(ui, "-  ZOOM OUT [PgDn]", zm_btn_size, self.active_zoom_state == ZoomDir::Wide);
                let zin_held = zoom_button(ui, "+  ZOOM IN [PgUp]", zm_btn_size, self.active_zoom_state == ZoomDir::Tele);

                if zout_held {
                    self.send_zoom(ZoomDir::Wide);
                } else if zin_held {
                    self.send_zoom(ZoomDir::Tele);
                }
            });
        });

        // Hardware Memory Presets 1-6
        card_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("PRESETS").strong().size(13.0).color(Color32::from_rgb(226, 232, 240)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("Keys: 1-6 Go To  •  Shift+1-6 Mark").size(11.0).color(Color32::from_rgb(148, 163, 184)));
                });
            });

            ui.separator();

            for i in 1..=6 {
                ui.horizontal(|ui| {
                    // Preset badge
                    let badge = egui::Button::new(RichText::new(format!("#{}", i)).strong().size(12.0))
                        .fill(Color32::from_rgb(45, 45, 60))
                        .stroke(Stroke::NONE);
                    ui.add_sized(Vec2::new(30.0, 26.0), badge);

                    let is_this_editing = self.editing_preset == Some(i);
                    if is_this_editing {
                        let resp = ui.text_edit_singleline(&mut self.edit_buf);
                        if resp.lost_focus() && ui.input(|inp| inp.key_pressed(Key::Enter)) {
                            let new_name = if self.edit_buf.trim().is_empty() {
                                format!("Preset {}", i)
                            } else {
                                self.edit_buf.trim().to_string()
                            };
                            self.config.presets.insert(i, new_name);
                            save_config(&self.config);
                            self.editing_preset = None;
                        }

                        if ui.button("SAVE").on_hover_text("Save Name").clicked() {
                            let new_name = if self.edit_buf.trim().is_empty() {
                                format!("Preset {}", i)
                            } else {
                                self.edit_buf.trim().to_string()
                            };
                            self.config.presets.insert(i, new_name);
                            save_config(&self.config);
                            self.editing_preset = None;
                        }

                        if ui.button("CANCEL").on_hover_text("Cancel").clicked() {
                            self.editing_preset = None;
                        }
                    } else {
                        let name = self.config.presets.get(&i).cloned().unwrap_or_else(|| format!("Preset {}", i));
                        ui.label(RichText::new(&name).strong().size(13.0).color(Color32::from_rgb(241, 245, 249)));

                        if ui.small_button("edit").on_hover_text("Rename Preset").clicked() {
                            self.editing_preset = Some(i);
                            self.edit_buf = name;
                        }
                    }

                    // Right-aligned action buttons
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let is_flashing = self.marked_flash.map(|(n, _)| n == i).unwrap_or(false);
                        let (mark_lbl, mark_color) = if is_flashing {
                            ("✓ SAVED!", Color32::from_rgb(34, 197, 94))
                        } else {
                            ("MARK", Color32::from_rgb(245, 158, 11))
                        };
                        let mark_btn = egui::Button::new(RichText::new(mark_lbl).size(12.0).strong().color(mark_color))
                            .stroke(Stroke::new(1.0_f32, mark_color))
                            .fill(Color32::from_rgb(30, 30, 40));
                        if ui.add_sized(Vec2::new(70.0, 26.0), mark_btn).on_hover_text(format!("Save position to Preset {} (Shift+{})", i, i)).clicked() {
                            if let Some(cam) = &self.camera {
                                cam.send(ViscaCmd::PresetSet(i));
                                self.marked_flash = Some((i, Instant::now()));
                                let name = self.config.presets.get(&i).cloned().unwrap_or_default();
                                self.set_action(&format!("✓ Saved Position to #{} ({})", i, name), 2);
                            }
                        }

                        // Go To Button (Primary Blue Pill)
                        let goto_btn = egui::Button::new(RichText::new("GO TO").size(12.0).strong().color(Color32::WHITE))
                            .fill(Color32::from_rgb(37, 99, 235))
                            .stroke(Stroke::NONE);
                        if ui.add_sized(Vec2::new(70.0, 26.0), goto_btn).on_hover_text(format!("Move to Preset {} (Key {})", i, i)).clicked() {
                            self.trigger_preset(i);
                        }
                    });
                });
            }
        });

        // Speed Sliders
        card_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Pan/Tilt Speed:").size(12.0));
                let mut spd = self.config.pan_speed;
                if ui.add(egui::Slider::new(&mut spd, 1..=24).show_value(true)).changed() {
                    self.config.pan_speed = spd;
                    self.config.tilt_speed = spd.min(20);
                    save_config(&self.config);
                }

                ui.add_space(16.0);

                ui.label(RichText::new("Zoom Speed:").size(12.0));
                let mut spd = self.config.zoom_speed;
                if ui.add(egui::Slider::new(&mut spd, 0..=7).show_value(true)).changed() {
                    self.config.zoom_speed = spd;
                    save_config(&self.config);
                }
            });
        });
    }

    fn render_hymns_tab(&mut self, ui: &mut egui::Ui) {
        // 1. Search & Quick-Load Card
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());

            ui.horizontal(|ui| {
                ui.label(RichText::new("Find Hymn:").strong().size(13.0));
                let resp = ui.add_sized(
                    Vec2::new(260.0, 28.0),
                    egui::TextEdit::singleline(&mut self.hymn_search).hint_text("Type number (e.g. 193) or title keyword..."),
                );

                let matches: Vec<HymnEntry> = self.hymn_catalog.search(&self.hymn_search).into_iter().cloned().collect();

                // Enter key loads first match immediately
                if resp.lost_focus() && ui.input(|inp| inp.key_pressed(Key::Enter)) {
                    if let Some(first) = matches.first() {
                        self.load_hymn_async(first.clone());
                    }
                }

                if ui.button(RichText::new("Clear").size(12.0)).clicked() {
                    self.hymn_search.clear();
                }
            });

            // Matched hymn preview & Load button
            let matches: Vec<HymnEntry> = self.hymn_catalog.search(&self.hymn_search).into_iter().cloned().collect();
            if let Some(first) = matches.first() {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("#{}: {}", first.num, first.title)).color(Color32::from_rgb(56, 189, 248)).strong().size(14.0));

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let load_btn = egui::Button::new(RichText::new("LOAD HYMN").strong().size(12.0).color(Color32::WHITE))
                            .fill(Color32::from_rgb(37, 99, 235));
                        if ui.add_sized(Vec2::new(110.0, 28.0), load_btn).on_hover_text("Download and load lyrics for broadcast").clicked() {
                            self.load_hymn_async(first.clone());
                        }
                    });
                });
            }

            if self.hymn_loading {
                ui.add_space(4.0);
                ui.label(RichText::new("⏳ Downloading hymn lyrics from Church website...").color(Color32::from_rgb(245, 158, 11)).strong());
            }
        });

        // 2. Sunday Meeting Shortcuts Card
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Sunday Shortcuts:").strong().size(12.0).color(Color32::from_rgb(148, 163, 184)));
                let quick_hymns = [
                    (2, "The Spirit of God"),
                    (6, "Redeemer of Israel"),
                    (193, "I Stand All Amazed"),
                    (85, "How Firm a Foundation"),
                    (300, "Families Can Be Together"),
                ];
                for (num, title) in quick_hymns {
                    let btn = egui::Button::new(RichText::new(format!("#{} {}", num, title)).size(11.0))
                        .fill(Color32::from_rgb(34, 34, 46));
                    if ui.add(btn).clicked() {
                        if let Some(h) = self.hymn_catalog.find_by_number(num) {
                            self.hymn_search = num.to_string();
                            self.load_hymn_async(h.clone());
                        }
                    }
                }
            });
        });

        // Search candidate list
        let matches: Vec<HymnEntry> = self.hymn_catalog.search(&self.hymn_search).into_iter().cloned().collect();
        if matches.len() > 1 {
            card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.strong("More Search Matches:");
                ui.horizontal_wrapped(|ui| {
                    for h in matches.into_iter().skip(1).take(8) {
                        let btn_text = format!("#{}: {}", h.num, h.title);
                        if ui.button(&btn_text).clicked() {
                            self.load_hymn_async(h);
                        }
                    }
                });
            });
        }

        // 3. Active Loaded Hymn Presentation Deck
        if let Some(hymn) = self.loaded_hymn.clone() {
            card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("Hymn #{}: {}", hymn.num, hymn.title)).strong().size(16.0).color(Color32::from_rgb(56, 189, 248)));

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Live Toggle Button
                        let (btn_text, btn_color) = if self.hymn_live {
                            ("🔴 ON STREAM (LIVE)", Color32::from_rgb(239, 68, 68))
                        } else {
                            ("SHOW ON STREAM", Color32::from_rgb(34, 197, 94))
                        };
                        let btn = egui::Button::new(RichText::new(btn_text).strong().size(13.0).color(Color32::WHITE)).fill(btn_color);
                        if ui.add(btn).clicked() {
                            self.hymn_live = !self.hymn_live;
                            self.sync_overlay_state();
                        }
                    });
                });

                ui.separator();

                // Live Slide Preview
                let cur_section = hymn.sections.get(self.current_verse_idx).cloned().unwrap_or_default();
                let section_header = if cur_section.starts_with("[Chorus]") {
                    "Chorus".to_string()
                } else {
                    format!("Verse {} of {}", self.current_verse_idx + 1, hymn.sections.len())
                };

                ui.vertical_centered(|ui| {
                    ui.label(RichText::new(&section_header).strong().size(14.0).color(Color32::from_rgb(148, 163, 184)));
                    ui.add_space(4.0);

                    // Box showing live preview
                    let preview_frame = egui::Frame::new()
                        .fill(Color32::from_rgb(15, 23, 42))
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(56, 189, 248)))
                        .corner_radius(CornerRadius::same(8))
                        .inner_margin(egui::Margin::symmetric(24, 16));

                    preview_frame.show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(&cur_section).size(18.0).strong().color(Color32::WHITE));
                    });

                    ui.add_space(8.0);

                    // Navigation Buttons
                    ui.horizontal(|ui| {
                        ui.add_space((ui.available_width() - 320.0) / 2.0);

                        let prev_enabled = self.current_verse_idx > 0;
                        if ui.add_enabled(prev_enabled, egui::Button::new(RichText::new("◀ PREV VERSE").size(13.0).strong()))
                            .clicked()
                        {
                            self.current_verse_idx -= 1;
                            self.sync_overlay_state();
                        }

                        ui.label(RichText::new(format!("{}/{}", self.current_verse_idx + 1, hymn.sections.len())).strong().size(14.0));

                        let next_enabled = self.current_verse_idx + 1 < hymn.sections.len();
                        if ui.add_enabled(next_enabled, egui::Button::new(RichText::new("NEXT VERSE ▶").size(13.0).strong()))
                            .clicked()
                        {
                            self.current_verse_idx += 1;
                            self.sync_overlay_state();
                        }
                    });
                });

                ui.separator();

                // Quick Jump to any verse
                ui.horizontal_wrapped(|ui| {
                    ui.label("Jump to:");
                    for idx in 0..hymn.sections.len() {
                        let label = format!("V{}", idx + 1);
                        let is_active = self.current_verse_idx == idx;
                        let mut btn = egui::Button::new(RichText::new(label).strong());
                        if is_active {
                            btn = btn.fill(Color32::from_rgb(37, 99, 235));
                        }
                        if ui.add_sized(Vec2::new(38.0, 24.0), btn).clicked() {
                            self.current_verse_idx = idx;
                            self.sync_overlay_state();
                        }
                    }
                });
            });
        }
    }

    fn render_overlays_tab(&mut self, ui: &mut egui::Ui) {
        // 1. Speaker Lower Third Card
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("SPEAKER LOWER THIRD").strong().size(14.0).color(Color32::from_rgb(56, 189, 248)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (btn_text, btn_color) = if self.lt_live {
                        ("🔴 LOWER THIRD LIVE", Color32::from_rgb(239, 68, 68))
                    } else {
                        ("SHOW ON STREAM", Color32::from_rgb(34, 197, 94))
                    };
                    let btn = egui::Button::new(RichText::new(btn_text).strong().size(12.0).color(Color32::WHITE)).fill(btn_color);
                    if ui.add(btn).clicked() {
                        self.lt_live = !self.lt_live;
                        self.sync_overlay_state();
                    }
                });
            });

            ui.separator();

            ui.horizontal(|ui| {
                ui.label("Speaker Name:   ");
                if ui.add_sized(Vec2::new(ui.available_width() - 8.0, 26.0), egui::TextEdit::singleline(&mut self.speaker_name)).changed() && self.lt_live {
                    self.sync_overlay_state();
                }
            });

            ui.horizontal(|ui| {
                ui.label("Sermon / Title: ");
                if ui.add_sized(Vec2::new(ui.available_width() - 8.0, 26.0), egui::TextEdit::singleline(&mut self.sermon_title)).changed() && self.lt_live {
                    self.sync_overlay_state();
                }
            });

            // Quick role buttons
            ui.horizontal(|ui| {
                ui.label(RichText::new("Quick Roles:").size(11.0).color(Color32::from_rgb(148, 163, 184)));
                for role in ["Bishop", "Conducting", "Speaker", "Youth Speaker", "Chorister", "Organist"] {
                    if ui.button(role).clicked() {
                        self.sermon_title = role.to_string();
                        if self.lt_live {
                            self.sync_overlay_state();
                        }
                    }
                }
            });

            ui.add_space(4.0);
            ui.checkbox(&mut self.auto_lt_on_preset1, "Smart Director: Auto-trigger lower third for 8s when Preset 1 (Podium) is recalled");
        });

        // 2. Announcement Marquee Ticker Card
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("ANNOUNCEMENT TICKER (BOTTOM SCROLL)").strong().size(14.0).color(Color32::from_rgb(56, 189, 248)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (btn_text, btn_color) = if self.ticker_live {
                        ("🔴 TICKER LIVE", Color32::from_rgb(239, 68, 68))
                    } else {
                        ("START TICKER", Color32::from_rgb(34, 197, 94))
                    };
                    let btn = egui::Button::new(RichText::new(btn_text).strong().size(12.0).color(Color32::WHITE)).fill(btn_color);
                    if ui.add(btn).clicked() {
                        self.ticker_live = !self.ticker_live;
                        self.sync_overlay_state();
                    }
                });
            });

            ui.separator();

            ui.horizontal(|ui| {
                ui.label("Ticker Text: ");
                if ui.add_sized(Vec2::new(ui.available_width() - 8.0, 26.0), egui::TextEdit::singleline(&mut self.ticker_text)).changed() && self.ticker_live {
                    self.sync_overlay_state();
                }
            });

            ui.horizontal(|ui| {
                ui.label(RichText::new("Templates:").size(11.0).color(Color32::from_rgb(148, 163, 184)));
                if ui.button("Welcome").clicked() {
                    self.ticker_text = "Welcome to Sunday Service • Nursery available in Room 102".into();
                    if self.ticker_live { self.sync_overlay_state(); }
                }
                if ui.button("Fellowship Dinner").clicked() {
                    self.ticker_text = "Join us for our monthly potluck dinner tonight at 6:00 PM in the cultural hall".into();
                    if self.ticker_live { self.sync_overlay_state(); }
                }
                if ui.button("Youth Activities").clicked() {
                    self.ticker_text = "Wednesday Night Youth Activities and Institute classes at 7:00 PM".into();
                    if self.ticker_live { self.sync_overlay_state(); }
                }
            });
        });
    }

    fn render_obs_setup_tab(&mut self, ui: &mut egui::Ui) {
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("OBS STUDIO INTEGRATION GUIDE").strong().size(16.0).color(Color32::from_rgb(56, 189, 248)));
            ui.separator();

            ui.label("The built-in graphics engine serves transparent broadcast overlays directly into OBS Studio:");

            ui.add_space(8.0);

            ui.horizontal(|ui| {
                ui.strong("Browser Source URL:");
                let url = format!("http://localhost:{}/overlay", self.overlay_server.port);
                ui.label(RichText::new(&url).strong().color(Color32::from_rgb(56, 189, 248)));
                if ui.button("📋 Copy URL").clicked() {
                    ui.ctx().copy_text(url.clone());
                    self.set_action("✓ Copied URL to clipboard!", 2);
                }
            });

            ui.add_space(8.0);

            ui.strong("Steps to Add in OBS Studio:");
            ui.label("1. Open OBS Studio.");
            ui.label("2. Under the 'Sources' box, click [+] and select 'Browser'.");
            ui.label(format!("3. In the URL field, paste: http://localhost:{}/overlay", self.overlay_server.port));
            ui.label("4. Set Width to 1920 and Height to 1080 (or your stream resolution).");
            ui.label("5. Check 'Shutdown source when not visible' = OFF and 'Refresh browser when scene becomes active' = ON.");
            ui.label("6. Place this Browser source above your camera video source.");

            ui.add_space(8.0);
            ui.label("Now, whenever you advance hymn lyrics or toggle a lower third in this app, it will animate onto your livestream automatically at 60 FPS!");
        });
    }
}

// -----------------------------------------------------------------------------
// Graphic Helper Functions
// -----------------------------------------------------------------------------
fn card_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(Color32::from_rgb(24, 24, 32))
        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(42, 42, 56)))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(12, 9))
}

#[derive(Clone, Copy)]
enum DPadDir {
    UpLeft,
    Up,
    UpRight,
    Left,
    Home,
    Right,
    DownLeft,
    Down,
    DownRight,
}

fn dpad_vector_button(ui: &mut egui::Ui, dir: DPadDir, size: Vec2, active: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let painter = ui.painter_at(rect);

    let bg_color = if active {
        Color32::from_rgb(37, 99, 235)
    } else if resp.hovered() {
        Color32::from_rgb(46, 46, 62)
    } else {
        Color32::from_rgb(32, 32, 42)
    };

    let stroke = if active {
        Stroke::new(2.0_f32, Color32::from_rgb(96, 165, 250))
    } else if resp.hovered() {
        Stroke::new(1.0_f32, Color32::from_rgb(70, 70, 90))
    } else {
        Stroke::new(1.0_f32, Color32::from_rgb(45, 45, 60))
    };

    painter.rect_filled(rect, CornerRadius::same(8), bg_color);
    painter.rect_stroke(rect, CornerRadius::same(8), stroke, egui::StrokeKind::Inside);

    let center = rect.center();
    let icon_color = if active {
        Color32::WHITE
    } else {
        Color32::from_rgb(226, 232, 240)
    };

    match dir {
        DPadDir::Up => {
            let pts = [
                center + egui::vec2(0.0, -11.0),
                center + egui::vec2(-10.0, 7.0),
                center + egui::vec2(10.0, 7.0),
            ];
            painter.add(egui::Shape::convex_polygon(pts.to_vec(), icon_color, Stroke::NONE));
        }
        DPadDir::Down => {
            let pts = [
                center + egui::vec2(0.0, 11.0),
                center + egui::vec2(-10.0, -7.0),
                center + egui::vec2(10.0, -7.0),
            ];
            painter.add(egui::Shape::convex_polygon(pts.to_vec(), icon_color, Stroke::NONE));
        }
        DPadDir::Left => {
            let pts = [
                center + egui::vec2(-11.0, 0.0),
                center + egui::vec2(7.0, -10.0),
                center + egui::vec2(7.0, 10.0),
            ];
            painter.add(egui::Shape::convex_polygon(pts.to_vec(), icon_color, Stroke::NONE));
        }
        DPadDir::Right => {
            let pts = [
                center + egui::vec2(11.0, 0.0),
                center + egui::vec2(-7.0, -10.0),
                center + egui::vec2(-7.0, 10.0),
            ];
            painter.add(egui::Shape::convex_polygon(pts.to_vec(), icon_color, Stroke::NONE));
        }
        DPadDir::UpLeft => {
            let s = Stroke::new(2.5_f32, icon_color);
            painter.line_segment([center + egui::vec2(-8.0, -8.0), center + egui::vec2(7.0, 7.0)], s);
            painter.line_segment([center + egui::vec2(-8.0, -8.0), center + egui::vec2(1.0, -8.0)], s);
            painter.line_segment([center + egui::vec2(-8.0, -8.0), center + egui::vec2(-8.0, 1.0)], s);
        }
        DPadDir::UpRight => {
            let s = Stroke::new(2.5_f32, icon_color);
            painter.line_segment([center + egui::vec2(8.0, -8.0), center + egui::vec2(-7.0, 7.0)], s);
            painter.line_segment([center + egui::vec2(8.0, -8.0), center + egui::vec2(-1.0, -8.0)], s);
            painter.line_segment([center + egui::vec2(8.0, -8.0), center + egui::vec2(8.0, 1.0)], s);
        }
        DPadDir::DownLeft => {
            let s = Stroke::new(2.5_f32, icon_color);
            painter.line_segment([center + egui::vec2(-8.0, 8.0), center + egui::vec2(7.0, -7.0)], s);
            painter.line_segment([center + egui::vec2(-8.0, 8.0), center + egui::vec2(1.0, 8.0)], s);
            painter.line_segment([center + egui::vec2(-8.0, 8.0), center + egui::vec2(-8.0, -1.0)], s);
        }
        DPadDir::DownRight => {
            let s = Stroke::new(2.5_f32, icon_color);
            painter.line_segment([center + egui::vec2(8.0, 8.0), center + egui::vec2(-7.0, -7.0)], s);
            painter.line_segment([center + egui::vec2(8.0, 8.0), center + egui::vec2(-1.0, 8.0)], s);
            painter.line_segment([center + egui::vec2(8.0, 8.0), center + egui::vec2(8.0, -1.0)], s);
        }
        DPadDir::Home => {
            let home_color = if active { Color32::WHITE } else { Color32::from_rgb(56, 189, 248) };
            let roof = [
                center + egui::vec2(0.0, -13.0),
                center + egui::vec2(-10.0, -3.0),
                center + egui::vec2(10.0, -3.0),
            ];
            painter.add(egui::Shape::convex_polygon(roof.to_vec(), home_color, Stroke::NONE));
            let body = egui::Rect::from_min_max(center + egui::vec2(-7.0, -3.0), center + egui::vec2(7.0, 5.0));
            painter.rect_filled(body, CornerRadius::ZERO, home_color);
            painter.text(
                center + egui::vec2(0.0, 14.0),
                egui::Align2::CENTER_CENTER,
                "HOME",
                egui::FontId::proportional(10.0),
                home_color,
            );
        }
    }

    if matches!(dir, DPadDir::Home) {
        resp.clicked()
    } else {
        resp.is_pointer_button_down_on()
    }
}

fn zoom_button(ui: &mut egui::Ui, text: &str, size: Vec2, active: bool) -> bool {
    let text_widget = RichText::new(text).size(13.0).strong().color(if active { Color32::WHITE } else { Color32::from_rgb(226, 232, 240) });
    let btn = egui::Button::new(text_widget)
        .fill(if active { Color32::from_rgb(37, 99, 235) } else { Color32::from_rgb(32, 32, 42) })
        .stroke(if active { Stroke::new(1.5_f32, Color32::from_rgb(96, 165, 250)) } else { Stroke::new(1.0_f32, Color32::from_rgb(45, 45, 60)) })
        .corner_radius(CornerRadius::same(8));

    let resp = ui.add_sized(size, btn);
    resp.is_pointer_button_down_on()
}
