//! `config.toml` (PLAN section 5.2): defaults, validation, atomic save.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Log levels accepted in the config file.
pub const LOG_LEVELS: [&str; 5] = ["error", "warn", "info", "debug", "trace"];

/// User settings. Unknown keys are ignored; missing keys use the defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Name shown to other devices.
    pub device_name: String,
    /// TCP port.
    pub port: u16,
    /// Pause all synchronisation.
    pub paused: bool,
    /// Sync text.
    pub sync_text: bool,
    /// Sync images.
    pub sync_images: bool,
    /// Accept incoming files.
    pub receive_files: bool,
    /// Receive folder; empty means `Downloads\TransClip`.
    pub download_dir: String,
    /// Start with Windows.
    pub autostart: bool,
    /// Virtual-key name of the radial menu trigger.
    pub radial_trigger: String,
    /// Radial menu radius in pixels at 100 % scaling.
    pub radial_radius_px: u32,
    /// Peers that use Bluetooth with priority.
    pub bt_priority_peers: Vec<String>,
    /// Largest image sent inline over Bluetooth.
    pub bt_inline_image_max: u64,
    /// Largest accepted incoming file; 0 means unlimited.
    pub max_incoming_file_bytes: u64,
    /// One of [`LOG_LEVELS`].
    pub log_level: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            device_name: std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Windows PC".into()),
            port: 47820,
            paused: false,
            sync_text: true,
            sync_images: true,
            receive_files: true,
            download_dir: String::new(),
            autostart: true,
            radial_trigger: "Control".into(),
            radial_radius_px: 140,
            bt_priority_peers: Vec::new(),
            bt_inline_image_max: 2 * 1024 * 1024,
            max_incoming_file_bytes: 0,
            log_level: "info".into(),
        }
    }
}

impl Config {
    /// Bring every value into its allowed range instead of rejecting the whole file.
    pub fn validate(&mut self) {
        let defaults = Self::default();
        self.device_name = self.device_name.trim().to_string();
        if self.device_name.is_empty() {
            self.device_name = defaults.device_name;
        }
        if self.device_name.len() > 64 {
            let mut end = 64;
            while !self.device_name.is_char_boundary(end) {
                end -= 1;
            }
            self.device_name.truncate(end);
        }
        if self.port < 1024 {
            self.port = defaults.port;
        }
        // Older versions stored left/right names; they become the shared spelling.
        self.radial_trigger =
            crate::radial::keys::canonical(&self.radial_trigger).unwrap_or(defaults.radial_trigger);
        self.radial_radius_px = self.radial_radius_px.clamp(60, 400);
        self.bt_inline_image_max = self.bt_inline_image_max.min(50 * 1024 * 1024);
        if !LOG_LEVELS.contains(&self.log_level.as_str()) {
            self.log_level = defaults.log_level;
        }
        self.bt_priority_peers
            .retain(|id| proto::ids::is_lower_hex(id) && id.len() == 32);
        self.bt_priority_peers.dedup();
    }

    /// Load the file. A missing file gives the defaults. A corrupt file is moved to
    /// `config.toml.bad` and replaced by the defaults, so the daemon always starts.
    pub fn load(path: &Path) -> Self {
        let mut cfg = match std::fs::read_to_string(path) {
            Ok(text) => match toml::from_str::<Self>(&text) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(error = %e, "config.toml is invalid, using defaults");
                    let _ = std::fs::rename(path, path.with_extension("toml.bad"));
                    Self::default()
                }
            },
            Err(_) => Self::default(),
        };
        cfg.validate();
        cfg
    }

    /// Write through a temporary file and rename, so a crash never leaves a half-written file.
    pub fn save(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self).context("cannot serialise config")?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text).context("cannot write config")?;
        std::fs::rename(&tmp, path).context("cannot replace config")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        let mut c = Config::default();
        c.validate();
        c.save(&p).unwrap();
        assert_eq!(Config::load(&p), c);
        assert!(!p.with_extension("toml.tmp").exists());
    }

    #[test]
    fn partial_file_fills_in_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(&p, "port = 50000\nunknown = 1\n").unwrap();
        let c = Config::load(&p);
        assert_eq!(c.port, 50000);
        assert!(c.sync_text);
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let mut c = Config {
            port: 80,
            radial_radius_px: 5000,
            log_level: "verbose".into(),
            device_name: "   ".into(),
            bt_priority_peers: vec!["zz".into(), "ab".repeat(16)],
            ..Config::default()
        };
        c.validate();
        assert_eq!(c.port, 47820);
        assert_eq!(c.radial_radius_px, 400);
        assert_eq!(c.log_level, "info");
        assert!(!c.device_name.is_empty());
        assert_eq!(c.bt_priority_peers.len(), 1);
    }

    #[test]
    fn corrupt_file_is_kept_aside() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(&p, "this is = not [valid").unwrap();
        let c = Config::load(&p);
        assert_eq!(c.port, 47820);
        assert!(dir.path().join("config.toml.bad").exists());
    }

    #[test]
    fn long_multibyte_name_is_cut_on_a_character_boundary() {
        let mut c = Config {
            device_name: "\u{65e5}".repeat(40),
            ..Config::default()
        };
        c.validate();
        assert!(c.device_name.len() <= 64);
    }
}
