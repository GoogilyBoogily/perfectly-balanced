use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::config::AppConfig;
use crate::db::Database;
use crate::events::EventHub;

/// Shared application state passed to all API handlers via axum's State extractor.
pub struct AppState {
    pub db: Database,
    pub config: AppConfig,
    pub event_hub: EventHub,
    pub status: tokio::sync::RwLock<DaemonStatus>,
    /// Shared cancellation flag for the currently running operation (scan or execution).
    pub cancel_flag: Arc<AtomicBool>,
}

impl AppState {
    /// Reset the cancel flag to `false` before starting a new operation.
    pub fn reset_cancel(&self) {
        self.cancel_flag.store(false, Ordering::SeqCst);
    }

    /// Request cancellation of the current operation.
    pub fn request_cancel(&self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
    }
}

/// The daemon's operating state, serialized to the API as a lowercase string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonState {
    Idle,
    Scanning,
    Planning,
    Executing,
}

#[derive(Debug, Clone, Serialize)]
pub struct DaemonStatus {
    pub state: DaemonState,
    pub detail: Option<String>,
}

impl DaemonStatus {
    pub const fn idle() -> Self {
        Self { state: DaemonState::Idle, detail: None }
    }

    pub fn scanning(detail: impl Into<String>) -> Self {
        Self { state: DaemonState::Scanning, detail: Some(detail.into()) }
    }

    pub const fn planning() -> Self {
        Self { state: DaemonState::Planning, detail: None }
    }

    pub fn executing(detail: impl Into<String>) -> Self {
        Self { state: DaemonState::Executing, detail: Some(detail.into()) }
    }
}
