//! View models for the History screens: what the hourly rollups say about the machine over days and months.
//! Same conventions as `views.rs`: camelCase on the wire, times in milliseconds since the Unix epoch.
//!
//! The rows are hourly and in UTC, exactly as stored. Grouping them into local-time days is the UI's job (it knows the
//! user's time zone and daylight saving; a stored UTC day could not be regrouped).

use serde::Serialize;
use syntrac_core::storage_guard::StorageStats;
use syntrac_core::{RepoError, RollupRepository, SystemHour, Timestamp};

pub const MIN_HISTORY_SECS: u64 = 3600;
/// Ten years: "all available history" is simply a range longer than the history.
pub const MAX_HISTORY_SECS: u64 = 10 * 365 * 24 * 3600;

pub fn clamp_history_secs(secs: u64) -> u64 {
    secs.clamp(MIN_HISTORY_SECS, MAX_HISTORY_SECS)
}

fn ms(ts: Timestamp) -> i64 {
    ts.as_micros() / 1000
}

fn percent(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 / whole as f64 * 100.0
    }
}

/// One completed hour of the machine as a whole.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemHourView {
    /// The start of the hour (UTC), milliseconds since the Unix epoch.
    pub hour_ms: i64,
    /// How many samples the hour is built from; the UI weights averages by it when it groups hours into days.
    pub samples: u32,
    pub cpu_avg_percent: f64,
    pub cpu_max_percent: f64,
    pub memory_avg_percent: f64,
    pub memory_max_percent: f64,
    pub net_received_bytes: u64,
    pub net_sent_bytes: u64,
    pub disk_read_bytes: u64,
    pub disk_write_bytes: u64,
    /// CPU used by processes that are not individually tracked.
    pub others_cpu_avg_percent: f64,
}

impl From<SystemHour> for SystemHourView {
    fn from(hour: SystemHour) -> Self {
        Self {
            hour_ms: ms(hour.hour),
            samples: hour.samples,
            cpu_avg_percent: hour.cpu_avg,
            cpu_max_percent: hour.cpu_max,
            memory_avg_percent: percent(hour.memory_used_avg, hour.memory_total),
            memory_max_percent: percent(hour.memory_used_max, hour.memory_total),
            net_received_bytes: hour.net_received,
            net_sent_bytes: hour.net_sent,
            disk_read_bytes: hour.disk_read,
            disk_write_bytes: hour.disk_write,
            others_cpu_avg_percent: hour.others_cpu_avg,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemHistoryView {
    pub from_ms: i64,
    pub to_ms: i64,
    /// Where the history begins (the oldest rollup), whatever range was asked for; `None` before the first rollup.
    pub history_starts_ms: Option<i64>,
    /// Completed hours only, oldest first. A missing hour is a gap (the collector was down or paused), never a zero.
    pub hours: Vec<SystemHourView>,
}

pub fn system_history<R>(repo: &R, now: Timestamp, range_secs: u64) -> Result<SystemHistoryView, RepoError>
where
    R: RollupRepository + StorageStats,
{
    let window = i64::try_from(clamp_history_secs(range_secs).saturating_mul(1_000_000)).unwrap_or(i64::MAX);
    let from = Timestamp::from_micros(now.as_micros().saturating_sub(window));
    let hours = repo.system_hours(from, now)?.into_iter().map(SystemHourView::from).collect();
    Ok(SystemHistoryView {
        from_ms: ms(from),
        to_ms: ms(now),
        history_starts_ms: StorageStats::usage(repo)?.oldest_rollup.map(ms),
        hours,
    })
}
