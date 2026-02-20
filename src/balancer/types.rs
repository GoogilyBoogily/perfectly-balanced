use crate::db::Disk;

/// Classification of a disk relative to the target utilization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiskClass {
    OverUtilized,
    AboveAverage,
    BelowAverage,
    UnderUtilized,
}

/// Internal working state for a disk during planning.
#[derive(Debug, Clone)]
pub(crate) struct DiskState {
    pub disk: Disk,
    pub class: DiskClass,
    /// Simulated used bytes (changes as moves are planned).
    pub sim_used: u64,
}

impl DiskState {
    pub(crate) fn sim_utilization(&self) -> f64 {
        if self.disk.total_bytes == 0 {
            return 0.0;
        }
        self.sim_used as f64 / self.disk.total_bytes as f64
    }

    pub(crate) const fn sim_free(&self) -> u64 {
        self.disk.total_bytes.saturating_sub(self.sim_used)
    }
}

/// Result of running the balance algorithm.
#[derive(Debug)]
pub(crate) struct BalanceResult {
    pub plan_id: i64,
    pub target_utilization: f64,
    pub initial_imbalance: f64,
    pub projected_imbalance: f64,
    pub total_moves: usize,
    pub total_bytes: u64,
}
