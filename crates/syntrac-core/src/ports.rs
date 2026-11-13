//! Interfaces (ports) the brain depends on. Implementations live in other crates.
//!
//! Memory layer: [`ProcessRepository`], [`EventRepository`].
//! Tools layer: [`TelemetrySource`].
//! Planned for later phases: `FileEnricher`, `IntelProvider`, `LLMService`.

use serde::{Deserialize, Serialize};

use crate::event::{EventCategory, EventEnvelope};
use crate::identity::ProcessInstanceId;
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
    fn record_start(&mut self, record: ProcessRecord) -> Result<(), RepoError>;

    fn record_exit(&mut self, id: ProcessInstanceId, ended: Timestamp, exit_code: Option<u32>)
        -> Result<(), RepoError>;

    fn get(&self, id: ProcessInstanceId) -> Result<Option<ProcessRecord>, RepoError>;

    /// Which instance owned `pid` at time `at`? This is the PID-reuse safeguard.
    fn resolve(&self, pid: u32, at: Timestamp) -> Result<Option<ProcessInstanceId>, RepoError>;
}

pub trait EventRepository {
    fn append(&mut self, events: &[EventEnvelope]) -> Result<(), RepoError>;

    /// Results are ordered by timestamp ascending.
    fn query(&self, query: &EventQuery) -> Result<Vec<EventEnvelope>, RepoError>;
}

/// A producer of raw, uncorrelated events. Real sources (ETW) buffer internally;
/// `drain` hands over whatever arrived since the last call.
pub trait TelemetrySource {
    fn name(&self) -> &str;
    fn drain(&mut self) -> Result<Vec<EventEnvelope>, SourceError>;
}
