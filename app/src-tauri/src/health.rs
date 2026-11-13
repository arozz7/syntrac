//! Is the collector healthy? Combines what the pipe says with how old the newest data is, so the banner can tell
//! "running", "running but silent", "not running" and "cannot tell" apart.

use serde::Serialize;
use syntrac_core::Timestamp;
use syntrac_ipc::pipe::ClientError;
use syntrac_ipc::protocol::StatusReport;

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
            let (state, message) = match (data_age_secs, silent, dropped) {
                (Some(age), _, _) if age > STALE_AFTER_SECS => {
                    (HealthState::Stale, format!("The collector is running but its newest data is {age} s old."))
                }
                (_, Some(silent), _) if silent >= ETW_SILENT_DEGRADED_AFTER_SECS => (
                    HealthState::Degraded,
                    format!(
                        "ETW has delivered no events for {silent} s, so process and network activity is not being \
                         recorded. System metrics are still collected."
                    ),
                ),
                (_, _, Some(dropped)) if dropped > 0 => (
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
