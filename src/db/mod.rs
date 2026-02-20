mod disk_queries;
mod file_queries;
mod models;
mod move_queries;
mod optional_ext;
mod plan_queries;

pub(crate) use models::*;

use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;
use tracing::info;

/// Thread-safe wrapper around a SQLite connection.
///
/// SQLite in WAL mode supports concurrent readers but only one writer.
/// We use a Mutex to serialize all access — this is fine for our workload
/// where writes are batched and reads are infrequent API calls.
pub struct Database {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database").finish()
    }
}

impl Database {
    /// Open (or create) the SQLite database at the given path.
    pub fn open(path: &str) -> Result<Self> {
        if let Some(parent) = Path::new(path).parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create database directory: {}", parent.display())
            })?;
        }

        let conn =
            Connection::open(path).with_context(|| format!("Failed to open database at {path}"))?;

        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             PRAGMA cache_size = -64000;
             PRAGMA temp_store = MEMORY;",
        )?;

        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Open an in-memory database (for testing).
    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;",
        )?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Run database migrations.
    pub fn run_migrations(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        let has_schema_table: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master \
                 WHERE type='table' AND name='schema_version'",
                [],
                |row| row.get(0),
            )
            .context("Failed to check for schema_version table")?;

        let current_version = if has_schema_table {
            conn.query_row("SELECT COALESCE(MAX(version), 0) FROM schema_version", [], |row| {
                row.get::<_, i64>(0)
            })
            .context("Failed to read schema version")?
        } else {
            0
        };

        if current_version < 1 {
            info!("Applying migration 001_initial...");
            let migration = include_str!("../../migrations/001_initial.sql");
            conn.execute_batch(migration)?;
            info!("Migration 001_initial applied successfully");
        }

        Ok(())
    }

    /// Get a lock on the database connection for executing queries.
    pub fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap()
    }
}
