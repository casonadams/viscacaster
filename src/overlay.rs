//! Embedded Local Web Server for OBS Studio Browser Sources and Overlays
//!
//! Serves transparent 60 FPS HTML5/CSS3 graphics at http://localhost:8080/overlay
//! that can be added directly into OBS Studio as a Browser Source.

use std::sync::{Arc, RwLock};
use std::thread;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LowerThirdData {
    pub title: String,
    pub subtitle: String,
    pub visible: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HymnOverlayData {
    pub hymn_num: u16,
    pub title: String,
    pub section_title: String,
    pub text: String,
    pub visible: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TickerData {
    pub text: String,
    pub visible: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OverlayState {
    pub lower_third: LowerThirdData,
    pub hymn: HymnOverlayData,
    pub ticker: TickerData,
}

pub struct OverlayServer {
    pub state: Arc<RwLock<OverlayState>>,
    pub port: u16,
}

impl OverlayServer {
    pub fn start(start_port: u16) -> Self {
        let state = Arc::new(RwLock::new(OverlayState::default()));
        let state_clone = Arc::clone(&state);

        let mut actual_server = None;
        let mut port = start_port;
        for p in start_port..start_port + 10 {
            let addr = format!("0.0.0.0:{}", p);
            if let Ok(s) = tiny_http::Server::http(&addr) {
                actual_server = Some(s);
                port = p;
                break;
            }
        }

        if let Some(server) = actual_server {
            println!("[Overlay] Live graphics server listening at http://localhost:{}/overlay", port);
            thread::Builder::new()
                .name("overlay-web-server".into())
                .spawn(move || {
                    run_http_server(server, state_clone);
                })
                .expect("Failed to start overlay HTTP server");
        } else {
            eprintln!("[Overlay] Could not bind to any port in range {}..{}", start_port, start_port + 10);
        }

        Self { state, port }
    }
}

fn run_http_server(server: tiny_http::Server, state: Arc<RwLock<OverlayState>>) {

    for request in server.incoming_requests() {
        let url = request.url().to_string();

        if url == "/api/state" {
            let json = {
                let st = state.read().unwrap();
                serde_json::to_string(&*st).unwrap_or_else(|_| "{}".into())
            };
            let response = tiny_http::Response::from_string(json)
                .with_header(tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap())
                .with_header(tiny_http::Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap());
            let _ = request.respond(response);
        } else {
            // Serve the overlay HTML
            let html = OVERLAY_HTML;
            let response = tiny_http::Response::from_string(html)
                .with_header(tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]).unwrap());
            let _ = request.respond(response);
        }
    }
}

const OVERLAY_HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<style>
  * { box-sizing: border-box; margin: 0; padding: 0; }
  body {
    background: transparent;
    overflow: hidden;
    width: 100vw;
    height: 100vh;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
  }

  /* ------------------------------------------------------------- */
  /* Lower Third (Speaker & Sermon) */
  /* ------------------------------------------------------------- */
  #lower-third {
    position: absolute;
    bottom: 80px;
    left: 80px;
    display: flex;
    flex-direction: column;
    opacity: 0;
    transform: translateY(30px);
    transition: opacity 0.5s cubic-bezier(0.16, 1, 0.3, 1), transform 0.5s cubic-bezier(0.16, 1, 0.3, 1);
  }
  #lower-third.visible {
    opacity: 1;
    transform: translateY(0);
  }
  .lt-card {
    background: rgba(15, 23, 42, 0.88);
    border-left: 6px solid #38bdf8;
    backdrop-filter: blur(12px);
    padding: 16px 28px;
    border-radius: 0 12px 12px 0;
    box-shadow: 0 10px 25px rgba(0,0,0,0.5), 0 0 1px rgba(255,255,255,0.2);
  }
  .lt-title {
    font-size: 32px;
    font-weight: 800;
    color: #ffffff;
    text-shadow: 0 2px 4px rgba(0,0,0,0.6);
    letter-spacing: 0.5px;
  }
  .lt-subtitle {
    font-size: 20px;
    font-weight: 500;
    color: #94a3b8;
    margin-top: 4px;
    text-shadow: 0 2px 4px rgba(0,0,0,0.6);
  }

  /* ------------------------------------------------------------- */
  /* Hymn / Lyric Overlay (Center Lower-Third) */
  /* ------------------------------------------------------------- */
  #hymn-overlay {
    position: absolute;
    bottom: 75px;
    left: 50%;
    transform: translateX(-50%) translateY(20px);
    opacity: 0;
    width: 85%;
    max-width: 1400px;
    display: flex;
    flex-direction: column;
    align-items: center;
    transition: opacity 0.4s ease, transform 0.4s ease;
  }
  #hymn-overlay.visible {
    opacity: 1;
    transform: translateX(-50%) translateY(0);
  }
  .hymn-header-badge {
    background: #0284c7;
    color: #ffffff;
    font-size: 15px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 1px;
    padding: 6px 18px;
    border-radius: 20px;
    margin-bottom: 8px;
    box-shadow: 0 4px 10px rgba(0,0,0,0.4);
  }
  .hymn-card {
    background: rgba(15, 23, 42, 0.90);
    border: 1px solid rgba(255, 255, 255, 0.15);
    backdrop-filter: blur(14px);
    padding: 20px 40px;
    border-radius: 16px;
    text-align: center;
    box-shadow: 0 12px 30px rgba(0,0,0,0.6);
    width: 100%;
  }
  .hymn-text {
    font-size: 34px;
    font-weight: 700;
    color: #ffffff;
    line-height: 1.4;
    text-shadow: 0 2px 8px rgba(0,0,0,0.8);
    white-space: pre-line;
  }

  /* ------------------------------------------------------------- */
  /* Marquee Announcement Ticker (Bottom Bar) */
  /* ------------------------------------------------------------- */
  #ticker-bar {
    position: absolute;
    bottom: 0;
    left: 0;
    width: 100%;
    height: 48px;
    background: rgba(10, 15, 28, 0.95);
    border-top: 2px solid #38bdf8;
    display: flex;
    align-items: center;
    overflow: hidden;
    opacity: 0;
    transition: opacity 0.5s ease;
  }
  #ticker-bar.visible {
    opacity: 1;
  }
  .ticker-badge {
    background: #0284c7;
    color: white;
    font-weight: 800;
    font-size: 14px;
    padding: 0 20px;
    height: 100%;
    display: flex;
    align-items: center;
    letter-spacing: 1px;
    z-index: 10;
  }
  .ticker-track {
    white-space: nowrap;
    will-change: transform;
    animation: marquee 35s linear infinite;
    font-size: 20px;
    font-weight: 600;
    color: #f1f5f9;
    padding-left: 100%;
  }
  @keyframes marquee {
    0%   { transform: translate(0, 0); }
    100% { transform: translate(-100%, 0); }
  }
</style>
</head>
<body>

  <!-- Lower Third -->
  <div id="lower-third">
    <div class="lt-card">
      <div id="lt-title" class="lt-title">Speaker Name</div>
      <div id="lt-subtitle" class="lt-subtitle">Sacrament Meeting</div>
    </div>
  </div>

  <!-- Hymn Lyrics -->
  <div id="hymn-overlay">
    <div id="hymn-badge" class="hymn-header-badge">Hymn #193 • Verse 1</div>
    <div class="hymn-card">
      <div id="hymn-text" class="hymn-text">I stand all amazed at the love Jesus offers me...</div>
    </div>
  </div>

  <!-- Announcement Ticker -->
  <div id="ticker-bar">
    <div class="ticker-badge">ANNOUNCEMENTS</div>
    <div id="ticker-text" class="ticker-track">Welcome to our worship service • Sacrament Meeting • Youth Activities Wednesday at 7:00 PM</div>
  </div>

<script>
  let lastState = {};

  async function pollState() {
    try {
      const res = await fetch('/api/state');
      if (res.ok) {
        const state = await res.json();
        render(state);
      }
    } catch(e) {}
    setTimeout(pollState, 150);
  }

  function render(state) {
    // 1. Lower Third
    const ltEl = document.getElementById('lower-third');
    if (state.lower_third && state.lower_third.visible) {
      document.getElementById('lt-title').textContent = state.lower_third.title;
      document.getElementById('lt-subtitle').textContent = state.lower_third.subtitle;
      ltEl.classList.add('visible');
    } else {
      ltEl.classList.remove('visible');
    }

    // 2. Hymn Overlay
    const hymnEl = document.getElementById('hymn-overlay');
    if (state.hymn && state.hymn.visible) {
      document.getElementById('hymn-badge').textContent = state.hymn.section_title || `Hymn #${state.hymn.hymn_num}`;
      document.getElementById('hymn-text').textContent = state.hymn.text;
      hymnEl.classList.add('visible');
    } else {
      hymnEl.classList.remove('visible');
    }

    // 3. Ticker
    const tickerEl = document.getElementById('ticker-bar');
    if (state.ticker && state.ticker.visible && state.ticker.text.trim()) {
      document.getElementById('ticker-text').textContent = state.ticker.text;
      tickerEl.classList.add('visible');
    } else {
      tickerEl.classList.remove('visible');
    }
  }

  pollState();
</script>
</body>
</html>
"#;
