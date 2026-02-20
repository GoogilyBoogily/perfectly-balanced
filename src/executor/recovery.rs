use crate::db::{Database, MoveStatus};
use anyhow::Result;
use std::path::Path;
use tracing::{info, warn};

/// Stats returned by partial file cleanup.
#[allow(dead_code)]
pub(crate) struct CleanupStats {
    pub completed: usize,
    pub cleaned: usize,
    pub data_loss: usize,
}

/// Examine the filesystem state for each recovered move and take corrective action.
///
/// Decision matrix (based on `--remove-source-files` semantics):
///
/// | Source | Target | Action                                                  |
/// |--------|--------|---------------------------------------------------------|
/// | exists | exists | Delete target (partial), leave move as Pending          |
/// | exists | absent | No action, move stays Pending                           |
/// | absent | exists | rsync completed successfully — mark Completed           |
/// | absent | absent | Data loss — mark Failed                                 |
pub(crate) async fn cleanup_partial_files(
    db: &Database,
    recovered_move_ids: &[i64],
) -> Result<CleanupStats> {
    if recovered_move_ids.is_empty() {
        return Ok(CleanupStats { completed: 0, cleaned: 0, data_loss: 0 });
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
                // Target is a partial file from interrupted rsync — delete it
                if let Err(e) = tokio::fs::remove_file(&target).await {
                    warn!(
                        "Failed to remove partial file {} for move {}: {}",
                        target, m.id, e
                    );
                } else {
                    info!("Removed partial file: {}", target);
                    cleaned += 1;
                }
                // Move stays Pending (already reset by recover_stale_states)
            }
            (true, false) => {
                // No partial file to clean up, move stays Pending
            }
            (false, true) => {
                // rsync completed the transfer (source was removed after verified copy)
                // but the daemon crashed before updating the DB
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

    Ok(CleanupStats { completed, cleaned, data_loss })
}
