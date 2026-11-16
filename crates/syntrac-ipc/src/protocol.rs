//! The wire protocol between the collector service and its clients (the UI, `--status`).
//!
//! One JSON object per line, one request per connection. Deliberately tiny and stable: the UI is written against
//! these exact field names (see `tests/protocol.rs`). Unknown fields in a request are ignored, so a newer client
//! can still talk to an older service.

use serde::{Deserialize, Serialize};

/// A request longer than this is refused before it is parsed.
pub const MAX_REQUEST_BYTES: usize = 4096;
/// A response longer than this is refused by the client.
pub const MAX_RESPONSE_BYTES: usize = 64 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("empty message")]
    Empty,
    #[error("malformed message: {0}")]
    Malformed(String),
    #[error("message too large ({size} bytes, limit {limit})")]
    TooLarge { size: usize, limit: usize },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "request", rename_all = "snake_case")]
pub enum Request {
    /// Is the service alive? Answered with its version.
    Ping,
    /// A snapshot of what the collector is doing.
    Status,
    /// Confirm the product of the program at path as a known one (docs/KNOWN-PROGRAMS-DESIGN.md). The service builds the
    /// product from the identity it measured for that file, never from anything the client says about the signer.
    KnownProductAdd {
        path: String,
        label: String,
        #[serde(default)]
        note: Option<String>,
    },
    /// Remove a known product.
    KnownProductRemove { id: i64 },
    /// What the owner dataset is and whether a download is under way. Makes no network request.
    OwnerDataStatus,
    /// Start downloading the owner dataset (docs/OWNER-PLAN.md) and answer at once with the status. The address downloaded
    /// from is compiled into the service: a request cannot name one.
    OwnerDataRefresh,
}

// A status reply is one small message every few seconds; boxing the report would only add noise at every use.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "response", rename_all = "snake_case")]
pub enum Response {
    Pong {
        version: String,
    },
    Status(StatusReport),
    /// The known product was saved (or updated); its id.
    KnownProductSaved {
        id: i64,
    },
    /// The owner dataset and the state of its download.
    OwnerData(OwnerDataReport),
    /// The request was carried out.
    Done,
    Error {
        message: String,
    },
}

/// The owner dataset as stored now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnerDatasetReport {
    /// Where it was downloaded from (a host name).
    pub source: String,
    pub downloaded_at_micros: i64,
    /// Ranges of addresses it holds.
    pub rows: u32,
}

/// What the service knows about the owner dataset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnerDataReport {
    /// `None` until a download succeeded.
    pub dataset: Option<OwnerDatasetReport>,
    /// A download is under way now.
    pub refreshing: bool,
    /// Why the last download failed, in plain words; cleared when one succeeds or starts.
    pub last_error: Option<String>,
    /// `false` when the service is configured never to download (`owner_data_allowed = false`).
    pub allowed: bool,
}

/// Counters since the collector started. Plain data, independent of the collector's own types.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectorTotals {
    pub ticks: u64,
    pub events_stored: u64,
    pub flows_stored: u64,
    pub metrics_samples: u64,
    pub source_errors: u64,
    pub metrics_errors: u64,
    pub storage_errors: u64,
}

/// Health of the live ETW trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EtwStatus {
    pub received: u64,
    /// Events lost because the collector fell behind. Anything above zero deserves attention.
    pub dropped: u64,
    pub parse_errors: u64,
    pub max_lag_ms: i64,
    pub future_events: u64,
    /// Seconds since ETW last delivered an event. A trace can be "started" and still deliver nothing, which no other
    /// counter reveals. Absent in replies from older services, which reads as 0.
    #[serde(default)]
    pub silent_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusReport {
    pub version: String,
    pub pid: u32,
    pub started_at_micros: i64,
    pub uptime_secs: u64,
    pub database_path: String,
    pub etw_enabled: bool,
    /// When the collector last finished a pass; the UI uses this for "data freshness".
    pub last_tick_at_micros: Option<i64>,
    pub totals: CollectorTotals,
    /// Absent when the collector runs without ETW.
    pub etw: Option<EtwStatus>,
    /// The database and its drive, as last measured by the storage guard. Absent in replies from older services, and
    /// before the guard's first check.
    #[serde(default)]
    pub storage: Option<StorageStatus>,
}

/// How the storage guard is coping with the data drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageLevel {
    /// Within every limit.
    Ok,
    /// Over the size cap or short of free space: the oldest rolled-up detail is being trimmed early.
    Trimming,
    /// Free space is below the hard floor: detail collection is paused (rollups and status continue).
    Paused,
}

/// The database and its drive, as the collector last measured them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageStatus {
    pub level: StorageLevel,
    pub database_file_bytes: u64,
    /// Bytes of live data in the file (the rest is free pages not yet returned to the drive).
    pub database_live_bytes: u64,
    pub drive_free_bytes: u64,
    /// The oldest row of detail still stored, microseconds since the Unix epoch.
    pub oldest_detail_micros: Option<i64>,
    /// The oldest hourly rollup, microseconds since the Unix epoch.
    pub oldest_rollup_micros: Option<i64>,
    /// One-hour steps of detail the guard has trimmed early since the service started.
    pub trimmed_hours_total: u64,
}

fn check_size(bytes: &[u8], limit: usize) -> Result<(), ProtocolError> {
    if bytes.len() > limit {
        return Err(ProtocolError::TooLarge { size: bytes.len(), limit });
    }
    Ok(())
}

fn trimmed(bytes: &[u8]) -> Result<&[u8], ProtocolError> {
    let trimmed = bytes.trim_ascii();
    if trimmed.is_empty() {
        return Err(ProtocolError::Empty);
    }
    Ok(trimmed)
}

pub fn decode_request(bytes: &[u8]) -> Result<Request, ProtocolError> {
    check_size(bytes, MAX_REQUEST_BYTES)?;
    serde_json::from_slice(trimmed(bytes)?).map_err(|e| ProtocolError::Malformed(e.to_string()))
}

pub fn decode_response(bytes: &[u8]) -> Result<Response, ProtocolError> {
    check_size(bytes, MAX_RESPONSE_BYTES)?;
    serde_json::from_slice(trimmed(bytes)?).map_err(|e| ProtocolError::Malformed(e.to_string()))
}

pub fn encode_request(request: &Request) -> String {
    line(serde_json::to_string(request))
}

pub fn encode_response(response: &Response) -> String {
    line(serde_json::to_string(response))
}

/// These types always serialize; the fallback only exists so no code path can panic.
fn line(json: Result<String, serde_json::Error>) -> String {
    let mut text = json.unwrap_or_else(|_| "{\"response\":\"error\",\"message\":\"internal error\"}".to_owned());
    text.push('\n');
    text
}
