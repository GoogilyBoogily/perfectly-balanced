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
        size_bytes: row.get::<_, i64>(3)? as u64,
        mtime: row.get(4)?,
    })
}

const FILE_COLUMNS: &str = "id, disk_id, file_path, size_bytes, mtime";

impl Database {
    /// Atomic disk scan: clear existing data and insert all files.
    ///
    /// The entire operation runs in a single transaction under a single mutex lock.
    /// If any step fails, the transaction is rolled back and previous data is preserved.
    pub fn atomic_disk_scan(&self, disk_id: i64, files: &[FileInsert]) -> Result<()> {
        let conn = self.conn()?;
        let tx = conn.unchecked_transaction()?;

        // Clear existing file data for this disk
        tx.execute("DELETE FROM files WHERE disk_id = ?1", params![disk_id])?;

        // Batch insert all files
        {
            let mut stmt = tx.prepare_cached(
                "INSERT OR REPLACE INTO files \
                 (disk_id, file_path, size_bytes, mtime)
                 VALUES (?1, ?2, ?3, ?4)",
            )?;

            for f in files {
                stmt.execute(params![
                    f.disk_id,
                    f.file_path,
                    f.size_bytes as i64,
                    f.mtime,
                ])?;
            }
        }

        tx.commit()?;
        Ok(())
    }

    /// Get all files on a disk, sorted by size descending.
    pub fn get_all_files_on_disk_by_size(&self, disk_id: i64) -> Result<Vec<FileEntry>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {FILE_COLUMNS} FROM files \
                 WHERE disk_id = ?1 \
                 ORDER BY size_bytes DESC"
        ))?;

        let files =
            stmt.query_map(params![disk_id], map_file_row)?.collect::<Result<Vec<_>, _>>()?;

        Ok(files)
    }
}
