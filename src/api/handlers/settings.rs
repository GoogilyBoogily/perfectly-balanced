use crate::api::responses::{ApiResponse, SettingsUpdateRequest};
use crate::AppState;
use axum::{extract::State, response::IntoResponse, Json};
use std::sync::Arc;

pub(crate) async fn get_settings(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Json(ApiResponse::ok(state.config.clone()))
}

pub(crate) async fn update_settings(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SettingsUpdateRequest>,
) -> impl IntoResponse {
    let mut config = state.config.clone();

    if let Some(v) = req.scan_threads {
        config.scan_threads = v;
    }
    if let Some(v) = req.slider_alpha {
        config.slider_alpha = v;
    }
    if let Some(v) = req.max_tolerance {
        config.max_tolerance = v;
    }
    if let Some(v) = req.min_free_headroom {
        config.min_free_headroom = v;
    }
    if let Some(v) = req.excluded_disks {
        config.excluded_disks = v.into_iter().collect();
    }
    if let Some(v) = req.warn_parity_check {
        config.warn_parity_check = v;
    }

    if let Err(e) = config.validate() {
        return Json(ApiResponse::<&str>::err(format!("Invalid settings: {e}")));
    }

    match config.save() {
        Ok(()) => Json(ApiResponse::ok("Settings saved (restart to apply)")),
        Err(e) => Json(ApiResponse::<&str>::err(format!("Failed to save settings: {e}"))),
    }
}
