use crate::api::responses::{ApiResponse, PlanRequest, PlanSummary};
use crate::db::PlanStatus;
use crate::{AppState, DaemonState, DaemonStatus};
use axum::{extract::State, response::IntoResponse, Json};
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

    *state.status.write().await = DaemonStatus::planning();

    let result = crate::balancer::generate_plan(
        &state.db,
        alpha,
        state.config.max_tolerance,
        state.config.min_free_headroom,
        &[],
    );

    *state.status.write().await = DaemonStatus::idle();

    match result {
        Ok(balance_result) => {
            let moves = match state.db.get_plan_moves(balance_result.plan_id) {
                Ok(m) => m,
                Err(e) => {
                    return Json(ApiResponse::<PlanSummary>::err(format!(
                        "Failed to fetch plan moves: {e}"
                    )));
                }
            };
            let plan = match state.db.get_plan(balance_result.plan_id) {
                Ok(p) => p,
                Err(e) => {
                    return Json(ApiResponse::<PlanSummary>::err(format!(
                        "Failed to fetch plan: {e}"
                    )));
                }
            };

            // Publish PlanReady only after confirming both DB reads succeeded
            let _ = state.event_hub.publish(crate::events::Event::PlanReady {
                plan_id: balance_result.plan_id,
                total_moves: moves.len() as u32,
                total_bytes: balance_result.total_bytes,
                projected_imbalance: balance_result.projected_imbalance,
            });

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

