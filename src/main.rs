use anyhow::{Context, Result};
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

    // Acquire exclusive file lock to prevent dual daemon instances.
    // The lock is held for the process lifetime via _lock_guard.
    let lock_path = std::path::Path::new(&config.db_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("/tmp"))
        .join("perfectly-balanced.lock");
    let lock_file = std::fs::File::create(&lock_path)
        .with_context(|| format!("Failed to create lock file: {}", lock_path.display()))?;
    try_lock_exclusive(&lock_file, &lock_path)?;
    let _lock_guard = lock_file; // Hold for process lifetime
    info!("Acquired exclusive lock: {}", lock_path.display());

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

    let app = api::router(Arc::clone(&state));

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
        if let Err(e) = child.kill().await {
            warn!("Failed to kill rsync child: {}", e);
        }
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

/// Try to acquire an exclusive flock on the given file.
/// Replaces the unmaintained `fs2` crate with a direct `libc::flock` call.
#[allow(unsafe_code)] // flock() is a safe POSIX operation; no memory unsafety
fn try_lock_exclusive(file: &std::fs::File, lock_path: &std::path::Path) -> Result<()> {
    use std::os::unix::io::AsRawFd;
    let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        anyhow::bail!(
            "Another perfectly-balanced instance is already running (lock: {}): {}",
            lock_path.display(),
            err
        );
    }
    Ok(())
}

/// Wait for SIGTERM or SIGINT for graceful shutdown.
#[allow(clippy::expect_used)] // Signal handlers are bootstrap code; panic is correct failure mode
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
