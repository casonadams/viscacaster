# ViscaCaster

Universal VISCA PTZ Camera & Broadcast Presentation Director

A cross-platform desktop application written in Rust for controlling VISCA PTZ cameras (Serial RS-232/USB and Network IP/UDP), paired with an embedded broadcast graphics engine for live video productions (OBS Studio, vMix, etc.).

---

## Features

### Camera Control (Universal VISCA)
* **Dual Transport:** Supports both **Serial RS-232/USB** adapters (Keyspan, FTDI, CP210x, CH340, PL2303) and **VISCA-over-IP** (UDP port 52381 / 1259) across Sony, PTZOptics, BirdDog, Minrray, and generic VISCA PTZ cameras.
* **Directional Movement:** 8-way directional movement (including simultaneous diagonals) and Center Home.
* **Continuous Press-and-Hold:** Streams movement while held (keyboard or mouse/touch), halts on release.
* **Optical Zoom Control:** Variable speed Tele (Zoom In) and Wide (Zoom Out) controls.
* **Hardware Memory Presets (1-6):**
  * Instant recall (**Go To**) and live saving (**Mark**).
  * Custom preset descriptions saved across sessions.
* **Dual Speed Sliders:** Independent sliders for Pan/Tilt speed (1-24) and Optical Zoom speed (0-7).
* **Power and Video Management:** Standby/power toggle and one-click video un-mute with auto-exposure reset.

### Port and Network Auto-Discovery
* **3-Tier Serial Auto-Detection:**
  1. Actively probes serial ports with non-blocking VISCA handshakes.
  2. Inspects USB device metadata for known camera converters.
  3. Filters out legacy unpopulated motherboard UARTs.
* **Network Camera Discovery:**
  * Broadcasts VISCA-over-IP and ONVIF WS-Discovery probes to automatically locate network PTZ cameras on the local subnet.
* **Manual Override:** Select any serial port or manually input IP address and port at any time.

### Hymns and Lyric Presentation
* **Embedded Catalog (1-341+):**
  * Instant numerical lookup and title search.
  * Quick-pick buttons for common service selections.
* **Automatic Lyric Extraction:**
  * Pulls verses and choruses and caches them locally for offline use.
* **Slide Carousel:**
  * Advance lyrics verse-by-verse during singing (`Next Verse` / `Prev Verse` or jump directly to any verse/chorus).
  * One-click **Live on Stream** toggle.

### Broadcast Overlays (OBS Studio Integration)
* **Embedded Graphics Server (http://localhost:8080/overlay):**
  * Transparent 1080p/4K HTML5/CSS3 layer rendered at 60 FPS directly into OBS Studio as a Browser Source.
* **Speaker Lower Thirds:**
  * Customizable Speaker Name and Title/Topic with animated transitions.
  * **Director Automation:** Automatically triggers the speaker lower third for 8 seconds whenever **Preset 1 (Podium)** is recalled.
* **Announcement Marquee Ticker:**
  * Hardware-accelerated scrolling announcement ticker along the bottom of the stream.

---

## Keyboard Shortcuts

| Shortcut | Action |
|---|---|
| **Up / Down / Left / Right** | Pan & Tilt (continuous while held; supports simultaneous diagonals) |
| **Page Up / Page Down** | Zoom In / Zoom Out |
| **1 - 6** | Go To Preset 1 - 6 |
| **Shift + 1 - 6** / **Ctrl + 1 - 6** | Mark Preset 1 - 6 (saves current position) |
| **Home / H** | Center Camera to Home position |
| **Space** | Emergency Stop (instantly halts all motor drives) |
| **P** | Toggle Camera Power (Standby / On) |
| **M** | Unmute Video & Reset Auto-Exposure |

---

## Quick Start

### Build and Run
```bash
cargo run --release
```

### Adding to OBS Studio
1. In OBS Studio, click **+** under **Sources** and select **Browser**.
2. Set **URL** to: `http://localhost:8080/overlay`
3. Set **Width:** `1920` and **Height:** `1080`.
4. Set **Shutdown source when not visible** to OFF.
5. Set **Refresh browser when scene becomes active** to ON.
6. Position the Browser source above your camera feed in the scene list.

---

## Architecture

* **UI Framework:** [egui](https://github.com/emilk/egui) / [eframe](https://github.com/emilk/egui/tree/master/crates/eframe) (Pure Rust immediate-mode GUI; runs on Linux, Windows, macOS).
* **Serial Engine:** [serialport-rs](https://crates.io/crates/serialport) (Cross-platform raw binary serial transport).
* **Local Web Server:** [tiny_http](https://crates.io/crates/tiny_http) (Embedded lightweight graphics server for OBS).
