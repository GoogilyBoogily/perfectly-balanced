mod handlers;
pub(crate) mod responses;

use crate::AppState;
use axum::{
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

/// Build the complete API router.
pub(crate) fn router(state: Arc<AppState>) -> Router {
    let cors = CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any);

    Router::new()
        // Status
        .route("/api/status", get(handlers::get_status))
        // Disks
        .route("/api/disks", get(handlers::get_disks))
        .route("/api/disks/{disk_id}/include", post(handlers::set_disk_included))
        .route("/api/disks/{disk_id}/exclude", post(handlers::set_disk_excluded))
        // Scanning
        .route("/api/scan", post(handlers::start_scan))
        // Planning
        .route("/api/plan", post(handlers::handle_generate_plan))
        .route("/api/plan/{plan_id}", get(handlers::get_plan))
        // Execution
        .route("/api/plan/{plan_id}/execute", post(handlers::execute_plan))
        .route("/api/plan/{plan_id}/cancel", post(handlers::cancel_operation))
        // Settings
        .route("/api/settings", get(handlers::get_settings))
        .route("/api/settings", post(handlers::update_settings))
        // SSE events
        .route("/api/events", get(handlers::sse_events))
        .with_state(state)
        .layer(cors)
        .layer(TraceLayer::new_for_http())
}
