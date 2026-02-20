use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tracing::{error, info, warn};

mod api;
mod balancer;
mod config;
mod db;
mod events;
mod executor;
mod scanner;
mod state;

#[cfg(test)]
mod tests;

use config::AppConfig;
use db::Database;
use events::EventHub;
pub use state::{AppState, DaemonState, DaemonStatus};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "perfectly_balanced=info,tower_http=info".into()),
        )
        .init();

    info!("Perfectly Balanced v{} starting up", env!("CARGO_PKG_VERSION"));

    let config = AppConfig::load()?;
    info!("Configuration loaded: port={}, db_path={}", config.port, config.db_path);

    let db = Database::open(&config.db_path)?;
    db.run_migrations()?;
    info!("Database initialized at {}", config.db_path);

    // --- Startup recovery: fix stale states left by previous crash ---
    let recovery = db.recover_stale_states()?;
    if !recovery.recovered_move_ids.is_empty() {
        executor::recovery::cleanup_partial_files(&db, &recovery.recovered_move_ids).await?;
    }

    let event_hub = EventHub::new(256);

    let state = Arc::new(AppState::new(db, config.clone(), event_hub));

    let app = api::router(state.clone());

    let bind_addr = format!("127.0.0.1:{}", config.port);
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    info!("Listening on {}", bind_addr);

    axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await?;

    // --- Graceful shutdown: cancel operations, kill rsync, await background task ---
    info!("Shutting down...");

    // 1. Cancel any running operation
    state.request_cancel().await;

    // 2. Kill any running rsync child process
    let rsync_child = state.rsync_child.lock().await.take();
    if let Some(mut child) = rsync_child {
        info!("Killing in-flight rsync child");
        child.kill().await.ok();
    }

    // 3. Wait for background task with timeout
    let bg_task = state.background_task.lock().await.take();
    if let Some(handle) = bg_task {
        match tokio::time::timeout(Duration::from_secs(10), handle).await {
            Ok(Ok(())) => info!("Background task completed cleanly"),
            Ok(Err(e)) => error!("Background task error: {:?}", e),
            Err(_) => warn!("Background task did not finish within 10s, abandoning"),
        }
    }

    info!("Perfectly Balanced shut down cleanly");
    Ok(())
}

/// Wait for SIGTERM or SIGINT for graceful shutdown.
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c().await.expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => { warn!("Received Ctrl+C, shutting down..."); },
        () = terminate => { warn!("Received SIGTERM, shutting down..."); },
    }
}
