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
use tracing::{info, warn};

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

    /// Recover stale states left behind by a crash or kill.
    ///
    /// In a single transaction:
    /// 1. Collect IDs of moves stuck at `in_progress` (for later filesystem cleanup)
    /// 2. Mark any `executing` plans as `failed`
    /// 3. Reset `in_progress` moves back to `pending`
    pub(crate) fn recover_stale_states(&self) -> Result<RecoveryStats> {
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;

        // Collect in_progress move IDs before resetting them
        let mut stmt = tx.prepare("SELECT id FROM planned_moves WHERE status = 'in_progress'")?;
        let recovered_move_ids: Vec<i64> =
            stmt.query_map([], |row| row.get(0))?.collect::<Result<Vec<_>, _>>()?;
        drop(stmt);

        let plans_failed = tx
            .execute("UPDATE balance_plans SET status = 'failed' WHERE status = 'executing'", [])?;

        let moves_reset = tx.execute(
            "UPDATE planned_moves SET status = 'pending', error_message = NULL \
             WHERE status = 'in_progress'",
            [],
        )?;

        tx.commit()?;

        if plans_failed > 0 || moves_reset > 0 {
            warn!(
                "Startup recovery: {} plan(s) marked failed, {} move(s) reset",
                plans_failed, moves_reset
            );
        }

        Ok(RecoveryStats { plans_failed, moves_reset, recovered_move_ids })
    }
}

/// Stats returned by startup recovery.
pub(crate) struct RecoveryStats {
    #[allow(dead_code)]
    pub plans_failed: usize,
    #[allow(dead_code)]
    pub moves_reset: usize,
    /// IDs of moves that were `in_progress` at crash time — need filesystem cleanup.
    pub recovered_move_ids: Vec<i64>,
}
