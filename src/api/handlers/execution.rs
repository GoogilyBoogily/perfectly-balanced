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
static PROGRESS_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(\d+)%\s+([\d.]+\w+/s)?\s*([\d:]+)?").unwrap());

/// All the context needed to execute a single rsync file move.
struct RsyncJob<'a> {
    move_id: i64,
    file_path: &'a str,
    source_mount: &'a str,
    target_mount: &'a str,
    use_progress2: bool,
    event_hub: &'a EventHub,
    cancel: &'a CancellationToken,
    rsync_child_slot: &'a tokio::sync::Mutex<Option<tokio::process::Child>>,
}

pub(crate) async fn execute_plan(
    State(state): State<Arc<AppState>>,
    Path(plan_id): Path<i64>,
) -> impl IntoResponse {
    {
        let status = state.status.read().await;
        if status.state != DaemonState::Idle {
            return Json(ApiResponse::<&str>::err(format!(
                "Cannot execute: daemon is currently {:?}",
                status.state
            )));
        }
    }

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

    if state.config.warn_parity_check && crate::executor::is_parity_check_running().await {
        return Json(ApiResponse::<&str>::err(
            "A parity check is currently running. \
             Stop it first or disable the warning in settings.",
        ));
    }

    let token = state.new_operation_token().await;

    *state.status.write().await = DaemonStatus::executing("Starting plan execution...");

    let state_clone = state.clone();
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
                state.db.update_move_status(m.id, MoveStatus::Failed, Some("Unknown source disk"))?;
                failed += 1;
                continue;
            };
            let target_mount = if let Some(p) = disk_map.get(&m.target_disk_id) {
                p.clone()
            } else {
                state.db.update_move_status(m.id, MoveStatus::Failed, Some("Unknown target disk"))?;
                failed += 1;
                continue;
            };

            let source_full = format!("{}/{}", source_mount, m.file_path);

            if !std::path::Path::new(&source_full).exists() {
                state.db.update_move_status(m.id, MoveStatus::Skipped, Some("Source file not found"))?;
                skipped += 1;
                continue;
            }

            if crate::executor::is_file_open(&source_full).await {
                tracing::warn!("File is open, skipping: {}", source_full);
                state.db.update_move_status(m.id, MoveStatus::Skipped, Some("File is currently open"))?;
                skipped += 1;
                let _ = state.event_hub.publish(crate::events::Event::MoveComplete {
                    move_id: m.id,
                    status: "skipped".to_string(),
                    verified: false,
                    error: Some("File is currently open".to_string()),
                });
                continue;
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
                use_progress2,
                event_hub: &state.event_hub,
                cancel,
                rsync_child_slot: &state.rsync_child,
            };

            match execute_single_rsync(&job).await {
                Ok(true) => {
                    state.db.update_move_status(m.id, MoveStatus::Completed, None)?;
                    completed += 1;
                    let _ = state.event_hub.publish(crate::events::Event::MoveComplete {
                        move_id: m.id,
                        status: "success".to_string(),
                        verified: true,
                        error: None,
                    });
                }
                Ok(false) => {
                    if cancel.is_cancelled() {
                        state.db.update_move_status(m.id, MoveStatus::Pending, None)?;
                    } else {
                        state.db.update_move_status(m.id, MoveStatus::Failed, Some("rsync failed"))?;
                        failed += 1;
                    }
                }
                Err(e) => {
                    let msg = format!("{e:#}");
                    state.db.update_move_status(m.id, MoveStatus::Failed, Some(&msg))?;
                    failed += 1;
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

async fn execute_single_rsync(job: &RsyncJob<'_>) -> anyhow::Result<bool> {
    use tokio::io::AsyncBufReadExt;

    let source = format!("{}/{}", job.source_mount, job.file_path);
    let target = format!("{}/{}", job.target_mount, job.file_path);

    if source.contains("/mnt/user/") || target.contains("/mnt/user/") {
        anyhow::bail!("SAFETY: Cannot operate on FUSE paths");
    }

    if let Some(parent) = std::path::Path::new(&target).parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let mut args = vec!["-avPX", "--remove-source-files"];
    if job.use_progress2 {
        args.push("--info=progress2");
    }
    args.push(&source);
    args.push(&target);

    let mut child = tokio::process::Command::new("rsync")
        .args(&args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.take();

    // Store child in the shared slot so shutdown can kill it
    *job.rsync_child_slot.lock().await = Some(child);

    if let Some(stdout) = stdout {
        let reader = tokio::io::BufReader::new(stdout);
        let mut lines = reader.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            if job.cancel.is_cancelled() {
                // Kill the child via the slot
                let child = job.rsync_child_slot.lock().await.take();
                if let Some(mut child) = child {
                    child.kill().await.ok();
                    child.wait().await.ok();
                }
                return Ok(false);
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

    // Take child back from slot and wait for it
    let child = job.rsync_child_slot.lock().await.take();
    if let Some(mut child) = child {
        let exit = child.wait().await?;
        Ok(exit.success())
    } else {
        // Child was already killed by shutdown — treat as cancelled
        Ok(false)
    }
}

pub(crate) async fn cancel_operation(
    State(state): State<Arc<AppState>>,
    Path(_plan_id): Path<i64>,
) -> impl IntoResponse {
    state.request_cancel().await;
    info!("Cancellation requested");
    Json(ApiResponse::ok("Cancellation requested"))
}
