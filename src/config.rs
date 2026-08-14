//! User configuration persistence (volume, play mode, bitrate).
//! Stored as TOML under `<config>/rust-musicfox/config.toml`.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Playback volume 0.0 .. 1.0
    #[serde(default = "default_volume")]
    pub volume: f32,
    /// Play mode: list / single / shuffle / sequence
    #[serde(default = "default_play_mode")]
    pub play_mode: String,
    /// Requested audio bitrate (128000 / 320000 / ...)
    #[serde(default = "default_br")]
    pub br: u32,
}

fn default_volume() -> f32 {
    0.8
}
fn default_play_mode() -> String {
    "list".into()
}
fn default_br() -> u32 {
    128000
}

impl Default for Config {
    fn default() -> Self {
        Config {
            volume: default_volume(),
            play_mode: default_play_mode(),
            br: default_br(),
        }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        crate::api::data_dir().join("config.toml")
    }

    pub fn load() -> Result<Self> {
        let path = Self::path();
        if !path.exists() {
            return Ok(Config::default());
        }
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("read config from {}", path.display()))?;
        let cfg: Config = toml::from_str(&raw).unwrap_or_default();
        Ok(cfg)
    }

    pub fn save(&self) {
        if let Ok(toml) = toml::to_string(self) {
            if let Some(parent) = Self::path().parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(Self::path(), toml);
        }
    }
}
