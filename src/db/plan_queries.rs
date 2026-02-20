use super::models::{BalancePlan, PlanStatus};
use super::optional_ext::OptionalExt;
use super::Database;
use anyhow::Result;
use rusqlite::params;

impl Database {
    /// Create a new balance plan.
    pub fn create_plan(
        &self,
        tolerance: f64,
        slider_alpha: f64,
        target_utilization: f64,
        initial_imbalance: f64,
    ) -> Result<i64> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO balance_plans \
             (tolerance, slider_alpha, target_utilization, initial_imbalance)
             VALUES (?1, ?2, ?3, ?4)",
            params![tolerance, slider_alpha, target_utilization, initial_imbalance],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Update a plan with projected results.
    pub fn update_plan_projections(
        &self,
        plan_id: i64,
        projected_imbalance: f64,
        total_moves: i32,
        total_bytes: u64,
    ) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE balance_plans \
             SET projected_imbalance = ?1, total_moves = ?2, total_bytes_to_move = ?3 \
             WHERE id = ?4",
            params![projected_imbalance, total_moves, total_bytes as i64, plan_id],
        )?;
        Ok(())
    }

    /// Update plan status.
    pub fn update_plan_status(&self, plan_id: i64, status: PlanStatus) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE balance_plans SET status = ?1 WHERE id = ?2",
            params![status.as_str(), plan_id],
        )?;
        Ok(())
    }

    /// Get a balance plan by ID.
    pub fn get_plan(&self, plan_id: i64) -> Result<Option<BalancePlan>> {
        let conn = self.conn();
        let plan = conn
            .query_row(
                "SELECT id, created_at, tolerance, slider_alpha, target_utilization,
                        initial_imbalance, projected_imbalance, total_moves, \
                 total_bytes_to_move, status
                 FROM balance_plans WHERE id = ?1",
                params![plan_id],
                |row| {
                    let status_str: String = row.get(9)?;
                    let status = PlanStatus::try_from(status_str.as_str())
                        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(9, rusqlite::types::Type::Text, Box::from(e)))?;
                    Ok(BalancePlan {
                        id: row.get(0)?,
                        created_at: row.get(1)?,
                        tolerance: row.get(2)?,
                        slider_alpha: row.get(3)?,
                        target_utilization: row.get(4)?,
                        initial_imbalance: row.get(5)?,
                        projected_imbalance: row.get(6)?,
                        total_moves: row.get(7)?,
                        total_bytes_to_move: row.get::<_, i64>(8)? as u64,
                        status,
                    })
                },
            )
            .optional()?;

        Ok(plan)
    }
}
