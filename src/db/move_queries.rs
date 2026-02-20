use super::models::{MoveStatus, MovePathInfo, PlannedMove, PlannedMoveDetail};
use super::Database;
use anyhow::Result;
use rusqlite::params;

/// Map a row from the planned_moves JOIN query into a `PlannedMoveDetail`.
fn map_move_detail_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PlannedMoveDetail> {
    let status_str: String = row.get(9)?;
    let status = MoveStatus::try_from(status_str.as_str())
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(9, rusqlite::types::Type::Text, Box::from(e)))?;
    Ok(PlannedMoveDetail {
        move_info: PlannedMove {
            id: row.get(0)?,
            plan_id: row.get(1)?,
            file_id: row.get(2)?,
            source_disk_id: row.get(3)?,
            target_disk_id: row.get(4)?,
            file_path: row.get(5)?,
            file_size: row.get::<_, i64>(6)? as u64,
            move_order: row.get(7)?,
            phase: row.get(8)?,
            status,
            error_message: row.get(10)?,
        },
        source_disk_name: row.get(11)?,
        target_disk_name: row.get(12)?,
    })
}

impl Database {
    /// Insert a batch of planned moves.
    pub fn insert_planned_moves(&self, moves: &[PlannedMove]) -> Result<()> {
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;

        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO planned_moves \
                 (plan_id, file_id, source_disk_id, target_disk_id, file_path, \
                 file_size, exec_order, phase)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;

            for m in moves {
                stmt.execute(params![
                    m.plan_id,
                    m.file_id,
                    m.source_disk_id,
                    m.target_disk_id,
                    m.file_path,
                    m.file_size as i64,
                    m.move_order,
                    m.phase,
                ])?;
            }
        }

        tx.commit()?;
        Ok(())
    }

    /// Get all moves for a plan, ordered by execution order.
    pub fn get_plan_moves(&self, plan_id: i64) -> Result<Vec<PlannedMoveDetail>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT m.id, m.plan_id, m.file_id, m.source_disk_id, m.target_disk_id,
                    m.file_path, m.file_size, m.exec_order, m.phase, m.status, \
             m.error_message,
                    s.disk_name AS source_disk_name, t.disk_name AS target_disk_name
             FROM planned_moves m
             JOIN disks s ON m.source_disk_id = s.id
             JOIN disks t ON m.target_disk_id = t.id
             WHERE m.plan_id = ?1
             ORDER BY m.exec_order",
        )?;

        let moves = stmt
            .query_map(params![plan_id], map_move_detail_row)?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(moves)
    }

    /// Update the status of a specific move.
    pub fn update_move_status(
        &self,
        move_id: i64,
        status: MoveStatus,
        error_message: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE planned_moves SET status = ?1, error_message = ?2 WHERE id = ?3",
            params![status.as_str(), error_message, move_id],
        )?;
        Ok(())
    }

    /// Get all pending moves for a plan in a specific phase.
    pub fn get_pending_moves_for_phase(
        &self,
        plan_id: i64,
        phase: i32,
    ) -> Result<Vec<PlannedMoveDetail>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT m.id, m.plan_id, m.file_id, m.source_disk_id, m.target_disk_id,
                    m.file_path, m.file_size, m.exec_order, m.phase, m.status, \
             m.error_message,
                    s.disk_name AS source_disk_name, t.disk_name AS target_disk_name
             FROM planned_moves m
             JOIN disks s ON m.source_disk_id = s.id
             JOIN disks t ON m.target_disk_id = t.id
             WHERE m.plan_id = ?1 AND m.phase = ?2 AND m.status = 'pending'
             ORDER BY m.exec_order",
        )?;

        let moves = stmt
            .query_map(params![plan_id, phase], map_move_detail_row)?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(moves)
    }

    /// Get the max phase number in a plan.
    pub fn get_max_phase(&self, plan_id: i64) -> Result<i32> {
        let conn = self.conn();
        let max: i32 = conn.query_row(
            "SELECT COALESCE(MAX(phase), 0) FROM planned_moves WHERE plan_id = ?1",
            params![plan_id],
            |row| row.get(0),
        )?;
        Ok(max)
    }

    /// Get lightweight path info for a set of move IDs (used by crash recovery).
    pub fn get_moves_path_info(&self, ids: &[i64]) -> Result<Vec<MovePathInfo>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let conn = self.conn();
        let placeholders: String = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT m.id, m.file_path, s.mount_path, t.mount_path \
             FROM planned_moves m \
             JOIN disks s ON m.source_disk_id = s.id \
             JOIN disks t ON m.target_disk_id = t.id \
             WHERE m.id IN ({placeholders})"
        );

        let mut stmt = conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::types::ToSql> =
            ids.iter().map(|id| id as &dyn rusqlite::types::ToSql).collect();

        let moves = stmt
            .query_map(params.as_slice(), |row| {
                Ok(MovePathInfo {
                    id: row.get(0)?,
                    file_path: row.get(1)?,
                    source_mount: row.get(2)?,
                    target_mount: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(moves)
    }

    /// Mark all in_progress moves for a plan as failed (used by panic guard).
    pub fn fail_in_progress_moves(&self, plan_id: i64) -> Result<usize> {
        let conn = self.conn();
        let count = conn.execute(
            "UPDATE planned_moves SET status = 'failed', error_message = 'Task panicked' \
             WHERE plan_id = ?1 AND status = 'in_progress'",
            params![plan_id],
        )?;
        Ok(count)
    }
}
