use super::models::Disk;
use super::optional_ext::OptionalExt;
use super::Database;
use anyhow::Result;
use rusqlite::params;

/// Map a row from the disks table into a `Disk`.
fn map_disk_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Disk> {
    Ok(Disk {
        id: row.get(0)?,
        disk_name: row.get(1)?,
        mount_path: row.get(2)?,
        total_bytes: row.get::<_, i64>(3)? as u64,
        used_bytes: row.get::<_, i64>(4)? as u64,
        free_bytes: row.get::<_, i64>(5)? as u64,
        filesystem: row.get(6)?,
        included: row.get::<_, i64>(7)? != 0,
        updated_at: row.get(8)?,
    })
}

const DISK_COLUMNS: &str =
    "id, disk_name, mount_path, total_bytes, used_bytes, free_bytes, filesystem, included, updated_at";

impl Database {
    /// Insert or update a disk record, returning its ID in a single round-trip.
    #[allow(clippy::too_many_arguments)]
    pub fn upsert_disk(
        &self,
        disk_name: &str,
        mount_path: &str,
        total_bytes: u64,
        used_bytes: u64,
        free_bytes: u64,
        filesystem: Option<&str>,
    ) -> Result<i64> {
        let conn = self.conn();
        let id: i64 = conn.query_row(
            "INSERT INTO disks (disk_name, mount_path, total_bytes, used_bytes, free_bytes, \
             filesystem, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, strftime('%Y-%m-%dT%H:%M:%fZ','now'))
             ON CONFLICT(disk_name) DO UPDATE SET
                mount_path = excluded.mount_path,
                total_bytes = excluded.total_bytes,
                used_bytes = excluded.used_bytes,
                free_bytes = excluded.free_bytes,
                filesystem = excluded.filesystem,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
             RETURNING id",
            params![
                disk_name,
                mount_path,
                total_bytes as i64,
                used_bytes as i64,
                free_bytes as i64,
                filesystem
            ],
            |row| row.get(0),
        )?;
        Ok(id)
    }

    /// Get all disks.
    pub fn get_all_disks(&self) -> Result<Vec<Disk>> {
        let conn = self.conn();
        let mut stmt =
            conn.prepare(&format!("SELECT {DISK_COLUMNS} FROM disks ORDER BY disk_name"))?;

        let disks = stmt.query_map([], map_disk_row)?.collect::<Result<Vec<_>, _>>()?;

        Ok(disks)
    }

    /// Get included disks only.
    pub fn get_included_disks(&self) -> Result<Vec<Disk>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {DISK_COLUMNS} FROM disks WHERE included = 1 ORDER BY disk_name"
        ))?;

        let disks = stmt.query_map([], map_disk_row)?.collect::<Result<Vec<_>, _>>()?;

        Ok(disks)
    }

    /// Get a disk by ID.
    pub fn get_disk(&self, disk_id: i64) -> Result<Option<Disk>> {
        let conn = self.conn();
        let disk = conn
            .query_row(
                &format!("SELECT {DISK_COLUMNS} FROM disks WHERE id = ?1"),
                params![disk_id],
                map_disk_row,
            )
            .optional()?;

        Ok(disk)
    }

    /// Set disk inclusion status.
    pub fn set_disk_included(&self, disk_id: i64, included: bool) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE disks SET included = ?1, \
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?2",
            params![included as i64, disk_id],
        )?;
        Ok(())
    }
}
