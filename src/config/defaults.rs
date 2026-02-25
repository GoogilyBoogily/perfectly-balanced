/// Default path where Unraid stores plugin config on the USB flash drive.
pub(crate) const DEFAULT_CONFIG_PATH: &str =
    "/boot/config/plugins/perfectly-balanced/perfectly-balanced.cfg";

/// Default path for the SQLite catalog database.
///
/// Uses /tmp (tmpfs / RAM-backed on Unraid) to avoid wearing out the USB
/// flash drive. The catalog is rebuilt from a fresh scan after each reboot.
/// Users who want persistence can set CATALOG_PATH in the config to a
/// cache-disk location (e.g. /mnt/cache/appdata/perfectly-balanced/catalog.db).
pub(crate) const DEFAULT_DB_PATH: &str = "/tmp/perfectly-balanced/catalog.db";

/// Default port the daemon listens on (localhost only).
pub(crate) const DEFAULT_PORT: u16 = 7091;

/// Default number of parallel scan threads.
pub(crate) const DEFAULT_SCAN_THREADS: usize = 2;

/// Default balance slider alpha value (0.0 = fewest moves, 1.0 = perfect balance).
pub(crate) const DEFAULT_SLIDER_ALPHA: f64 = 0.5;

/// Default minimum free space headroom per disk in bytes (1 GB).
pub(crate) const DEFAULT_MIN_FREE_HEADROOM: u64 = 1_073_741_824;

/// The base path where Unraid mounts array disks.
pub(crate) const UNRAID_MNT_BASE: &str = "/mnt";
