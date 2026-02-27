-- Migration 002: Add source_mtime to planned_moves
-- Enables mtime-based verification before source deletion (two-phase move safety)

ALTER TABLE planned_moves ADD COLUMN source_mtime INTEGER;
INSERT OR IGNORE INTO schema_version (version) VALUES (2);
