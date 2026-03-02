use crate::db::{Database, MoveStatus};
use anyhow::Result;
use std::path::Path;
use tracing::{info, warn};

/// Examine the filesystem state for each recovered move and take corrective action.
///
/// Under two-phase move semantics, rsync never deletes the source — only our
/// `verify_and_remove_source()` does. Crashes can leave us in these states:
///
/// | Source | Target | Action                                                        |
/// |--------|--------|---------------------------------------------------------------|
/// | exists | exists | Size+mtime check → complete if verified, else delete target   |
/// | exists | absent | No action, move stays Pending                                 |
/// | absent | exists | Source removal succeeded → mark Completed                     |
/// | absent | absent | Data loss — mark Failed                                       |
pub(crate) async fn cleanup_partial_files(
    db: &Database,
    recovered_move_ids: &[i64],
) -> Result<()> {
    if recovered_move_ids.is_empty() {
        return Ok(());
    }

    let move_infos = db.get_moves_path_info(recovered_move_ids)?;

    let mut completed = 0usize;
    let mut cleaned = 0usize;
    let mut data_loss = 0usize;

    for m in &move_infos {
        let source = format!("{}/{}", m.source_mount, m.file_path);
        let target = format!("{}/{}", m.target_mount, m.file_path);

        let source_exists = Path::new(&source).exists();
        let target_exists = Path::new(&target).exists();

        match (source_exists, target_exists) {
            (true, true) => {
                let target_size = tokio::fs::metadata(&target)
                    .await
                    .map(|md| md.len())
                    .unwrap_or(0);

                if target_size == m.file_size {
                    // Target matches expected size — but we need to verify source mtime
                    // hasn't changed to detect files modified after the crash.
                    let source_mtime_ok = match m.source_mtime {
                        Some(planned_mtime) => {
                            // We have the planned mtime — compare against current source
                            match tokio::fs::metadata(&source).await {
                                Ok(meta) => {
                                    match meta.modified() {
                                        Ok(current) => {
                                            let current_epoch = current
                                                .duration_since(std::time::UNIX_EPOCH)
                                                .map(|d| d.as_secs() as i64)
                                                .unwrap_or(0);
                                            current_epoch == planned_mtime
                                        }
                                        Err(_) => true, // Can't read mtime, trust size match
                                    }
                                }
                                Err(_) => true, // Can't stat source, trust size match
                            }
                        }
                        None => true, // No planned mtime (old plan), trust size match
                    };

                    if source_mtime_ok {
                        // Verified: target is a complete copy and source is unmodified
                        if let Err(e) = tokio::fs::remove_file(&source).await {
                            warn!(
                                "Move {} recovered as completed but failed to remove source {}: {}",
                                m.id, source, e
                            );
                        } else {
                            info!(
                                "Move {} recovered as completed (removed source): {}",
                                m.id, m.file_path
                            );
                        }
                        db.update_move_status(m.id, MoveStatus::Completed, None)?;
                        completed += 1;
                    } else {
                        // Source was modified after planning — the target is stale.
                        // Delete target, leave move as Pending for re-execution.
                        warn!(
                            "Move {} source mtime changed — deleting stale target: {}",
                            m.id, m.file_path
                        );
                        if let Err(e) = tokio::fs::remove_file(&target).await {
                            warn!(
                                "Failed to remove stale target {} for move {}: {}",
                                target, m.id, e
                            );
                        } else {
                            cleanup_empty_parents(&target).await;
                            cleaned += 1;
                        }
                        // Move stays Pending (already reset by recover_stale_states)
                    }
                } else {
                    // Target is partial — delete it and clean up empty dirs
                    if let Err(e) = tokio::fs::remove_file(&target).await {
                        warn!(
                            "Failed to remove partial file {} for move {}: {}",
                            target, m.id, e
                        );
                    } else {
                        info!(
                            "Removed partial file ({} bytes vs expected {}): {}",
                            target_size, m.file_size, target
                        );
                        cleanup_empty_parents(&target).await;
                        cleaned += 1;
                    }
                    // Move stays Pending (already reset by recover_stale_states)
                }
            }
            (true, false) => {
                // No partial file to clean up, move stays Pending
            }
            (false, true) => {
                // Under two-phase move: source was removed by verify_and_remove_source
                // after a verified copy. The daemon crashed before updating the DB.
                db.update_move_status(m.id, MoveStatus::Completed, None)?;
                info!(
                    "Move {} recovered as completed (source gone, target present): {}",
                    m.id, m.file_path
                );
                completed += 1;
            }
            (false, false) => {
                // Both source and target are gone — data loss
                db.update_move_status(
                    m.id,
                    MoveStatus::Failed,
                    Some("Data loss: source and target both missing after crash"),
                )?;
                warn!(
                    "Move {} data loss (both source and target missing): {}",
                    m.id, m.file_path
                );
                data_loss += 1;
            }
        }
    }

    if completed > 0 || cleaned > 0 || data_loss > 0 {
        info!(
            "Partial file cleanup: {} recovered as completed, {} partial files removed, {} data loss",
            completed, cleaned, data_loss
        );
    }

    Ok(())
}

/// Walk up from a file path removing empty directories, stopping at mount point depth.
pub(crate) async fn cleanup_empty_parents(path: &str) {
    let mut current = std::path::PathBuf::from(path);
    loop {
        if !current.pop() {
            break;
        }
        // Stop at mount point depth (e.g., /mnt/disk1 = 3 components)
        if current.components().count() <= 3 {
            break;
        }
        if tokio::fs::remove_dir(&current).await.is_err() {
            break;
        }
        info!("Removed empty directory: {}", current.display());
    }
}
