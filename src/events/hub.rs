use serde::Serialize;
use tokio::sync::broadcast;

/// Events that flow from background tasks (scanner, executor) to SSE subscribers.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", content = "data")]
pub enum Event {
    /// Progress update during filesystem scanning.
    ScanProgress {
        disk: String,
        files_scanned: u64,
        bytes_cataloged: u64,
        /// Estimated percent complete for this disk (0.0 - 100.0).
        percent: f64,
    },

    /// A single disk has finished scanning.
    ScanDiskComplete { disk: String, total_files: u64, total_bytes: u64 },

    /// All disk scanning is complete.
    ScanComplete { total_disks: u32, total_files: u64, total_bytes: u64, duration_seconds: f64 },

    /// A balance plan has been generated and is ready for review.
    PlanReady { plan_id: i64, total_moves: u32, total_bytes: u64, projected_imbalance: f64 },

    /// Progress update for a single file move via rsync.
    MoveProgress {
        move_id: i64,
        file_path: String,
        /// Percent of this file transferred (0.0 - 100.0).
        percent: f64,
        /// Transfer speed as reported by rsync (e.g., "112.45MB/s").
        speed: String,
        /// Estimated time remaining (e.g., "0:01:45").
        eta: String,
    },

    /// A single file move has completed.
    MoveComplete {
        move_id: i64,
        status: String, // "success" | "failed" | "skipped"
        verified: bool,
        error: Option<String>,
    },

    /// The entire plan execution has finished.
    ExecutionComplete {
        plan_id: i64,
        moves_completed: u32,
        moves_failed: u32,
        moves_skipped: u32,
        duration_seconds: f64,
    },

    /// A generic error event.
    DaemonError { message: String },
}

impl Event {
    /// Returns the SSE event type name for this event variant.
    pub const fn event_type(&self) -> &'static str {
        match self {
            Self::ScanProgress { .. } => "scan_progress",
            Self::ScanDiskComplete { .. } => "scan_disk_complete",
            Self::ScanComplete { .. } => "scan_complete",
            Self::PlanReady { .. } => "plan_ready",
            Self::MoveProgress { .. } => "move_progress",
            Self::MoveComplete { .. } => "move_complete",
            Self::ExecutionComplete { .. } => "execution_complete",
            Self::DaemonError { .. } => "daemon_error",
        }
    }
}

/// The central event broadcast hub.
///
/// Background tasks (scanner, executor) send events here via `publish()`.
/// SSE endpoint handlers subscribe via `subscribe()` and forward events to the browser.
#[derive(Debug, Clone)]
pub struct EventHub {
    sender: broadcast::Sender<Event>,
}

impl EventHub {
    /// Create a new EventHub with the given channel capacity.
    ///
    /// If subscribers fall behind by more than `capacity` events, they will
    /// receive a `Lagged` error and miss intermediate events. 256 is a safe
    /// default for the expected event rate.
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Publish an event to all current subscribers.
    ///
    /// Returns Ok(subscriber_count) or Err if there are no active subscribers
    /// (which is fine — events are fire-and-forget).
    pub fn publish(&self, event: Event) -> Result<usize, broadcast::error::SendError<Event>> {
        self.sender.send(event)
    }

    /// Subscribe to the event stream. Returns a broadcast Receiver.
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.sender.subscribe()
    }
}
