use anyhow::{bail, Result};
use std::path::Path;

/// Hard reject any path under FUSE mount points to prevent data corruption.
/// This is the single most critical safety check in the entire plugin.
pub(crate) fn validate_path(path: &str) -> Result<()> {
    let p = Path::new(path);
    for prefix in ["/mnt/user", "/mnt/user0"] {
        let prefix_path = Path::new(prefix);
        if p == prefix_path || p.starts_with(prefix_path) {
            bail!(
                "SAFETY: Path '{path}' uses Unraid's FUSE layer ({prefix}/). \
                 This plugin must only operate on direct disk paths (/mnt/diskX/). \
                 Using FUSE paths can cause data corruption."
            );
        }
    }
    Ok(())
}
