use serde::Serialize;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::config::AppConfig;
use crate::db::Database;
use crate::events::EventHub;

/// Shared application state passed to all API handlers via axum's State extractor.
pub struct AppState {
    pub db: Database,
    pub config: AppConfig,
    pub event_hub: EventHub,
    pub status: tokio::sync::RwLock<DaemonStatus>,
    /// Per-operation cancellation token, replaced on each new scan/execution.
    cancel_token: tokio::sync::Mutex<CancellationToken>,
    /// Handle to the currently running background task (scan or execution).
    pub background_task: tokio::sync::Mutex<Option<JoinHandle<()>>>,
    /// Handle to the in-flight rsync child process, for kill-on-shutdown.
    pub rsync_child: tokio::sync::Mutex<Option<tokio::process::Child>>,
}

impl AppState {
    pub fn new(db: Database, config: AppConfig, event_hub: EventHub) -> Self {
        Self {
            db,
            config,
            event_hub,
            status: tokio::sync::RwLock::new(DaemonStatus::idle()),
            cancel_token: tokio::sync::Mutex::new(CancellationToken::new()),
            background_task: tokio::sync::Mutex::new(None),
            rsync_child: tokio::sync::Mutex::new(None),
        }
    }

    /// Create a fresh `CancellationToken` for a new operation.
    /// Returns a clone for the spawned task to monitor.
    pub async fn new_operation_token(&self) -> CancellationToken {
        let token = CancellationToken::new();
        *self.cancel_token.lock().await = token.clone();
        token
    }

    /// Cancel the current operation (idempotent — safe to call from both
    /// the cancel API endpoint and the shutdown sequence).
    pub async fn request_cancel(&self) {
        self.cancel_token.lock().await.cancel();
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
