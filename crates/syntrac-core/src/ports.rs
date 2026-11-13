//! Interfaces (ports) the brain depends on. Implementations live in other crates.
//!
//! Memory layer: [`ProcessRepository`], [`EventRepository`], [`MetricsRepository`], [`Retention`].
//! Tools layer: [`TelemetrySource`], [`MetricsSource`].
//! Planned for later phases: `FileEnricher`, `IntelProvider`, `LLMService`.

use serde::{Deserialize, Serialize};

use crate::event::{EventCategory, EventEnvelope};
use crate::flow::{Destination, FlowRow};
use crate::identity::ProcessInstanceId;
use crate::metrics::{CpuBreakdown, MetricsSample, PurgeCounts, SystemPoint};
use crate::time::Timestamp;

#[derive(Debug, thiserror::Error)]
pub enum RepoError {
    #[error("storage backend failed: {0}")]
    Backend(String),
    #[error("record not found")]
    NotFound,
}

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("telemetry source failed: {0}")]
    Failed(String),
}

/// One observed instance of a process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessRecord {
    pub id: ProcessInstanceId,
    pub pid: u32,
    pub parent_pid: u32,
    /// Resolved parent instance, when the parent was known at start time.
    pub parent_id: Option<ProcessInstanceId>,
    pub image_path: String,
    pub command_line: Option<String>,
    pub started: Timestamp,
    pub ended: Option<Timestamp>,
    /// True when `ended` is a bound rather than a measurement: the process exited while the collector was
    /// down, and was closed at the next startup snapshot.
    pub ended_approximate: bool,
    pub exit_code: Option<u32>,
}

/// Filter for reading events back. All fields are optional and combined with AND.
#[derive(Debug, Clone, Default)]
pub struct EventQuery {
    /// Inclusive lower bound.
    pub from: Option<Timestamp>,
    /// Inclusive upper bound.
    pub to: Option<Timestamp>,
    pub category: Option<EventCategory>,
    pub process: Option<ProcessInstanceId>,
    pub limit: Option<usize>,
}

pub trait ProcessRepository {
    /// Idempotent upsert keyed by `record.id`. It must NEVER clear an `ended` / `exit_code` that is
    /// already stored, so a late startup snapshot cannot resurrect a process that already exited.
    fn record_start(&mut self, record: ProcessRecord) -> Result<(), RepoError>;

    /// Returns `NotFound` when the instance was never recorded.
    fn record_exit(&mut self, id: ProcessInstanceId, ended: Timestamp, exit_code: Option<u32>)
        -> Result<(), RepoError>;

    fn get(&self, id: ProcessInstanceId) -> Result<Option<ProcessRecord>, RepoError>;

    /// Called with the full list of processes alive at `observed_at` (a startup snapshot). Every process
    /// recorded as running that started at or before `observed_at` and is NOT in `alive` exited while the
    /// collector was down: it is closed at `observed_at` and flagged approximate. Returns how many were closed.
    /// Processes already ended, or started later, are untouched.
    fn reconcile_running(&mut self, alive: &[ProcessInstanceId], observed_at: Timestamp) -> Result<u64, RepoError>;

    /// Which instance owned `pid` at time `at`? This is the PID-reuse safeguard.
    fn resolve(&self, pid: u32, at: Timestamp) -> Result<Option<ProcessInstanceId>, RepoError>;
}

/// Which processes to list. Ordered newest start first.
#[derive(Debug, Clone, Default)]
pub struct ProcessFilter {
    /// Only processes that have not ended.
    pub running_only: bool,
    /// Case-insensitive match against any part of the image path.
    pub name_contains: Option<String>,
    pub limit: usize,
}

/// Read-side listing of processes for the UI.
pub trait ProcessQueries {
    fn list_processes(&self, filter: &ProcessFilter) -> Result<Vec<ProcessRecord>, RepoError>;
}

pub trait EventRepository {
    fn append(&mut self, events: &[EventEnvelope]) -> Result<(), RepoError>;

    /// Results are ordered by timestamp ascending.
    fn query(&self, query: &EventQuery) -> Result<Vec<EventEnvelope>, RepoError>;
}

pub trait MetricsRepository {
    fn append_metrics(&mut self, sample: &MetricsSample) -> Result<(), RepoError>;

    /// System time series, inclusive bounds, oldest first.
    fn system_series(&self, from: Timestamp, to: Timestamp) -> Result<Vec<SystemPoint>, RepoError>;

    /// Which processes used the CPU in the window? Heaviest first, at most `limit` processes.
    fn cpu_breakdown(&self, from: Timestamp, to: Timestamp, limit: usize) -> Result<CpuBreakdown, RepoError>;
}

/// Read-side questions about network flows that must not scan JSON payloads.
pub trait FlowQueries {
    /// Remote endpoints by total bytes (sent + received), busiest first. A flow counts when its
    /// window starts within the inclusive range.
    fn top_destinations(&self, from: Timestamp, to: Timestamp, limit: usize) -> Result<Vec<Destination>, RepoError>;

    /// The most recent flows whose window starts within the inclusive range, newest first, each with the name
    /// of the process that made it when that process is known.
    fn recent_flows(&self, from: Timestamp, to: Timestamp, limit: usize) -> Result<Vec<FlowRow>, RepoError>;
}

/// Removes data older than a cutoff so the database stays bounded.
/// Running processes are kept; ended processes are removed once they ended before the cutoff.
pub trait Retention {
    fn purge_before(&mut self, cutoff: Timestamp) -> Result<PurgeCounts, RepoError>;
}

/// A producer of raw, uncorrelated events. Real sources (ETW) buffer internally;
/// `drain` hands over whatever arrived since the last call.
pub trait TelemetrySource {
    fn name(&self) -> &str;
    fn drain(&mut self) -> Result<Vec<EventEnvelope>, SourceError>;

    /// Ends collection. Called once at shutdown, BEFORE the final `drain`, so a source that hands over
    /// buffered events when it ends does not lose them. Must not block indefinitely. Sources with nothing
    /// to release keep this default.
    fn stop(&mut self) {}
}

/// Takes a resource-usage sample. CPU figures are a share of the whole machine.
pub trait MetricsSource {
    fn name(&self) -> &str;
    fn sample(&mut self, now: Timestamp) -> Result<MetricsSample, SourceError>;
}
