use anyhow::{Context, Result};
use log::warn;
use omnect_ui_core::types::TimeoutSettings;
use std::{fs, path::PathBuf};

pub struct SettingsService;

impl SettingsService {
    /// Return current timeout settings, falling back to defaults if the file is missing or corrupt.
    #[must_use]
    pub fn get() -> TimeoutSettings {
        match Self::load() {
            Ok(settings) => settings,
            Err(e) => {
                warn!("failed to load settings, using defaults: {e:#}");
                TimeoutSettings::default()
            }
        }
    }

    /// Persist timeout settings to disk.
    pub fn save(settings: &TimeoutSettings) -> Result<()> {
        let path = Self::settings_path();
        let json =
            serde_json::to_string_pretty(settings).context("failed to serialize settings")?;
        fs::write(&path, json).context(format!("failed to write settings file: {}", path.display()))
    }

    fn load() -> Result<TimeoutSettings> {
        let path = Self::settings_path();
        let content = fs::read_to_string(&path)
            .context(format!("failed to read settings file: {}", path.display()))?;
        serde_json::from_str(&content).context("failed to deserialize settings")
    }

    fn settings_path() -> PathBuf {
        // Derive path from the config dir already created by PathConfig::load()
        crate::config::AppConfig::get()
            .paths
            .app_config_path
            .parent()
            .expect("app_config_path has no parent")
            .join("timeouts.json")
    }
}
