//! Cross-Platform Configuration and Preset Persistence

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

use crate::camera::ConnectionTarget;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub connection_target: Option<ConnectionTarget>,
    pub ip_host: String,
    pub ip_port: u16,
    pub pan_speed: u8,
    pub tilt_speed: u8,
    pub zoom_speed: u8,
    pub presets: HashMap<u8, String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        let mut presets = HashMap::new();
        for i in 1..=6 {
            presets.insert(i, format!("Preset {}", i));
        }
        Self {
            connection_target: None,
            ip_host: "192.168.1.100".into(),
            ip_port: 52381, // Standard VISCA over IP port
            pan_speed: 10,
            tilt_speed: 10,
            zoom_speed: 4,
            presets,
        }
    }
}

fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|mut p| {
        p.push("viscacaster");
        p.push("config.json");
        p
    })
}

pub fn load_config() -> AppConfig {
    if let Some(path) = config_path() {
        if path.exists() {
            if let Ok(contents) = fs::read_to_string(&path) {
                if let Ok(cfg) = serde_json::from_str::<AppConfig>(&contents) {
                    return cfg;
                }
            }
        }
    }
    AppConfig::default()
}

pub fn save_config(cfg: &AppConfig) {
    if let Some(path) = config_path() {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(cfg) {
            let _ = fs::write(path, json);
        }
    }
}
