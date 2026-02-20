use crate::api::responses::{ApiResponse, PlanRequest, PlanSummary};
use crate::db::PlanStatus;
use crate::{AppState, DaemonState, DaemonStatus};
use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use std::sync::Arc;

pub(crate) async fn handle_generate_plan(
    State(state): State<Arc<AppState>>,
    Json(req): Json<PlanRequest>,
) -> impl IntoResponse {
    {
        let status = state.status.read().await;
        if status.state != DaemonState::Idle {
            return Json(ApiResponse::<PlanSummary>::err(format!(
                "Cannot generate plan: daemon is currently {:?}",
                status.state
            )));
        }
    }

    let alpha = req.alpha.unwrap_or(state.config.slider_alpha);
    let excluded = req.excluded_disks.unwrap_or_default();

    *state.status.write().await = DaemonStatus::planning();

    let result = crate::balancer::generate_plan(
        &state.db,
        alpha,
        state.config.max_tolerance,
        state.config.min_free_headroom,
        &excluded,
    );

    *state.status.write().await = DaemonStatus::idle();

    match result {
        Ok(balance_result) => {
            let _ = state.event_hub.publish(crate::events::Event::PlanReady {
                plan_id: balance_result.plan_id,
                total_moves: balance_result.total_moves as u32,
                total_bytes: balance_result.total_bytes,
                projected_imbalance: balance_result.projected_imbalance,
            });

            let moves = state.db.get_plan_moves(balance_result.plan_id).unwrap_or_default();
            let plan = state.db.get_plan(balance_result.plan_id).ok().flatten();

            Json(ApiResponse::ok(PlanSummary {
                id: balance_result.plan_id,
                created_at: plan.as_ref().and_then(|p| p.created_at.clone()),
                tolerance: plan.as_ref().map_or(0.0, |p| p.tolerance),
                slider_alpha: alpha,
                target_utilization: balance_result.target_utilization,
                initial_imbalance: Some(balance_result.initial_imbalance),
                projected_imbalance: Some(balance_result.projected_imbalance),
                total_moves: balance_result.total_moves as i32,
                total_bytes_to_move: balance_result.total_bytes,
                status: PlanStatus::Planned,
                moves,
            }))
        }
        Err(e) => Json(ApiResponse::<PlanSummary>::err(format!("Planning failed: {e}"))),
    }
}

pub(crate) async fn get_plan(
    State(state): State<Arc<AppState>>,
    Path(plan_id): Path<i64>,
) -> impl IntoResponse {
    let plan = match state.db.get_plan(plan_id) {
        Ok(Some(p)) => p,
        Ok(None) => {
            return Json(ApiResponse::<PlanSummary>::err("Plan not found"));
        }
        Err(e) => {
            return Json(ApiResponse::<PlanSummary>::err(format!("{e}")));
        }
    };

    let moves = state.db.get_plan_moves(plan_id).unwrap_or_default();

    Json(ApiResponse::ok(PlanSummary {
        id: plan.id,
        created_at: plan.created_at,
        tolerance: plan.tolerance,
        slider_alpha: plan.slider_alpha,
        target_utilization: plan.target_utilization,
        initial_imbalance: plan.initial_imbalance,
        projected_imbalance: plan.projected_imbalance,
        total_moves: plan.total_moves,
        total_bytes_to_move: plan.total_bytes_to_move,
        status: plan.status,
        moves,
    }))
}
