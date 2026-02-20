use anyhow::{bail, Result};

/// Hard reject any path containing /mnt/user/ to prevent FUSE corruption.
/// This is the single most critical safety check in the entire plugin.
pub(crate) fn validate_path(path: &str) -> Result<()> {
    if path.contains("/mnt/user/") || path.contains("/mnt/user0/") {
        bail!(
            "SAFETY: Path '{path}' uses Unraid's FUSE layer (/mnt/user/). \
             This plugin must only operate on direct disk paths (/mnt/diskX/). \
             Using FUSE paths can cause data corruption."
        );
    }
    Ok(())
}
