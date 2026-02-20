use crate::api::responses::{ApiResponse, ScanRequest};
use crate::{scanner, AppState, DaemonState, DaemonStatus};
use axum::{extract::State, response::IntoResponse, Json};
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

pub(crate) async fn start_scan(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ScanRequest>,
) -> impl IntoResponse {
    {
        let status = state.status.read().await;
        if status.state != DaemonState::Idle {
            return Json(ApiResponse::<&str>::err(format!(
                "Cannot start scan: daemon is currently {:?}",
                status.state
            )));
        }
    }

    let threads = req.threads.unwrap_or(state.config.scan_threads);
    let token = state.new_operation_token().await;
    let state_clone = state.clone();

    let handle = tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Handle::current();

        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            rt.block_on(async {
                *state_clone.status.write().await = DaemonStatus::scanning("Discovering disks...");
            });

            let discovered = match scanner::discover_disks(&state_clone.config.mnt_base) {
                Ok(d) => d,
                Err(e) => {
                    error!("Disk discovery failed: {}", e);
                    let _ = state_clone.event_hub.publish(crate::events::Event::DaemonError {
                        message: format!("Disk discovery failed: {e}"),
                    });
                    return;
                }
            };

            info!("Discovered {} disks", discovered.len());
            scan_discovered_disks(&state_clone, &discovered, &req, threads, &rt, &token);
        }));

        if result.is_err() {
            error!("Scan task panicked!");
            let _ = state_clone.event_hub.publish(crate::events::Event::DaemonError {
                message: "Scan task panicked".to_string(),
            });
        }

        // ALWAYS reset to idle — both normal and panic paths
        rt.block_on(async {
            *state_clone.status.write().await = DaemonStatus::idle();
            *state_clone.background_task.lock().await = None;
        });
    });

    *state.background_task.lock().await = Some(handle);

    Json(ApiResponse::ok("Scan started"))
}

/// Parse /proc/mounts once into a mount_path → fs_type lookup.
fn parse_mount_table() -> HashMap<String, String> {
    let mut table = HashMap::new();
    if let Ok(mounts) = std::fs::read_to_string("/proc/mounts") {
        for line in mounts.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                table.insert(parts[1].to_string(), parts[2].to_string());
            }
        }
    }
    table
}

#[allow(clippy::too_many_arguments)]
fn scan_discovered_disks(
    state: &Arc<AppState>,
    discovered: &[scanner::DiscoveredDisk],
    req: &ScanRequest,
    threads: usize,
    rt: &tokio::runtime::Handle,
    cancel: &CancellationToken,
) {
    let mut total_files = 0u64;
    let mut total_bytes = 0u64;
    let start = std::time::Instant::now();
    let mount_table = parse_mount_table();

    for disk in discovered {
        let space = match scanner::get_disk_space(&disk.mount_path) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to get disk space for {}: {}", disk.name, e);
                continue;
            }
        };

        let fs_type = mount_table.get(&disk.mount_path).map(String::as_str);

        let disk_id = match state.db.upsert_disk(
            &disk.name,
            &disk.mount_path,
            space.total,
            space.used,
            space.free,
            fs_type,
        ) {
            Ok(id) => id,
            Err(e) => {
                error!("Failed to upsert disk {}: {}", disk.name, e);
                continue;
            }
        };

        if let Some(ref ids) = req.disk_ids {
            if !ids.contains(&disk_id) {
                continue;
            }
        }

        if state.config.excluded_disks.contains(&disk.name) {
            info!("Skipping excluded disk: {}", disk.name);
            continue;
        }

        rt.block_on(async {
            *state.status.write().await =
                DaemonStatus::scanning(format!("Scanning {}...", disk.name));
        });

        if cancel.is_cancelled() {
            info!("Scan cancelled by user");
            break;
        }

        let ctx = scanner::ScanContext {
            db: &state.db,
            disk_id,
            mount_path: &disk.mount_path,
            event_hub: &state.event_hub,
            cancel: cancel.clone(),
            num_threads: threads,
        };
        match scanner::scan_disk(&ctx) {
            Ok(stats) => {
                total_files += stats.files_scanned;
                total_bytes += stats.bytes_cataloged;
            }
            Err(e) => {
                error!("Scan failed for {}: {}", disk.name, e);
            }
        }
    }

    let duration = start.elapsed().as_secs_f64();

    let _ = state.event_hub.publish(crate::events::Event::ScanComplete {
        total_disks: discovered.len() as u32,
        total_files,
        total_bytes,
        duration_seconds: duration,
    });

    info!(
        "Full scan complete: {} disks, {} files, {} bytes in {:.1}s",
        discovered.len(),
        total_files,
        total_bytes,
        duration
    );
}
