use anyhow::{bail, Result};
use std::path::Path;

/// A disk discovered in the Unraid /mnt/ mount hierarchy.
pub(crate) struct DiscoveredDisk {
    pub name: String,
    pub mount_path: String,
}

/// Discover Unraid array disks by reading /mnt/ mount points.
pub(crate) fn discover_disks(mnt_base: &str) -> Result<Vec<DiscoveredDisk>> {
    let mnt_path = Path::new(mnt_base);
    if !mnt_path.exists() {
        bail!("Mount base path does not exist: {mnt_base}");
    }

    let mut disks = Vec::new();

    for entry in std::fs::read_dir(mnt_path)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();

        let is_array_disk = name.starts_with("disk")
            && name.len() > 4
            && name[4..].chars().all(|c| c.is_ascii_digit());
        let is_cache = name == "cache"
            || (name.starts_with("cache")
                && name.len() > 5
                && name[5..].chars().all(|c| c.is_ascii_digit()));

        if is_array_disk || is_cache {
            let mount_path = format!("{mnt_base}/{name}");
            if Path::new(&mount_path).is_dir() {
                disks.push(DiscoveredDisk { name, mount_path });
            }
        }
    }

    disks.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(disks)
}
