use super::settings::AppConfig;
use anyhow::Result;

impl AppConfig {
    /// Validate configuration values are sane.
    pub(crate) fn validate(&self) -> Result<()> {
        anyhow::ensure!(self.port > 0, "Port must be > 0");
        anyhow::ensure!(
            self.scan_threads >= 1 && self.scan_threads <= 32,
            "scan_threads must be between 1 and 32"
        );
        anyhow::ensure!(
            (0.0..=1.0).contains(&self.slider_alpha),
            "slider_alpha must be between 0.0 and 1.0"
        );
        anyhow::ensure!(
            self.max_tolerance > 0.0 && self.max_tolerance <= 1.0,
            "max_tolerance must be between 0.0 and 1.0"
        );
        Ok(())
    }
}
