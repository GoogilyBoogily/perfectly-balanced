use super::defaults::{
    DEFAULT_CONFIG_PATH, DEFAULT_DB_PATH, DEFAULT_MIN_FREE_HEADROOM, DEFAULT_PORT,
    DEFAULT_SCAN_THREADS, DEFAULT_SLIDER_ALPHA, UNRAID_MNT_BASE,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub port: u16,
    pub db_path: String,
    pub config_path: String,
    pub scan_threads: usize,
    /// Balance slider value [0.0, 1.0].
    /// 0.0 = fewest moves (high tolerance), 1.0 = perfect balance (low tolerance).
    pub slider_alpha: f64,
    /// Maximum tolerance percentage. Effective tolerance = max_tolerance * (1 - slider_alpha).
    pub max_tolerance: f64,
    /// Minimum free space headroom per disk in bytes.
    pub min_free_headroom: u64,
    /// Disk names explicitly excluded by the user (e.g., "disk3", "cache").
    pub excluded_disks: HashSet<String>,
    /// Base mount path for Unraid array disks.
    pub mnt_base: String,
    pub warn_parity_check: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            db_path: DEFAULT_DB_PATH.to_string(),
            config_path: DEFAULT_CONFIG_PATH.to_string(),
            scan_threads: DEFAULT_SCAN_THREADS,
            slider_alpha: DEFAULT_SLIDER_ALPHA,
            max_tolerance: 0.15,
            min_free_headroom: DEFAULT_MIN_FREE_HEADROOM,
            excluded_disks: HashSet::new(),
            mnt_base: UNRAID_MNT_BASE.to_string(),
            warn_parity_check: true,
        }
    }
}

impl AppConfig {
    /// Load configuration, merging defaults with config file values and env overrides.
    pub fn load() -> Result<Self> {
        let mut config = Self::default();

        if let Ok(path) = std::env::var("PB_CONFIG_PATH") {
            config.config_path = path;
        }
        if let Ok(path) = std::env::var("PB_DB_PATH") {
            config.db_path = path;
        }
        if let Ok(port) = std::env::var("PB_PORT") {
            config.port = port.parse().context("PB_PORT must be a valid port number")?;
        }
        if let Ok(base) = std::env::var("PB_MNT_BASE") {
            config.mnt_base = base;
        }

        let cfg_path = Path::new(&config.config_path);
        if cfg_path.exists() {
            let contents = fs::read_to_string(cfg_path)
                .with_context(|| format!("Failed to read config file: {}", config.config_path))?;
            config.parse_ini(&contents);
        }

        config.validate()?;
        Ok(config)
    }
}
