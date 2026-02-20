-- Perfectly Balanced: Initial schema
-- Applied automatically on first startup

PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS disks (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    disk_name     TEXT NOT NULL UNIQUE,
    mount_path    TEXT NOT NULL UNIQUE,
    total_bytes   INTEGER NOT NULL,
    used_bytes    INTEGER NOT NULL DEFAULT 0,
    free_bytes    INTEGER NOT NULL DEFAULT 0,
    filesystem    TEXT,
    included      INTEGER NOT NULL DEFAULT 1,
    updated_at    TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE TABLE IF NOT EXISTS files (
    id            INTEGER PRIMARY KEY,
    disk_id       INTEGER NOT NULL REFERENCES disks(id),
    file_path     TEXT NOT NULL,
    file_name     TEXT NOT NULL,
    size_bytes    INTEGER NOT NULL DEFAULT 0,
    is_directory  INTEGER NOT NULL DEFAULT 0,
    parent_path   TEXT,
    mtime         INTEGER,
    scan_id       INTEGER,
    UNIQUE(disk_id, file_path)
);

CREATE INDEX IF NOT EXISTS idx_files_disk_size ON files(disk_id, size_bytes DESC)
    WHERE is_directory = 0;
CREATE INDEX IF NOT EXISTS idx_files_size_global ON files(size_bytes DESC)
    WHERE is_directory = 0;
CREATE INDEX IF NOT EXISTS idx_files_parent ON files(disk_id, parent_path);

CREATE TABLE IF NOT EXISTS folder_sizes (
    id            INTEGER PRIMARY KEY,
    disk_id       INTEGER NOT NULL REFERENCES disks(id),
    folder_path   TEXT NOT NULL,
    total_bytes   INTEGER NOT NULL DEFAULT 0,
    file_count    INTEGER NOT NULL DEFAULT 0,
    UNIQUE(disk_id, folder_path)
);

CREATE TABLE IF NOT EXISTS balance_plans (
    id                    INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at            TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    tolerance             REAL NOT NULL,
    slider_alpha          REAL NOT NULL,
    target_utilization    REAL NOT NULL,
    initial_imbalance     REAL,
    projected_imbalance   REAL,
    total_moves           INTEGER DEFAULT 0,
    total_bytes_to_move   INTEGER DEFAULT 0,
    status                TEXT DEFAULT 'planned'
        CHECK(status IN ('planned','executing','completed','cancelled','failed'))
);

CREATE TABLE IF NOT EXISTS planned_moves (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    plan_id         INTEGER NOT NULL REFERENCES balance_plans(id) ON DELETE CASCADE,
    file_id         INTEGER NOT NULL REFERENCES files(id),
    source_disk_id  INTEGER NOT NULL REFERENCES disks(id),
    target_disk_id  INTEGER NOT NULL REFERENCES disks(id),
    file_path       TEXT NOT NULL,
    file_size       INTEGER NOT NULL,
    exec_order      INTEGER NOT NULL,
    phase           INTEGER NOT NULL DEFAULT 1,
    status          TEXT DEFAULT 'pending'
        CHECK(status IN ('pending','in_progress','completed','failed','skipped')),
    error_message   TEXT,
    CHECK(source_disk_id != target_disk_id)
);

CREATE INDEX IF NOT EXISTS idx_moves_plan_order ON planned_moves(plan_id, exec_order);
CREATE INDEX IF NOT EXISTS idx_moves_plan_status ON planned_moves(plan_id, status);

-- Schema version tracking
CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER PRIMARY KEY,
    applied_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

INSERT OR IGNORE INTO schema_version (version) VALUES (1);
