use crate::api::responses::{ApiResponse, StatusResponse};
use crate::AppState;
use axum::{extract::State, response::IntoResponse, Json};
use std::sync::Arc;

pub(crate) async fn get_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let status = state.status.read().await;
    Json(ApiResponse::ok(StatusResponse {
        state: status.state,
        detail: status.detail.clone(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    }))
}
