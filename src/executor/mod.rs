pub(crate) mod recovery;

use anyhow::{bail, Context, Result};
use regex::Regex;
use std::sync::OnceLock;
use tokio::process::Command;
use tracing::info;

/// Cached result of rsync --info=progress2 support check.
static RSYNC_PROGRESS2: OnceLock<bool> = OnceLock::new();

/// Check rsync version to determine if --info=progress2 is supported (>= 3.1.0).
async fn probe_rsync_progress2() -> Result<bool> {
    let output = Command::new("rsync")
        .arg("--version")
        .output()
        .await
        .context("Failed to execute rsync --version")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let re = Regex::new(r"rsync\s+version\s+(\d+)\.(\d+)\.(\d+)")?;

    if let Some(caps) = re.captures(&stdout) {
        let major: u32 = caps[1].parse()?;
        let minor: u32 = caps[2].parse()?;
        let patch: u32 = caps[3].parse()?;
        info!("rsync version: {major}.{minor}.{patch}");
        Ok(major > 3 || (major == 3 && minor >= 1))
    } else {
        bail!("Could not parse rsync version from output");
    }
}

/// Check if rsync supports --info=progress2 (cached after first call).
pub(crate) async fn rsync_supports_progress2() -> bool {
    if let Some(&cached) = RSYNC_PROGRESS2.get() {
        return cached;
    }
    let result = probe_rsync_progress2().await.unwrap_or(false);
    *RSYNC_PROGRESS2.get_or_init(|| result)
}

/// Check if a file is currently open by another process via lsof.
pub(crate) async fn is_file_open(path: &str) -> bool {
    match Command::new("lsof").arg(path).output().await {
        Ok(output) => {
            // lsof returns 0 if the file is open, 1 if not
            output.status.success()
        }
        Err(_) => {
            // If lsof isn't available, assume not open
            false
        }
    }
}

/// Check if a parity check is currently running.
///
/// Looks for active resync/check progress indicators in `/proc/mdstat`.
/// The progress lines contain patterns like `resync = 42.5%` or `check = 12.3%`.
/// We match on ` = ` suffix to avoid false-positives from the word "check"
/// appearing in other mdstat contexts.
pub(crate) async fn is_parity_check_running() -> bool {
    match tokio::fs::read_to_string("/proc/mdstat").await {
        Ok(content) => content.contains("resync =") || content.contains("check ="),
        Err(_) => false,
    }
}
