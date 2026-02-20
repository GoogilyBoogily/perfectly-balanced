use serde::{Deserialize, Serialize};
use std::fmt;

/// Represents a physical disk in the Unraid array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Disk {
    pub id: i64,
    pub disk_name: String,
    pub mount_path: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
    pub filesystem: Option<String>,
    pub included: bool,
    pub updated_at: Option<String>,
}

impl Disk {
    /// Current utilization as a fraction (0.0 - 1.0).
    pub fn utilization(&self) -> f64 {
        if self.total_bytes == 0 {
            return 0.0;
        }
        self.used_bytes as f64 / self.total_bytes as f64
    }
}

/// A file entry in the catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub id: i64,
    pub disk_id: i64,
    pub file_path: String,
    pub file_name: String,
    pub size_bytes: u64,
    pub is_directory: bool,
    pub parent_path: Option<String>,
    pub mtime: Option<i64>,
}

/// Status of a balance plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    Planned,
    Executing,
    Completed,
    Cancelled,
    Failed,
}

impl PlanStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Executing => "executing",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }
}

impl fmt::Display for PlanStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl TryFrom<&str> for PlanStatus {
    type Error = String;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "planned" => Ok(Self::Planned),
            "executing" => Ok(Self::Executing),
            "completed" => Ok(Self::Completed),
            "cancelled" => Ok(Self::Cancelled),
            "failed" => Ok(Self::Failed),
            _ => Err(format!("invalid plan status: {s}")),
        }
    }
}

/// Status of a single planned move.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MoveStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Skipped,
}

impl MoveStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }
}

impl fmt::Display for MoveStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl TryFrom<&str> for MoveStatus {
    type Error = String;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "pending" => Ok(Self::Pending),
            "in_progress" => Ok(Self::InProgress),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "skipped" => Ok(Self::Skipped),
            _ => Err(format!("invalid move status: {s}")),
        }
    }
}

/// A balance plan that groups a set of planned moves.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalancePlan {
    pub id: i64,
    pub created_at: Option<String>,
    pub tolerance: f64,
    pub slider_alpha: f64,
    pub target_utilization: f64,
    pub initial_imbalance: Option<f64>,
    pub projected_imbalance: Option<f64>,
    pub total_moves: i32,
    pub total_bytes_to_move: u64,
    pub status: PlanStatus,
}

/// A single file move within a balance plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedMove {
    pub id: i64,
    pub plan_id: i64,
    pub file_id: i64,
    pub source_disk_id: i64,
    pub target_disk_id: i64,
    pub file_path: String,
    pub file_size: u64,
    pub move_order: i32,
    pub phase: i32,
    pub status: MoveStatus,
    pub error_message: Option<String>,
}

/// A move with additional context for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedMoveDetail {
    #[serde(flatten)]
    pub move_info: PlannedMove,
    pub source_disk_name: String,
    pub target_disk_name: String,
}

/// Insert batch for scanning — lighter weight than FileEntry.
#[derive(Debug, Clone)]
pub struct FileInsert {
    pub disk_id: i64,
    pub file_path: String,
    pub file_name: String,
    pub size_bytes: u64,
    pub is_directory: bool,
    pub parent_path: Option<String>,
    pub mtime: Option<i64>,
}
