use crate::api::responses::ApiResponse;
use crate::AppState;
use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use std::sync::Arc;

pub(crate) async fn get_disks(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.db.get_all_disks() {
        Ok(disks) => Json(ApiResponse::ok(disks)),
        Err(e) => {
            Json(ApiResponse::<Vec<crate::db::Disk>>::err(format!("Failed to get disks: {e}")))
        }
    }
}

pub(crate) async fn set_disk_included(
    State(state): State<Arc<AppState>>,
    Path(disk_id): Path<i64>,
) -> impl IntoResponse {
    match state.db.set_disk_included(disk_id, true) {
        Ok(()) => Json(ApiResponse::ok("Disk included")),
        Err(e) => Json(ApiResponse::<&str>::err(format!("{e}"))),
    }
}

pub(crate) async fn set_disk_excluded(
    State(state): State<Arc<AppState>>,
    Path(disk_id): Path<i64>,
) -> impl IntoResponse {
    match state.db.set_disk_included(disk_id, false) {
        Ok(()) => Json(ApiResponse::ok("Disk excluded")),
        Err(e) => Json(ApiResponse::<&str>::err(format!("{e}"))),
    }
}
