use super::models::{FileEntry, FileInsert};
use super::Database;
use anyhow::Result;
use rusqlite::params;

/// Map a row from the files table into a `FileEntry`.
fn map_file_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<FileEntry> {
    Ok(FileEntry {
        id: row.get(0)?,
        disk_id: row.get(1)?,
        file_path: row.get(2)?,
        file_name: row.get(3)?,
        size_bytes: row.get::<_, i64>(4)? as u64,
        is_directory: row.get::<_, i64>(5)? != 0,
        parent_path: row.get(6)?,
        mtime: row.get(7)?,
    })
}

const FILE_COLUMNS: &str =
    "id, disk_id, file_path, file_name, size_bytes, is_directory, parent_path, mtime";

impl Database {
    /// Begin a full disk rescan: clear existing data and return a transaction guard.
    /// The caller must call `commit_disk_scan` when done, or the changes are rolled back.
    pub fn begin_disk_scan(&self, disk_id: i64) -> Result<()> {
        let conn = self.conn();
        conn.execute_batch("BEGIN IMMEDIATE")?;
        conn.execute("DELETE FROM files WHERE disk_id = ?1", params![disk_id])?;
        conn.execute("DELETE FROM folder_sizes WHERE disk_id = ?1", params![disk_id])?;
        Ok(())
    }

    /// Batch insert files within the current transaction.
    pub fn insert_files_batch(&self, files: &[FileInsert]) -> Result<()> {
        let conn = self.conn();

        let mut stmt = conn.prepare_cached(
            "INSERT OR REPLACE INTO files \
             (disk_id, file_path, file_name, size_bytes, is_directory, parent_path, mtime)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;

        for f in files {
            stmt.execute(params![
                f.disk_id,
                f.file_path,
                f.file_name,
                f.size_bytes as i64,
                f.is_directory as i64,
                f.parent_path,
                f.mtime,
            ])?;
        }

        Ok(())
    }

    /// Finalize a disk scan: recompute folder sizes and commit the transaction.
    pub fn commit_disk_scan(&self, disk_id: i64) -> Result<()> {
        let conn = self.conn();
        conn.execute("DELETE FROM folder_sizes WHERE disk_id = ?1", params![disk_id])?;
        conn.execute(
            "INSERT INTO folder_sizes (disk_id, folder_path, total_bytes, file_count)
             SELECT disk_id, parent_path, SUM(size_bytes), COUNT(*)
             FROM files
             WHERE disk_id = ?1 AND is_directory = 0 AND parent_path IS NOT NULL
             GROUP BY disk_id, parent_path",
            params![disk_id],
        )?;
        conn.execute_batch("COMMIT")?;
        Ok(())
    }

    /// Roll back a disk scan transaction (e.g. on error or cancellation).
    pub fn rollback_disk_scan(&self) -> Result<()> {
        let conn = self.conn();
        conn.execute_batch("ROLLBACK")?;
        Ok(())
    }

    /// Get all non-directory files on a disk, sorted by size descending.
    pub fn get_all_files_on_disk_by_size(&self, disk_id: i64) -> Result<Vec<FileEntry>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            &format!(
                "SELECT {FILE_COLUMNS} FROM files \
                 WHERE disk_id = ?1 AND is_directory = 0 \
                 ORDER BY size_bytes DESC"
            ),
        )?;

        let files = stmt
            .query_map(params![disk_id], map_file_row)?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(files)
    }

    /// Get total file count and bytes for a disk.
    pub fn get_disk_file_stats(&self, disk_id: i64) -> Result<(u64, u64)> {
        let conn = self.conn();
        let (count, bytes): (i64, i64) = conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(size_bytes), 0) \
             FROM files WHERE disk_id = ?1 AND is_directory = 0",
            params![disk_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        Ok((count as u64, bytes as u64))
    }
}
