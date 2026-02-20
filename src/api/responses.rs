use serde::{Deserialize, Serialize};

/// Generic API response wrapper.
#[derive(Debug, Serialize)]
pub(crate) struct ApiResponse<T: Serialize> {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl<T: Serialize> ApiResponse<T> {
    pub(crate) const fn ok(data: T) -> Self {
        Self { success: true, data: Some(data), error: None }
    }

    pub(crate) fn err(msg: impl Into<String>) -> Self {
        Self { success: false, data: None, error: Some(msg.into()) }
    }
}

/// Request body for POST /api/scan.
#[derive(Debug, Deserialize)]
pub(crate) struct ScanRequest {
    pub threads: Option<usize>,
    pub disk_ids: Option<Vec<i64>>,
}

/// Request body for POST /api/plan.
#[derive(Debug, Deserialize)]
pub(crate) struct PlanRequest {
    pub alpha: Option<f64>,
    pub excluded_disks: Option<Vec<i64>>,
}

/// Request body for POST /api/settings.
#[derive(Debug, Deserialize)]
pub(crate) struct SettingsUpdateRequest {
    pub scan_threads: Option<usize>,
    pub slider_alpha: Option<f64>,
    pub max_tolerance: Option<f64>,
    pub min_free_headroom: Option<u64>,
    pub excluded_disks: Option<Vec<String>>,
    pub warn_parity_check: Option<bool>,
}

/// Scan progress summary returned by status endpoint.
#[derive(Debug, Serialize)]
pub(crate) struct StatusResponse {
    pub state: crate::DaemonState,
    pub detail: Option<String>,
    pub version: String,
}

/// Plan summary for responses.
#[derive(Debug, Serialize)]
pub(crate) struct PlanSummary {
    pub id: i64,
    pub created_at: Option<String>,
    pub tolerance: f64,
    pub slider_alpha: f64,
    pub target_utilization: f64,
    pub initial_imbalance: Option<f64>,
    pub projected_imbalance: Option<f64>,
    pub total_moves: i32,
    pub total_bytes_to_move: u64,
    pub status: crate::db::PlanStatus,
    pub moves: Vec<crate::db::PlannedMoveDetail>,
}
