//! Is the collector healthy? Combines what the pipe says with how old the newest data is, so the banner can tell
//! "running", "running but silent", "not running" and "cannot tell" apart.

use serde::Serialize;
use syntrac_core::Timestamp;
use syntrac_ipc::pipe::ClientError;
use syntrac_ipc::protocol::{StatusReport, StorageLevel, StorageStatus};

/// Data older than this, from a collector that answers, is called stale.
pub const STALE_AFTER_SECS: u64 = 60;

/// ETW silent for this long, from a collector that is otherwise alive, is called degraded. It is shorter than the
/// service's own restart threshold (180 s) so the banner speaks up before the service tries to repair it.
pub const ETW_SILENT_DEGRADED_AFTER_SECS: u64 = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthState {
    Running,
    /// The collector answers but has not written recently.
    Stale,
    /// The collector is alive and writing, but a part of it is not working (ETW delivers nothing), so some activity
    /// is not being recorded.
    Degraded,
    /// Nothing is listening on the collector's pipe.
    Offline,
    /// The pipe could not be asked (access denied, timeout, ...).
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthView {
    pub state: HealthState,
    pub message: String,
    pub version: Option<String>,
    pub pid: Option<u32>,
    pub uptime_secs: Option<u64>,
    pub etw_dropped: Option<u64>,
    pub etw_silent_secs: Option<u64>,
    pub data_age_secs: Option<u64>,
    /// The database and its drive, when the collector reports them.
    pub storage: Option<StorageView>,
}

impl HealthView {
    fn bare(state: HealthState, message: String, data_age_secs: Option<u64>) -> Self {
        Self {
            state,
            message,
            version: None,
            pid: None,
            uptime_secs: None,
            etw_dropped: None,
            etw_silent_secs: None,
            data_age_secs,
            storage: None,
        }
    }

    pub fn unknown(message: &str) -> Self {
        Self::bare(HealthState::Unknown, message.to_owned(), None)
    }
}

pub fn assess_health(
    pipe: Result<StatusReport, ClientError>,
    newest_data: Option<Timestamp>,
    now: Timestamp,
) -> HealthView {
    // A sample stamped slightly in the future (clock jitter) counts as age zero, never negative.
    let data_age_secs = newest_data
        .map(|ts| u64::try_from(now.as_micros().saturating_sub(ts.as_micros()).max(0) / 1_000_000).unwrap_or(0));
    let age_note = data_age_secs
        .map_or_else(|| "No data has been written yet.".to_owned(), |age| format!("Newest data is {age} s old."));

    match pipe {
        Ok(report) => {
            let dropped = report.etw.map(|etw| etw.dropped);
            let silent = report.etw.map(|etw| etw.silent_secs);
            let storage_level = report.storage.map(|storage| storage.level);
            let (state, message) = match (data_age_secs, storage_level, silent, dropped) {
                (Some(age), _, _, _) if age > STALE_AFTER_SECS => {
                    (HealthState::Stale, format!("The collector is running but its newest data is {age} s old."))
                }
                (_, Some(StorageLevel::Paused), _, _) => (
                    HealthState::Degraded,
                    "Detail collection is paused because the data drive is nearly full. Process starts are still noted and \
                     the hourly summaries continue, but network, DNS and per-process detail is not being recorded."
                        .to_owned(),
                ),
                (_, _, Some(silent), _) if silent >= ETW_SILENT_DEGRADED_AFTER_SECS => (
                    HealthState::Degraded,
                    format!(
                        "ETW has delivered no events for {silent} s, so process and network activity is not being \
                         recorded. System metrics are still collected."
                    ),
                ),
                (_, Some(StorageLevel::Trimming), _, _) => (
                    HealthState::Degraded,
                    "The database or its drive is getting full, so the oldest detail is being trimmed early. It is already \
                     summarised in the hourly rollups, which are kept."
                        .to_owned(),
                ),
                (_, _, _, Some(dropped)) if dropped > 0 => (
                    HealthState::Running,
                    format!("The collector is running, but {dropped} ETW events were dropped because it fell behind."),
                ),
                _ => (HealthState::Running, "The collector is running.".to_owned()),
            };
            HealthView {
                state,
                message,
                version: Some(report.version),
                pid: Some(report.pid),
                uptime_secs: Some(report.uptime_secs),
                etw_dropped: dropped,
                etw_silent_secs: silent,
                data_age_secs,
                storage: report.storage.map(StorageView::from),
            }
        }
        Err(ClientError::NotRunning) => {
            HealthView::bare(HealthState::Offline, format!("The collector is not running. {age_note}"), data_age_secs)
        }
        Err(ClientError::AccessDenied) => HealthView::bare(
            HealthState::Unknown,
            format!("Access to the collector was denied for this user. {age_note}"),
            data_age_secs,
        ),
        Err(other) => HealthView::bare(
            HealthState::Unknown,
            format!("Could not ask the collector how it is doing ({other}). {age_note}"),
            data_age_secs,
        ),
    }
}

/// How full the database and its drive are, for the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageView {
    pub level: StorageLevelView,
    pub database_live_bytes: u64,
    pub database_file_bytes: u64,
    pub drive_free_bytes: u64,
    /// Milliseconds since the Unix epoch.
    pub oldest_detail_ms: Option<i64>,
    pub oldest_rollup_ms: Option<i64>,
    pub trimmed_hours_total: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageLevelView {
    Ok,
    Trimming,
    Paused,
}

impl From<StorageStatus> for StorageView {
    fn from(storage: StorageStatus) -> Self {
        Self {
            level: match storage.level {
                StorageLevel::Ok => StorageLevelView::Ok,
                StorageLevel::Trimming => StorageLevelView::Trimming,
                StorageLevel::Paused => StorageLevelView::Paused,
            },
            database_live_bytes: storage.database_live_bytes,
            database_file_bytes: storage.database_file_bytes,
            drive_free_bytes: storage.drive_free_bytes,
            oldest_detail_ms: storage.oldest_detail_micros.map(|micros| micros / 1_000),
            oldest_rollup_ms: storage.oldest_rollup_micros.map(|micros| micros / 1_000),
            trimmed_hours_total: storage.trimmed_hours_total,
        }
    }
}
