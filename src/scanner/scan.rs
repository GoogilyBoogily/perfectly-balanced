use super::validation::validate_path;
use crate::db::{Database, FileInsert};
use crate::events::{Event, EventHub};
use anyhow::{bail, Result};
use jwalk::{Parallelism, WalkDir};
use std::path::Path;
use std::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

/// Batch size for SQLite inserts during scanning.
const INSERT_BATCH_SIZE: usize = 2000;

/// Minimum interval between SSE progress updates (milliseconds).
const PROGRESS_INTERVAL_MS: u64 = 500;

/// All context needed to scan a single disk.
pub(crate) struct ScanContext<'a> {
    pub db: &'a Database,
    pub disk_id: i64,
    pub mount_path: &'a str,
    pub event_hub: &'a EventHub,
    pub cancel: CancellationToken,
    pub num_threads: usize,
}

/// Statistics from scanning a single disk.
pub(crate) struct ScanStats {
    pub files_scanned: u64,
    pub bytes_cataloged: u64,
}

/// Scan a single disk's filesystem and populate the database.
///
/// The entire operation (clear + inserts + folder recompute) runs in a single
/// transaction — if the scan fails or is cancelled, the previous catalog is preserved.
pub(crate) fn scan_disk(ctx: &ScanContext<'_>) -> Result<ScanStats> {
    validate_path(ctx.mount_path)?;

    let mount = Path::new(ctx.mount_path);
    if !mount.exists() {
        bail!("Mount path does not exist: {}", ctx.mount_path);
    }
    if !mount.is_dir() {
        bail!("Mount path is not a directory: {}", ctx.mount_path);
    }

    let disk_name = Path::new(ctx.mount_path)
        .file_name()
        .map_or_else(|| ctx.mount_path.to_string(), |n| n.to_string_lossy().to_string());

    info!("Starting scan of {} (disk_id={})", ctx.mount_path, ctx.disk_id);

    ctx.db.begin_disk_scan(ctx.disk_id)?;

    let result = run_walk(ctx, &disk_name);

    match &result {
        Ok(_) => {
            ctx.db.commit_disk_scan(ctx.disk_id)?;
        }
        Err(_) => {
            if let Err(rb_err) = ctx.db.rollback_disk_scan() {
                error!("Failed to rollback scan transaction: {}", rb_err);
            }
        }
    }

    result
}

/// Convert a jwalk directory entry into a `FileInsert`, or `None` if it should be skipped.
fn process_dir_entry(
    entry: &jwalk::DirEntry<((), ())>,
    mount: &Path,
    mount_path: &str,
    disk_id: i64,
) -> Option<FileInsert> {
    let entry_path = entry.path();

    if entry_path == mount {
        return None;
    }

    let path_str = entry_path.to_string_lossy();
    if let Err(e) = validate_path(&path_str) {
        error!("{}", e);
        return None;
    }

    let relative_path = entry_path.strip_prefix(mount_path).ok()?.to_string_lossy().to_string();
    let file_name = entry.file_name().to_string_lossy().to_string();

    let metadata = match entry.metadata() {
        Ok(m) => m,
        Err(err) => {
            warn!("Cannot read metadata for {}: {}", path_str, err);
            return None;
        }
    };

    let is_directory = metadata.is_dir();
    let size_bytes = if is_directory { 0 } else { metadata.len() };

    let parent_path = Path::new(&relative_path).parent().map(|p| {
        let s = p.to_string_lossy().to_string();
        if s.is_empty() { ".".to_string() } else { s }
    });

    let mtime = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);

    Some(FileInsert {
        disk_id,
        file_path: relative_path,
        file_name,
        size_bytes,
        is_directory,
        parent_path,
        mtime,
    })
}

fn run_walk(ctx: &ScanContext<'_>, disk_name: &str) -> Result<ScanStats> {
    let mut files_scanned = 0u64;
    let mut bytes_cataloged = 0u64;
    let start = Instant::now();
    let mut last_progress = Instant::now();
    let mount = Path::new(ctx.mount_path);

    let mut batch: Vec<FileInsert> = Vec::with_capacity(INSERT_BATCH_SIZE);

    let parallelism = if ctx.num_threads > 1 {
        Parallelism::RayonNewPool(ctx.num_threads)
    } else {
        Parallelism::Serial
    };

    let walker = WalkDir::new(ctx.mount_path).parallelism(parallelism).skip_hidden(false);

    for entry_result in walker {
        if ctx.cancel.is_cancelled() {
            info!("Scan cancelled for {}", ctx.mount_path);
            bail!("Scan cancelled");
        }

        let entry = match entry_result {
            Ok(e) => e,
            Err(err) => {
                warn!("Error reading directory entry: {}", err);
                continue;
            }
        };

        let Some(insert) = process_dir_entry(&entry, mount, ctx.mount_path, ctx.disk_id) else {
            continue;
        };

        if !insert.is_directory {
            files_scanned += 1;
            bytes_cataloged += insert.size_bytes;
        }

        batch.push(insert);

        if batch.len() >= INSERT_BATCH_SIZE {
            ctx.db.insert_files_batch(&batch)?;
            batch.clear();
        }

        if last_progress.elapsed().as_millis() >= u128::from(PROGRESS_INTERVAL_MS) {
            let _ = ctx.event_hub.publish(Event::ScanProgress {
                disk: disk_name.to_string(),
                files_scanned,
                bytes_cataloged,
                percent: 0.0,
            });
            last_progress = Instant::now();
        }
    }

    if !batch.is_empty() {
        ctx.db.insert_files_batch(&batch)?;
    }

    let duration = start.elapsed().as_secs_f64();

    info!(
        "Scan complete for {}: {} files, {} bytes in {:.1}s",
        ctx.mount_path, files_scanned, bytes_cataloged, duration
    );

    let _ = ctx.event_hub.publish(Event::ScanDiskComplete {
        disk: disk_name.to_string(),
        total_files: files_scanned,
        total_bytes: bytes_cataloged,
    });

    Ok(ScanStats { files_scanned, bytes_cataloged })
}
