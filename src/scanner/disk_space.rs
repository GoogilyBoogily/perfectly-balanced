use anyhow::{bail, Result};

/// Disk space measurements in bytes.
pub(crate) struct DiskSpace {
    pub total: u64,
    pub used: u64,
    pub free: u64,
}

/// Get disk space info via statvfs.
#[cfg(unix)]
#[allow(unsafe_code)]
pub(crate) fn get_disk_space(mount_path: &str) -> Result<DiskSpace> {
    use std::ffi::CString;

    let c_path = CString::new(mount_path)?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };

    let ret = unsafe { libc::statvfs(c_path.as_ptr(), &raw mut stat) };
    if ret != 0 {
        bail!("statvfs failed for {}: {}", mount_path, std::io::Error::last_os_error());
    }

    let block_size = stat.f_frsize as u64;
    let total = stat.f_blocks as u64 * block_size;
    let free = stat.f_bfree as u64 * block_size;
    let used = total.saturating_sub(free);

    Ok(DiskSpace { total, used, free })
}

/// Fallback for non-unix platforms (development on macOS/Windows).
#[cfg(not(unix))]
pub fn get_disk_space(_mount_path: &str) -> Result<DiskSpace> {
    tracing::warn!("get_disk_space: using dummy values on non-unix platform");
    Ok(DiskSpace { total: 1_000_000_000_000, used: 500_000_000_000, free: 500_000_000_000 })
}
