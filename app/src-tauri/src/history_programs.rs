//! View models for the Programs history: which programs used the most of something over a range, and one program's hours.
//! Same conventions as `history.rs`: camelCase on the wire, times in milliseconds since the Unix epoch.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use syntrac_core::rollup::hour_start;
use syntrac_core::storage_guard::StorageStats;
use syntrac_core::{
    collect_totals, rank_totals, ProgramMetric, ProgramSummary, ProgramTotals, RepoError, RollupRepository, Timestamp,
    TrendRepository,
};

use crate::history::{clamp_history_secs, ms, range_start};
use crate::names::display_name;

pub const UNKNOWN_PROGRAM: &str = "Unknown program";
pub const DEFAULT_PROGRAM_ROWS: usize = 25;
pub const MAX_PROGRAM_ROWS: usize = 200;

/// The metric, as the UI names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramMetricParam {
    Cpu,
    Memory,
    Starts,
    Network,
}

impl From<ProgramMetricParam> for ProgramMetric {
    fn from(param: ProgramMetricParam) -> Self {
        match param {
            ProgramMetricParam::Cpu => ProgramMetric::CpuAverage,
            ProgramMetricParam::Memory => ProgramMetric::MemoryPeak,
            ProgramMetricParam::Starts => ProgramMetric::Starts,
            ProgramMetricParam::Network => ProgramMetric::NetworkBytes,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramRowView {
    pub name: String,
    /// `None` for the bucket of processes whose program is unknown.
    pub path: Option<String>,
    /// Averaged over every sample of the range: a program that was not among the heaviest processes counts as zero then.
    pub cpu_avg_percent: f64,
    pub cpu_peak_percent: f64,
    pub memory_peak_bytes: u64,
    pub starts: u64,
    /// In how many hours the program was recorded among the heaviest processes (out of `hoursInRange`).
    pub hours_present: u32,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub destinations: u32,
    pub first_seen_ms: Option<i64>,
    pub last_seen_ms: Option<i64>,
}

impl From<ProgramSummary> for ProgramRowView {
    fn from(program: ProgramSummary) -> Self {
        let path = program.exe.map(|exe| exe.path);
        Self {
            name: path.as_deref().map_or_else(|| UNKNOWN_PROGRAM.to_owned(), |p| display_name(p, 0)),
            path,
            cpu_avg_percent: program.cpu_avg_percent,
            cpu_peak_percent: program.cpu_peak_percent,
            memory_peak_bytes: program.memory_peak_bytes,
            starts: program.starts,
            hours_present: program.hours_present,
            bytes_sent: program.bytes_sent,
            bytes_received: program.bytes_received,
            destinations: program.destinations,
            first_seen_ms: program.ledger.map(|l| ms(l.first_seen)),
            last_seen_ms: program.ledger.map(|l| ms(l.last_seen)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramsView {
    pub from_ms: i64,
    pub to_ms: i64,
    pub history_starts_ms: Option<i64>,
    /// Hours of the range that have data at all: the denominator for each program's `hoursPresent`.
    pub hours_in_range: u32,
    pub programs: Vec<ProgramRowView>,
}

/// What the store summed for one range: the expensive part of a ranking. Ranking it by any metric afterwards is cheap, so the
/// backend keeps it until a new hour is rolled up (see `Backend::top_programs`).
#[derive(Debug, Clone, PartialEq)]
pub struct RangeTotals {
    pub from: Timestamp,
    pub to: Timestamp,
    /// Hours of the range that have data at all.
    pub hours_in_range: u32,
    pub totals: ProgramTotals,
}

/// Reads the sums for the range. The start is moved back to a whole hour, so asking again within the hour reads the same range.
pub fn range_totals<R>(repo: &R, now: Timestamp, range_secs: u64) -> Result<RangeTotals, RepoError>
where
    R: RollupRepository + TrendRepository,
{
    let from = hour_start(range_start(now, range_secs));
    Ok(RangeTotals {
        from,
        to: now,
        hours_in_range: u32::try_from(repo.system_hours(from, now)?.len()).unwrap_or(u32::MAX),
        totals: collect_totals(repo, from, now)?,
    })
}

/// Ranks collected sums by one metric.
pub fn rank_range<R>(
    repo: &R,
    range: &RangeTotals,
    metric: ProgramMetricParam,
    limit: usize,
) -> Result<ProgramsView, RepoError>
where
    R: RollupRepository + StorageStats,
{
    let limit = limit.clamp(1, MAX_PROGRAM_ROWS);
    let programs =
        rank_totals(repo, &range.totals, metric.into(), limit)?.into_iter().map(ProgramRowView::from).collect();
    Ok(ProgramsView {
        from_ms: ms(range.from),
        to_ms: ms(range.to),
        history_starts_ms: StorageStats::usage(repo)?.oldest_rollup.map(ms),
        hours_in_range: range.hours_in_range,
        programs,
    })
}

pub fn program_ranking<R>(
    repo: &R,
    now: Timestamp,
    range_secs: u64,
    metric: ProgramMetricParam,
    limit: usize,
) -> Result<ProgramsView, RepoError>
where
    R: RollupRepository + TrendRepository + StorageStats,
{
    rank_range(repo, &range_totals(repo, now, range_secs)?, metric, limit)
}

/// One hour of one program. Present in every hour that has system data: a program that was not recorded in an hour has
/// zeros and `present: false`, so averaging hours into days weights them the way the ranking does.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramHourView {
    pub hour_ms: i64,
    /// The samples behind the hour (the weight when hours are grouped into days).
    pub samples: u32,
    pub present: bool,
    pub cpu_avg_percent: f64,
    pub cpu_peak_percent: f64,
    pub memory_peak_bytes: u64,
    pub starts: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramHistoryView {
    pub name: String,
    pub path: String,
    pub from_ms: i64,
    pub to_ms: i64,
    pub first_seen_ms: Option<i64>,
    pub last_seen_ms: Option<i64>,
    pub hours: Vec<ProgramHourView>,
}

pub fn program_history<R>(
    repo: &R,
    now: Timestamp,
    range_secs: u64,
    path: &str,
) -> Result<ProgramHistoryView, RepoError>
where
    R: RollupRepository,
{
    let from = range_start(now, clamp_history_secs(range_secs));
    let rows = repo.executable_hours(path, from, now)?;
    // The path is matched without regard to case; show it the way it was first recorded.
    let path = rows.iter().find_map(|row| row.exe.as_ref().map(|exe| exe.path.as_str())).unwrap_or(path).to_owned();
    let path = path.as_str();
    let mut own: HashMap<i64, _> = rows.into_iter().map(|row| (row.hour.as_micros(), row)).collect();
    let hours = repo
        .system_hours(from, now)?
        .into_iter()
        .map(|system| {
            let row = own.remove(&system.hour.as_micros());
            ProgramHourView {
                hour_ms: ms(system.hour),
                samples: system.samples,
                present: row.is_some(),
                cpu_avg_percent: row.as_ref().map_or(0.0, |r| r.cpu_avg),
                cpu_peak_percent: row.as_ref().map_or(0.0, |r| r.cpu_peak),
                memory_peak_bytes: row.as_ref().map_or(0, |r| r.memory_peak),
                starts: row.as_ref().map_or(0, |r| r.starts),
            }
        })
        .collect();
    let ledger = repo.executable_ledger(path)?;
    Ok(ProgramHistoryView {
        name: display_name(path, 0),
        path: path.to_owned(),
        from_ms: ms(from),
        to_ms: ms(now),
        first_seen_ms: ledger.map(|l| ms(l.first_seen)),
        last_seen_ms: ledger.map(|l| ms(l.last_seen)),
        hours,
    })
}
