use super::types::{BalanceResult, DiskClass, DiskState};
use crate::db::{Database, Disk, FileEntry, MoveStatus, PlannedMove};
use anyhow::{bail, Result};
use std::collections::HashMap;
use tracing::info;

/// Shared context for the move assignment phase.
struct PlanContext {
    plan_id: i64,
    target_utilization: f64,
    effective_tolerance: f64,
    min_free_headroom: u64,
    disk_idx: HashMap<i64, usize>,
}

/// Compute the maximum deviation from target utilization across all disks.
fn max_imbalance(disk_states: &[DiskState], target: f64) -> f64 {
    disk_states.iter().map(|ds| (ds.sim_utilization() - target).abs()).fold(0.0_f64, f64::max)
}

/// Check if all disks are within tolerance of the target utilization.
fn is_balanced(disk_states: &[DiskState], target: f64, tolerance: f64) -> bool {
    disk_states.iter().all(|ds| (ds.sim_utilization() - target).abs() <= tolerance)
}

/// Generate a balance plan.
///
/// `slider_alpha` ranges from 0.0 (fewest moves / high tolerance) to 1.0 (perfect balance).
/// `max_tolerance` is the maximum tolerance (e.g., 0.15 for 15%).
/// `min_free_headroom` is the minimum bytes to leave free on any disk.
pub(crate) fn generate_plan(
    db: &Database,
    slider_alpha: f64,
    max_tolerance: f64,
    min_free_headroom: u64,
    excluded_disk_ids: &[i64],
) -> Result<BalanceResult> {
    let all_disks = db.get_all_disks()?;
    let disks: Vec<Disk> = all_disks
        .into_iter()
        .filter(|d| d.included && !excluded_disk_ids.contains(&d.id))
        .collect();

    if disks.len() < 2 {
        bail!("Need at least 2 included disks to balance");
    }

    let total_used: u64 = disks.iter().map(|d| d.used_bytes).sum();
    let total_capacity: u64 = disks.iter().map(|d| d.total_bytes).sum();

    if total_capacity == 0 {
        bail!("Total disk capacity is zero");
    }

    let target_utilization = total_used as f64 / total_capacity as f64;
    let effective_tolerance = max_tolerance * (1.0 - slider_alpha);

    info!(
        "Balance planning: target_utilization={:.2}%, tolerance={:.2}%, alpha={:.2}",
        target_utilization * 100.0,
        effective_tolerance * 100.0,
        slider_alpha
    );

    let mut disk_states = classify_disks(&disks, target_utilization, effective_tolerance);
    let initial_imbalance = max_imbalance(&disk_states, target_utilization);

    let has_outer = disk_states
        .iter()
        .any(|ds| ds.class == DiskClass::OverUtilized || ds.class == DiskClass::UnderUtilized);

    if !has_outer {
        info!("Array is already balanced within tolerance");
        let plan_id = db.create_plan(
            effective_tolerance,
            slider_alpha,
            target_utilization,
            initial_imbalance,
        )?;
        db.update_plan_projections(plan_id, initial_imbalance, 0, 0)?;

        return Ok(BalanceResult {
            plan_id,
            target_utilization,
            initial_imbalance,
            projected_imbalance: initial_imbalance,
            total_moves: 0,
            total_bytes: 0,
        });
    }

    let plan_id =
        db.create_plan(effective_tolerance, slider_alpha, target_utilization, initial_imbalance)?;

    let candidate_files = collect_candidates(db, &disk_states)?;

    let plan_ctx = PlanContext {
        plan_id,
        target_utilization,
        effective_tolerance,
        min_free_headroom,
        disk_idx: disk_states.iter().enumerate().map(|(i, ds)| (ds.disk.id, i)).collect(),
    };

    let (planned_moves, total_bytes_to_move) =
        assign_moves(&plan_ctx, &candidate_files, &mut disk_states);

    let projected_imbalance = max_imbalance(&disk_states, target_utilization);

    info!(
        "Plan generated: {} moves, {} bytes, imbalance {:.2}% -> {:.2}%",
        planned_moves.len(),
        total_bytes_to_move,
        initial_imbalance * 100.0,
        projected_imbalance * 100.0,
    );

    if !planned_moves.is_empty() {
        db.insert_planned_moves(&planned_moves)?;
    }

    db.update_plan_projections(
        plan_id,
        projected_imbalance,
        planned_moves.len() as i32,
        total_bytes_to_move,
    )?;

    Ok(BalanceResult {
        plan_id,
        target_utilization,
        initial_imbalance,
        projected_imbalance,
        total_moves: planned_moves.len(),
        total_bytes: total_bytes_to_move,
    })
}

fn classify_disks(
    disks: &[Disk],
    target_utilization: f64,
    effective_tolerance: f64,
) -> Vec<DiskState> {
    disks
        .iter()
        .map(|d| {
            let utilization = d.utilization();

            let class = if utilization > target_utilization + effective_tolerance {
                DiskClass::OverUtilized
            } else if utilization > target_utilization {
                DiskClass::AboveAverage
            } else if utilization < target_utilization - effective_tolerance {
                DiskClass::UnderUtilized
            } else {
                DiskClass::BelowAverage
            };

            DiskState { disk: d.clone(), class, sim_used: d.used_bytes }
        })
        .collect()
}

fn collect_candidates(db: &Database, disk_states: &[DiskState]) -> Result<Vec<FileEntry>> {
    let over_disk_ids: Vec<i64> = disk_states
        .iter()
        .filter(|ds| ds.class == DiskClass::OverUtilized || ds.class == DiskClass::AboveAverage)
        .map(|ds| ds.disk.id)
        .collect();

    let mut candidate_files: Vec<FileEntry> = Vec::new();
    for disk_id in &over_disk_ids {
        let files = db.get_all_files_on_disk_by_size(*disk_id)?;
        candidate_files.extend(files);
    }

    candidate_files.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
    Ok(candidate_files)
}

fn assign_moves(
    ctx: &PlanContext,
    candidate_files: &[FileEntry],
    disk_states: &mut [DiskState],
) -> (Vec<PlannedMove>, u64) {
    let mut planned_moves: Vec<PlannedMove> = Vec::new();
    let mut total_bytes_to_move: u64 = 0;
    let mut move_order: i32 = 0;

    for file in candidate_files {
        let Some(&src_idx) = ctx.disk_idx.get(&file.disk_id) else {
            continue;
        };

        let src_util = disk_states[src_idx].sim_utilization();
        if src_util <= ctx.target_utilization + ctx.effective_tolerance {
            continue;
        }

        let best_target =
            find_best_target(disk_states, file, ctx.target_utilization, ctx.min_free_headroom);

        if let Some(tgt_idx) = best_target {
            move_order += 1;
            let target_disk_id = disk_states[tgt_idx].disk.id;

            planned_moves.push(PlannedMove {
                id: 0,
                plan_id: ctx.plan_id,
                file_id: file.id,
                source_disk_id: file.disk_id,
                target_disk_id,
                file_path: file.file_path.clone(),
                file_size: file.size_bytes,
                move_order,
                phase: 1,
                status: MoveStatus::Pending,
                error_message: None,
            });

            disk_states[src_idx].sim_used =
                disk_states[src_idx].sim_used.saturating_sub(file.size_bytes);
            disk_states[tgt_idx].sim_used += file.size_bytes;
            total_bytes_to_move += file.size_bytes;
        }

        if is_balanced(disk_states, ctx.target_utilization, ctx.effective_tolerance) {
            info!("All disks within tolerance after {} moves", planned_moves.len());
            break;
        }
    }

    (planned_moves, total_bytes_to_move)
}

fn find_best_target(
    disk_states: &[DiskState],
    file: &FileEntry,
    target_utilization: f64,
    min_free_headroom: u64,
) -> Option<usize> {
    let mut best_target: Option<usize> = None;
    let mut best_remaining: i64 = i64::MIN;

    for (i, ds) in disk_states.iter().enumerate() {
        if ds.disk.id == file.disk_id {
            continue;
        }

        if ds.sim_utilization() >= target_utilization {
            continue;
        }

        let available = ds.sim_free().saturating_sub(min_free_headroom);
        if available < file.size_bytes {
            continue;
        }

        let target_used = (target_utilization * ds.disk.total_bytes as f64) as u64;
        let remaining = target_used as i64 - ds.sim_used as i64;

        if remaining > best_remaining {
            best_remaining = remaining;
            best_target = Some(i);
        }
    }

    best_target
}
