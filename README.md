# Sony EVI-D70 VISCA PTZ & Church Broadcast Controller

A high-performance, cross-platform desktop application written in **Rust** for controlling Sony EVI-D70 (and compatible VISCA) PTZ cameras, paired with an integrated broadcast graphics engine for church livestreams (OBS Studio, vMix, etc.).

---

## Features

### 🎥 Hardware Camera Control (VISCA over Serial)
* **Smooth Directional D-Pad:** 8-way directional movement (including simultaneous diagonals) and Center Home.
* **Continuous Press-and-Hold:** Streams movement while held (keyboard or mouse/touch), halts on release.
* **Optical Zoom Rocker:** Tele (Zoom In) and Wide (Zoom Out) controls.
* **Hardware Memory Presets (1–6):**
  * Instant recall (**Go To**) and live saving (**Mark**).
  * Custom preset names saved across reboots.
* **Dual Speed Sliders:** Independent sliders for Pan/Tilt speed ($1$–$24$) and Optical Zoom speed ($0$–$7$).
* **Smart Power & Video Management:** Instant standby/power toggle and one-click video un-mute + auto-exposure reset.

### 🔍 Smart Port Auto-Detection + Manual Selection
* **3-Tier Automatic Port Detection:**
  1. Actively probes serial ports with non-blocking VISCA handshakes.
  2. Inspects USB device metadata for known camera converters (Keyspan USA-19HS, FTDI, CP210x, CH340, PL2303).
  3. Filters out legacy unpopulated motherboard UARTs.
* **Manual Dropdown:** Switch to any serial port on the fly.

### 🎵 Church Hymns & Lyric Presentation
* **Full Embedded Catalog (Hymns 1–341+):**
  * Instant numerical lookup (e.g. typing `193` displays *"I Stand All Amazed"*).
  * Fast title search.
  * Sunday Service Quick Picks for instant access.
* **Automatic Lyric Downloader:**
  * Pulls official verses and choruses from `churchofjesuschrist.org` and caches them locally for offline use.
* **Slide Carousel:**
  * Advance lyrics verse-by-verse during singing (`Next Verse` / `Prev Verse` or jump directly to any verse/chorus).
  * One-click **Live on Stream** toggle.

### 🏷️ Broadcast Overlays (OBS Studio Integration)
* **Embedded Graphics Server (`http://localhost:8080/overlay`):**
  * Transparent 1080p/4K HTML5/CSS3 layer rendered at 60 FPS directly into OBS Studio as a **Browser Source**.
* **Speaker & Sermon Lower Thirds:**
  * Customizable Speaker Name and Sermon/Title with animated entrance/exit transitions.
  * **Smart Director Automation:** Automatically triggers the speaker's lower third for 8 seconds whenever **Preset 1 (Podium)** is recalled!
* **Announcement Marquee Ticker:**
  * Hardware-accelerated scrolling announcement ticker along the bottom of the stream.

---

## Keyboard Shortcuts

| Shortcut | Action |
|---|---|
| **$\uparrow$ / $\downarrow$ / $\leftarrow$ / $\rightarrow$** | **Pan & Tilt** (continuous while held; supports simultaneous diagonals) |
| **`Page Up` / `Page Down`** | **Zoom In / Zoom Out** |
| **`1` – `6`** | **Go To** Preset 1 – 6 |
| **`Shift + 1` – `6`** / **`Ctrl + 1` – `6`** | **Mark** Preset 1 – 6 (saves current position) |
| **`Home`** or **`H`** | **Center Camera** to Home position |
| **`Space`** | **Emergency Stop** (instantly halts all motors) |
| **`P`** | **Toggle Camera Power** (Standby / On) |
| **`M`** | **Unmute Video** & Reset Auto-Exposure |

---

## Quick Start

### Build & Run
```bash
cargo run --release
```

### Adding to OBS Studio (Takes 30 Seconds)
1. In OBS Studio, click **`+`** under **Sources** and select **Browser**.
2. Set **URL** to: `http://localhost:8080/overlay`
3. Set **Width:** `1920` and **Height:** `1080`.
4. Check **"Shutdown source when not visible"** $\rightarrow$ `OFF`.
5. Check **"Refresh browser when scene becomes active"** $\rightarrow$ `ON`.
6. Position the Browser source above your camera feed in the scene list.

---

## Architecture

* **UI Framework:** [egui](https://github.com/emilk/egui) / [eframe](https://github.com/emilk/egui/tree/master/crates/eframe) (Pure Rust immediate-mode GUI; runs identically on Linux, Windows, macOS).
* **Serial Engine:** [serialport-rs](https://crates.io/crates/serialport) (Cross-platform raw binary serial transport).
* **Local Web Server:** [tiny_http](https://crates.io/crates/tiny_http) (Embedded lightweight graphics server for OBS).
