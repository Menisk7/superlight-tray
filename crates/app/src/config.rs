use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// DPI values shown in the tray menu.
    pub presets: Vec<u16>,
    /// Last DPI picked in the menu; re-applied when the mouse (re)connects.
    pub last_dpi: Option<u16>,
    pub reapply_on_connect: bool,
    /// Battery poll interval while connected, seconds.
    pub poll_seconds: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self { presets: vec![400, 800, 1200, 1600, 3200, 6400], last_dpi: None, reapply_on_connect: true, poll_seconds: 60 }
    }
}

fn dirs() -> Option<directories::ProjectDirs> {
    directories::ProjectDirs::from("", "", "superlight-tray")
}

pub fn config_dir() -> PathBuf {
    dirs().map(|d| d.config_dir().to_path_buf()).unwrap_or_else(|| PathBuf::from("."))
}

pub fn data_dir() -> PathBuf {
    dirs().map(|d| d.data_dir().to_path_buf()).unwrap_or_else(|| PathBuf::from("."))
}

fn path() -> PathBuf {
    config_dir().join("config.toml")
}

impl Config {
    pub fn load() -> Self {
        let p = path();
        match fs::read_to_string(&p) {
            Ok(s) => match toml::from_str::<Config>(&s) {
                Ok(mut c) => {
                    c.presets.retain(|d| (100..=25600).contains(d));
                    c.presets.sort_unstable();
                    c.presets.dedup();
                    if c.presets.is_empty() {
                        c.presets = Config::default().presets;
                    }
                    c.poll_seconds = c.poll_seconds.clamp(10, 3600);
                    c
                }
                Err(e) => {
                    log::warn!("config {} invalid ({e}); using defaults", p.display());
                    Config::default()
                }
            },
            Err(_) => Config::default(),
        }
    }

    /// Atomic save: write temp file then rename.
    pub fn save(&self) {
        let p = path();
        let r = (|| -> std::io::Result<()> {
            fs::create_dir_all(config_dir())?;
            let tmp = p.with_extension("toml.tmp");
            fs::write(&tmp, toml::to_string_pretty(self).map_err(std::io::Error::other)?)?;
            fs::rename(&tmp, &p)
        })();
        if let Err(e) = r {
            log::error!("saving config failed: {e}");
        }
    }
}
