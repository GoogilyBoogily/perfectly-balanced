use crate::api::responses::ApiResponse;
use crate::db::{MoveStatus, PlanStatus};
use crate::events::EventHub;
use crate::{AppState, DaemonState, DaemonStatus};
use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use futures::FutureExt;
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, LazyLock};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

/// Pre-compiled regex for parsing rsync `--info=progress2` output.
#[allow(clippy::unwrap_used)] // Compile-time constant regex, provably valid
static PROGRESS_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(\d+)%\s+([\d.]+\w+/s)?\s*([\d:]+)?").unwrap());

/// All the context needed to execute a single rsync file move.
struct RsyncJob<'a> {
    move_id: i64,
    file_path: &'a str,
    source_mount: &'a str,
    target_mount: &'a str,
    file_size: u64,
    use_progress2: bool,
    event_hub: &'a EventHub,
    cancel: &'a CancellationToken,
    rsync_child_slot: &'a tokio::sync::Mutex<Option<tokio::process::Child>>,
}

pub(crate) async fn execute_plan(
    State(state): State<Arc<AppState>>,
    Path(plan_id): Path<i64>,
) -> impl IntoResponse {
    // Validate plan exists and is executable (before acquiring status lock)
    match state.db.get_plan(plan_id) {
        Ok(Some(plan)) if plan.status == PlanStatus::Planned => {}
        Ok(Some(plan)) => {
            return Json(ApiResponse::<&str>::err(format!(
                "Plan is in '{}' status, can only execute 'planned' plans",
                plan.status
            )));
        }
        Ok(None) => {
            return Json(ApiResponse::<&str>::err("Plan not found"));
        }
        Err(e) => {
            return Json(ApiResponse::<&str>::err(format!("{e}")));
        }
    }

    // Check parity (before acquiring status lock)
    if state.config.warn_parity_check {
        match crate::executor::is_parity_check_running().await {
            Ok(true) => {
                return Json(ApiResponse::<&str>::err(
                    "A parity check is currently running. \
                     Stop it first or disable the warning in settings.",
                ));
            }
            Ok(false) => {} // no parity check, proceed
            Err(e) => {
                tracing::warn!("Cannot determine parity status: {}", e);
                // On non-Linux systems (dev), /proc/mdstat won't exist — allow proceeding
            }
        }
    }

    // Atomically check idle and transition to executing
    {
        let mut status = state.status.write().await;
        if status.state != DaemonState::Idle {
            return Json(ApiResponse::<&str>::err(format!(
                "Cannot execute: daemon is currently {:?}",
                status.state
            )));
        }
        *status = DaemonStatus::executing("Starting plan execution...");
    }

    let token = state.new_operation_token().await;

    let state_clone = Arc::clone(&state);
    let handle = tokio::spawn(async move {
        let result = AssertUnwindSafe(async {
            match process_plan_moves(&state_clone, plan_id, &token).await {
                Ok(()) => {
                    info!("Plan {} execution task completed", plan_id);
                }
                Err(e) => {
                    error!("Plan {} execution failed: {}", plan_id, e);
                    let _ = state_clone.event_hub.publish(crate::events::Event::DaemonError {
                        message: format!("Execution failed: {e}"),
                    });
                }
            }
        })
        .catch_unwind()
        .await;

        if result.is_err() {
            error!("Plan {} execution panicked!", plan_id);
            // Best-effort panic recovery
            let _ = state_clone.db.update_plan_status(plan_id, PlanStatus::Failed);
            let _ = state_clone.db.fail_in_progress_moves(plan_id);
            let _ = state_clone.event_hub.publish(crate::events::Event::DaemonError {
                message: format!("Execution panicked for plan {plan_id}"),
            });
        }

        // ALWAYS reset to idle — both normal and panic paths
        *state_clone.status.write().await = DaemonStatus::idle();
        *state_clone.background_task.lock().await = None;
    });

    *state.background_task.lock().await = Some(handle);

    Json(ApiResponse::ok("Execution started"))
}

async fn process_plan_moves(
    state: &Arc<AppState>,
    plan_id: i64,
    cancel: &CancellationToken,
) -> anyhow::Result<()> {
    let start = std::time::Instant::now();

    let disks = state.db.get_all_disks()?;
    let disk_map: std::collections::HashMap<i64, String> =
        disks.iter().map(|d| (d.id, d.mount_path.clone())).collect();

    state.db.update_plan_status(plan_id, PlanStatus::Executing)?;

    let use_progress2 = crate::executor::rsync_supports_progress2().await;
    let max_phase = state.db.get_max_phase(plan_id)?;

    let mut completed = 0u32;
    let mut failed = 0u32;
    let mut skipped = 0u32;

    for phase in 1..=max_phase {
        if cancel.is_cancelled() {
            state.db.update_plan_status(plan_id, PlanStatus::Cancelled)?;
            return Ok(());
        }

        let moves = state.db.get_pending_moves_for_phase(plan_id, phase)?;

        for move_detail in &moves {
            if cancel.is_cancelled() {
                break;
            }

            let m = &move_detail.move_info;
            let source_mount = if let Some(p) = disk_map.get(&m.source_disk_id) {
                p.clone()
            } else {
                state.db.update_move_status(
                    m.id,
                    MoveStatus::Failed,
                    Some("Unknown source disk"),
                )?;
                failed += 1;
                continue;
            };
            let target_mount = if let Some(p) = disk_map.get(&m.target_disk_id) {
                p.clone()
            } else {
                state.db.update_move_status(
                    m.id,
                    MoveStatus::Failed,
                    Some("Unknown target disk"),
                )?;
                failed += 1;
                continue;
            };

            let source_full = format!("{}/{}", source_mount, m.file_path);

            if !std::path::Path::new(&source_full).exists() {
                state.db.update_move_status(
                    m.id,
                    MoveStatus::Skipped,
                    Some("Source file not found"),
                )?;
                skipped += 1;
                continue;
            }

            // Fix 5: Pre-move file size validation
            match tokio::fs::metadata(&source_full).await {
                Ok(meta) => {
                    let current_size = meta.len();
                    if current_size != m.file_size {
                        let msg = format!(
                            "File size changed since planning (expected {}, now {})",
                            m.file_size, current_size
                        );
                        tracing::warn!("Skipping move {}: {}", m.id, msg);
                        state.db.update_move_status(m.id, MoveStatus::Skipped, Some(&msg))?;
                        skipped += 1;
                        let _ = state.event_hub.publish(crate::events::Event::MoveComplete {
                            move_id: m.id,
                            status: "skipped".to_string(),
                            verified: false,
                            error: Some(msg),
                        });
                        continue;
                    }
                }
                Err(e) => {
                    let msg = format!("Failed to stat source file: {e}");
                    state.db.update_move_status(m.id, MoveStatus::Failed, Some(&msg))?;
                    failed += 1;
                    continue;
                }
            }

            // Fix 4: Pre-move free space check
            {
                let target_mount_path = &target_mount;
                match crate::scanner::get_disk_space(target_mount_path) {
                    Ok(space) => {
                        let required = m.file_size.saturating_add(state.config.min_free_headroom);
                        if space.free < required {
                            let msg = format!(
                                "Insufficient space on target disk (need {} bytes, have {} free)",
                                required, space.free
                            );
                            tracing::warn!("Skipping move {}: {}", m.id, msg);
                            state.db.update_move_status(
                                m.id,
                                MoveStatus::Skipped,
                                Some(&msg),
                            )?;
                            skipped += 1;
                            let _ =
                                state.event_hub.publish(crate::events::Event::MoveComplete {
                                    move_id: m.id,
                                    status: "skipped".to_string(),
                                    verified: false,
                                    error: Some(msg),
                                });
                            continue;
                        }
                    }
                    Err(e) => {
                        let msg = format!("Failed to check target disk space: {e}");
                        state.db.update_move_status(m.id, MoveStatus::Failed, Some(&msg))?;
                        failed += 1;
                        continue;
                    }
                }
            }

            match crate::executor::is_file_open(&source_full).await {
                Ok(true) => {
                    tracing::warn!("File is open, skipping: {}", source_full);
                    state.db.update_move_status(
                        m.id,
                        MoveStatus::Skipped,
                        Some("File is currently open"),
                    )?;
                    skipped += 1;
                    let _ = state.event_hub.publish(crate::events::Event::MoveComplete {
                        move_id: m.id,
                        status: "skipped".to_string(),
                        verified: false,
                        error: Some("File is currently open".to_string()),
                    });
                    continue;
                }
                Ok(false) => {} // file not open, proceed
                Err(e) => {
                    tracing::error!("Cannot verify file safety: {}", e);
                    state.db.update_move_status(
                        m.id,
                        MoveStatus::Failed,
                        Some(&format!("Cannot verify file safety: {e}")),
                    )?;
                    failed += 1;
                    let _ = state.event_hub.publish(crate::events::Event::MoveComplete {
                        move_id: m.id,
                        status: "failed".to_string(),
                        verified: false,
                        error: Some(format!("Cannot verify file safety: {e}")),
                    });
                    continue;
                }
            }

            state.db.update_move_status(m.id, MoveStatus::InProgress, None)?;

            *state.status.write().await = DaemonStatus::executing(format!(
                "Moving {} ({}/{})",
                m.file_path,
                completed + failed + skipped + 1,
                moves.len()
            ));

            let job = RsyncJob {
                move_id: m.id,
                file_path: &m.file_path,
                source_mount: &source_mount,
                target_mount: &target_mount,
                file_size: m.file_size,
                use_progress2,
                event_hub: &state.event_hub,
                cancel,
                rsync_child_slot: &state.rsync_child,
            };

            match execute_single_rsync(&job).await {
                Ok(()) => {
                    state.db.update_move_status(m.id, MoveStatus::Completed, None)?;
                    completed += 1;
                    let _ = state.event_hub.publish(crate::events::Event::MoveComplete {
                        move_id: m.id,
                        status: "success".to_string(),
                        verified: true,
                        error: None,
                    });
                }
                Err(_e) if cancel.is_cancelled() => {
                    state.db.update_move_status(m.id, MoveStatus::Pending, None)?;
                }
                Err(e) => {
                    let msg = format!("{e:#}");
                    state.db.update_move_status(m.id, MoveStatus::Failed, Some(&msg))?;
                    failed += 1;
                    let _ = state.event_hub.publish(crate::events::Event::MoveComplete {
                        move_id: m.id,
                        status: "failed".to_string(),
                        verified: false,
                        error: Some(msg.clone()),
                    });
                }
            }
        }
    }

    let duration = start.elapsed().as_secs_f64();
    let status = if cancel.is_cancelled() { PlanStatus::Cancelled } else { PlanStatus::Completed };
    state.db.update_plan_status(plan_id, status)?;

    let _ = state.event_hub.publish(crate::events::Event::ExecutionComplete {
        plan_id,
        moves_completed: completed,
        moves_failed: failed,
        moves_skipped: skipped,
        duration_seconds: duration,
    });

    Ok(())
}

async fn execute_single_rsync(job: &RsyncJob<'_>) -> anyhow::Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt};
    const STDERR_CAP: usize = 64 * 1024;

    let source = format!("{}/{}", job.source_mount, job.file_path);
    let target = format!("{}/{}", job.target_mount, job.file_path);

    crate::scanner::validation::validate_path(&source)?;
    crate::scanner::validation::validate_path(&target)?;

    if let Some(parent) = std::path::Path::new(&target).parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    // Record source mtime before rsync starts (for post-copy verification)
    let pre_rsync_mtime = tokio::fs::metadata(&source)
        .await?
        .modified()?;

    // Two-phase move: copy only (no --remove-source-files)
    let mut args = vec!["-avPX"];
    if job.use_progress2 {
        args.push("--info=progress2");
    }
    args.push(&source);
    args.push(&target);

    let mut rsync_proc = tokio::process::Command::new("rsync")
        .args(&args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    let stdout = rsync_proc.stdout.take();
    let stderr = rsync_proc.stderr.take();

    // Store child in the shared slot so shutdown can kill it
    *job.rsync_child_slot.lock().await = Some(rsync_proc);

    // Drain stderr in background to prevent pipe buffer deadlock.
    let stderr_task = tokio::spawn(async move {
        if let Some(mut stderr) = stderr {
            let mut buf = String::new();
            match stderr.read_to_string(&mut buf).await {
                Ok(n) if n > STDERR_CAP => buf.truncate(STDERR_CAP),
                _ => {}
            }
            buf
        } else {
            String::new()
        }
    });

    if let Some(stdout) = stdout {
        let reader = tokio::io::BufReader::new(stdout);
        let mut lines = reader.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            if job.cancel.is_cancelled() {
                let child = job.rsync_child_slot.lock().await.take();
                if let Some(mut child) = child {
                    child.kill().await.ok();
                    child.wait().await.ok();
                }
                stderr_task.abort();
                cleanup_target(&target).await;
                anyhow::bail!("rsync cancelled during execution");
            }
            if let Some(caps) = PROGRESS_RE.captures(&line) {
                let pct: f64 = caps[1].parse().unwrap_or(0.0);
                let speed = caps.get(2).map(|m| m.as_str().to_string()).unwrap_or_default();
                let eta = caps.get(3).map(|m| m.as_str().to_string()).unwrap_or_default();
                let _ = job.event_hub.publish(crate::events::Event::MoveProgress {
                    move_id: job.move_id,
                    file_path: job.file_path.to_string(),
                    percent: pct,
                    speed,
                    eta,
                });
            }
        }
    }

    // Cancel check after stdout loop exits: if shutdown killed rsync while we were
    // reading the final bytes, handle it here instead of falling into the wrong branch.
    if job.cancel.is_cancelled() {
        let child = job.rsync_child_slot.lock().await.take();
        if let Some(mut child) = child {
            child.kill().await.ok();
            child.wait().await.ok();
        }
        stderr_task.abort();
        cleanup_target(&target).await;
        anyhow::bail!("rsync cancelled during execution");
    }

    // Take child back from slot and wait for it
    let child = job.rsync_child_slot.lock().await.take();
    let stderr_output = stderr_task.await.unwrap_or_default();
    if let Some(mut child) = child {
        let exit = child.wait().await?;
        if exit.success() {
            // Cancel guard: if cancellation arrived between rsync completing and now,
            // clean up target instead of proceeding to delete the source.
            if job.cancel.is_cancelled() {
                cleanup_target(&target).await;
                anyhow::bail!("cancelled after rsync completed");
            }
            // Phase 2: Verify copy and remove source
            verify_and_remove_source(&source, &target, job.file_size, pre_rsync_mtime).await
        } else {
            let code = exit.code().unwrap_or(-1);
            let stderr_summary = if stderr_output.is_empty() {
                String::new()
            } else {
                format!(": {}", stderr_output.lines().last().unwrap_or(""))
            };
            cleanup_target(&target).await;
            anyhow::bail!("rsync exited with code {code}{stderr_summary}")
        }
    } else {
        cleanup_target(&target).await;
        anyhow::bail!("rsync process was killed during shutdown")
    }
}

/// Verify the target copy is correct, then remove the source.
///
/// Safety invariant: the source file is NEVER deleted unless:
/// 1. The target exists and matches the expected size
/// 2. The source mtime hasn't changed since rsync started (no concurrent modification)
async fn verify_and_remove_source(
    source: &str,
    target: &str,
    expected_size: u64,
    pre_rsync_mtime: std::time::SystemTime,
) -> anyhow::Result<()> {
    // Verify target exists and size matches
    let target_meta = tokio::fs::metadata(target).await.map_err(|e| {
        anyhow::anyhow!("Post-copy verification failed: target file missing or unreadable: {e}")
    })?;
    let target_size = target_meta.len();
    if target_size != expected_size {
        anyhow::bail!(
            "Post-copy verification failed: target size {target_size} != expected {expected_size} \
             (both copies preserved)"
        );
    }

    // Verify source hasn't been modified during the transfer
    let source_meta = tokio::fs::metadata(source).await.map_err(|e| {
        anyhow::anyhow!("Post-copy verification failed: cannot stat source: {e}")
    })?;
    let current_mtime = source_meta.modified()?;
    if current_mtime != pre_rsync_mtime {
        anyhow::bail!(
            "Post-copy verification failed: source was modified during transfer \
             (both copies preserved)"
        );
    }

    // All checks passed — safe to delete source
    tokio::fs::remove_file(source).await.map_err(|e| {
        anyhow::anyhow!(
            "Copy verified but failed to remove source (both copies exist, manual cleanup needed): {e}"
        )
    })?;

    Ok(())
}

/// Best-effort cleanup of a target file and any empty parent directories.
/// Used after rsync failure, cancellation, or shutdown kill.
async fn cleanup_target(target: &str) {
    let path = std::path::Path::new(target);
    if path.exists() {
        if let Err(e) = tokio::fs::remove_file(path).await {
            tracing::warn!("Failed to clean up target file {target}: {e}");
        } else {
            tracing::info!("Cleaned up partial target: {target}");
        }
    }
    crate::executor::recovery::cleanup_empty_parents(target).await;
}

pub(crate) async fn cancel_operation(
    State(state): State<Arc<AppState>>,
    Path(plan_id): Path<i64>,
) -> impl IntoResponse {
    let status = state.status.read().await;
    if status.state == DaemonState::Idle {
        return Json(ApiResponse::<&str>::err("No operation in progress"));
    }
    drop(status);
    state.request_cancel().await;
    info!("Cancellation requested for plan {}", plan_id);
    Json(ApiResponse::ok("Cancellation requested"))
}
