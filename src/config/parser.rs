use super::settings::AppConfig;
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;
use tracing::warn;

impl AppConfig {
    /// Parse Unraid's simple KEY="VALUE" config format.
    pub(crate) fn parse_ini(&mut self, contents: &str) {
        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim();
                let value = value.trim().trim_matches('"');

                match key {
                    "PORT" => match value.parse() {
                        Ok(v) => self.port = v,
                        Err(e) => warn!("Invalid PORT value '{}': {}", value, e),
                    },
                    "SCAN_THREADS" => match value.parse() {
                        Ok(v) => self.scan_threads = v,
                        Err(e) => warn!("Invalid SCAN_THREADS value '{}': {}", value, e),
                    },
                    "SLIDER_ALPHA" => match value.parse() {
                        Ok(v) => self.slider_alpha = v,
                        Err(e) => warn!("Invalid SLIDER_ALPHA value '{}': {}", value, e),
                    },
                    "MAX_TOLERANCE" => match value.parse() {
                        Ok(v) => self.max_tolerance = v,
                        Err(e) => warn!("Invalid MAX_TOLERANCE value '{}': {}", value, e),
                    },
                    "MIN_FREE_HEADROOM" => match value.parse() {
                        Ok(v) => self.min_free_headroom = v,
                        Err(e) => warn!("Invalid MIN_FREE_HEADROOM value '{}': {}", value, e),
                    },
                    "EXCLUDED_DISKS" => {
                        self.excluded_disks = value
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
                    "WARN_PARITY_CHECK" => {
                        self.warn_parity_check = value == "yes" || value == "true" || value == "1";
                    }
                    "CATALOG_PATH" => {
                        if !value.is_empty() {
                            self.db_path = value.to_string();
                        }
                    }
                    _ => {} // Ignore unknown keys
                }
            }
        }
    }

    /// Save current config back to the Unraid INI file.
    pub fn save(&self) -> Result<()> {
        use super::defaults::DEFAULT_DB_PATH;

        let excluded = self.excluded_disks.iter().cloned().collect::<Vec<_>>().join(",");

        // Write CATALOG_PATH only when the user has set a custom (non-default) location.
        let catalog_path = if self.db_path == DEFAULT_DB_PATH { "" } else { &self.db_path };

        let contents = format!(
            r#"# Perfectly Balanced configuration
# Auto-generated — edit via the plugin UI
PORT="{}"
SCAN_THREADS="{}"
SLIDER_ALPHA="{}"
MAX_TOLERANCE="{}"
MIN_FREE_HEADROOM="{}"
EXCLUDED_DISKS="{}"
WARN_PARITY_CHECK="{}"
CATALOG_PATH="{}"
"#,
            self.port,
            self.scan_threads,
            self.slider_alpha,
            self.max_tolerance,
            self.min_free_headroom,
            excluded,
            if self.warn_parity_check { "yes" } else { "no" },
            catalog_path,
        );

        if let Some(parent) = Path::new(&self.config_path).parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(&self.config_path, contents)
            .with_context(|| format!("Failed to write config to {}", self.config_path))?;

        Ok(())
    }
}
