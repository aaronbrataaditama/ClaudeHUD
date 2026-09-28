//! `claudehud.settings.json`, beside the exe when writable, else `%APPDATA%\ClaudeHUD\` (§6).

use crate::collectors::strip_bom;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

pub const FILE_NAME: &str = "claudehud.settings.json";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Edge {
    #[default]
    Top,
    Left,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub edge: Edge,
    pub monitor: String,
    pub warn_percent: u8,
    pub usage_poll_s: u32,
    pub status_poll_s: u32,
    pub autostart: bool,
    pub first_run_done: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            edge: Edge::Top,
            monitor: "primary".to_string(),
            warn_percent: 85,
            usage_poll_s: 300,
            status_poll_s: 300,
            autostart: false,
            first_run_done: false,
        }
    }
}

impl Settings {
    pub fn sanitised(mut self) -> Settings {
        self.warn_percent = self.warn_percent.clamp(50, 99);
        self.usage_poll_s = self.usage_poll_s.max(60);
        self.status_poll_s = self.status_poll_s.max(60);
        if self.monitor.trim().is_empty() {
            self.monitor = "primary".to_string();
        }
        self
    }
}

/// Missing or corrupt file → defaults. Never fails.
pub fn load(path: &Path) -> Settings {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Settings>(strip_bom(&t)).ok())
        .unwrap_or_default()
        .sanitised()
}

/// Atomic: write a temp file then rename over the old one.
pub fn save(path: &Path, s: &Settings) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(s).map_err(io::Error::other)?;
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

fn dir_writable(dir: &Path) -> bool {
    let probe = dir.join(".claudehud-write-test");
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

pub fn settings_path(exe_dir: &Path, appdata: Option<&Path>) -> PathBuf {
    let beside = exe_dir.join(FILE_NAME);
    if beside.is_file() || dir_writable(exe_dir) {
        return beside;
    }
    match appdata {
        Some(a) => a.join("ClaudeHUD").join(FILE_NAME),
        None => beside,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_spec() {
        let s = Settings::default();
        assert_eq!(s.edge, Edge::Top);
        assert_eq!(s.monitor, "primary");
        assert_eq!(
            (s.warn_percent, s.usage_poll_s, s.status_poll_s),
            (85, 300, 300)
        );
        assert!(!s.autostart && !s.first_run_done);
    }

    #[test]
    fn sanitise_clamps_out_of_range_values() {
        let s = Settings {
            warn_percent: 5,
            usage_poll_s: 1,
            status_poll_s: 0,
            monitor: String::new(),
            ..Default::default()
        }
        .sanitised();
        assert_eq!(
            (
                s.warn_percent,
                s.usage_poll_s,
                s.status_poll_s,
                s.monitor.as_str()
            ),
            (50, 60, 60, "primary")
        );
        assert_eq!(
            Settings {
                warn_percent: 250,
                ..Default::default()
            }
            .sanitised()
            .warn_percent,
            99
        );
    }

    #[test]
    fn partial_json_uses_defaults_and_ignores_unknown_fields() {
        let s: Settings = serde_json::from_str(r#"{"edge":"left","colour":"pink"}"#).unwrap();
        assert_eq!(s.edge, Edge::Left);
        assert_eq!(s.warn_percent, 85);
    }
}
