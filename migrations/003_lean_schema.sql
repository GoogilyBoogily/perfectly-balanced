-- Migration 003: Lean schema cleanup
-- Removes dead columns and tables from the schema.
-- Safe to run on existing DBs: files data is transient (rebuilt on next scan),
-- and old plans are stale after a schema upgrade.

-- Drop dead table
DROP TABLE IF EXISTS folder_sizes;

-- Drop old indexes before recreating tables
DROP INDEX IF EXISTS idx_files_disk_size;
DROP INDEX IF EXISTS idx_files_size_global;
DROP INDEX IF EXISTS idx_files_parent;

-- Recreate files with lean schema (data is transient — rebuilt on next scan)
DROP TABLE IF EXISTS files;
CREATE TABLE files (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    disk_id       INTEGER NOT NULL REFERENCES disks(id),
    file_path     TEXT NOT NULL,
    size_bytes    INTEGER NOT NULL DEFAULT 0,
    mtime         INTEGER,
    UNIQUE(disk_id, file_path)
);
CREATE INDEX idx_files_disk_size ON files(disk_id, size_bytes DESC);

-- Drop old move indexes before recreating table
DROP INDEX IF EXISTS idx_moves_plan_order;
DROP INDEX IF EXISTS idx_moves_plan_status;

-- Recreate planned_moves without file_id (old plans are stale after schema upgrade)
DROP TABLE IF EXISTS planned_moves;
CREATE TABLE planned_moves (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    plan_id         INTEGER NOT NULL REFERENCES balance_plans(id) ON DELETE CASCADE,
    source_disk_id  INTEGER NOT NULL REFERENCES disks(id),
    target_disk_id  INTEGER NOT NULL REFERENCES disks(id),
    file_path       TEXT NOT NULL,
    file_size       INTEGER NOT NULL,
    exec_order      INTEGER NOT NULL,
    phase           INTEGER NOT NULL DEFAULT 1,
    status          TEXT DEFAULT 'pending'
        CHECK(status IN ('pending','in_progress','completed','failed','skipped')),
    error_message   TEXT,
    source_mtime    INTEGER,
    CHECK(source_disk_id != target_disk_id)
);
CREATE INDEX idx_moves_plan_order ON planned_moves(plan_id, exec_order);
CREATE INDEX idx_moves_plan_status ON planned_moves(plan_id, status);

-- Mark any in-flight plans as failed since we just dropped their moves
UPDATE balance_plans SET status = 'failed' WHERE status IN ('planned', 'executing');

INSERT OR IGNORE INTO schema_version (version) VALUES (3);
