use anyhow::Result;
use std::sync::Arc;
use tokio::signal;
use tracing::{info, warn};

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

    let event_hub = EventHub::new(256);

    let state = Arc::new(AppState {
        db,
        config: config.clone(),
        event_hub,
        status: tokio::sync::RwLock::new(DaemonStatus::idle()),
        cancel_flag: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    });

    let app = api::router(state.clone());

    let bind_addr = format!("127.0.0.1:{}", config.port);
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    info!("Listening on {}", bind_addr);

    axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await?;

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
