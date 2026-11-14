//! Trend queries over the hourly rollups: which programs used the most of something over a range.
//!
//! The store only SUMS (one group-by per table, so a year of rows never reaches the app). Merging those sums, deriving averages
//! and ranking are pure functions here, so the in-memory and SQLite stores cannot disagree about the maths.

use std::collections::BTreeMap;

use crate::ports::RepoError;
use crate::rollup::{Executable, LedgerEntry};
use crate::rollup_run::RollupRepository;
use crate::time::Timestamp;

/// What a ranking is ordered by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramMetric {
    CpuAverage,
    MemoryPeak,
    Starts,
    /// Bytes sent plus received, not counting traffic to this machine itself.
    NetworkBytes,
}

/// One executable's process rollups, summed over a range. Only hours that also have a system row count (their samples are the
/// weights), so a gap in the data is not counted as zero.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessTotals {
    /// `None`: processes whose executable is unknown.
    pub exe: Option<Executable>,
    pub hours_present: u32,
    pub samples_present: u64,
    /// The sum over hours of (the executable's CPU average for the hour x the samples in that hour).
    pub cpu_weighted: f64,
    pub cpu_peak: f64,
    pub memory_peak: u64,
    pub starts: u64,
}

/// One executable's network rollups, summed over a range. Traffic to loopback addresses is not included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrafficTotals {
    pub exe: Option<Executable>,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub flows: u64,
    /// Distinct remote addresses over the whole range.
    pub destinations: u32,
}

/// The read side of trends. Implementations only group and sum.
pub trait TrendRepository {
    /// The samples behind every system hour in `[from, to]`: the denominator of every CPU average.
    fn system_sample_total(&self, from: Timestamp, to: Timestamp) -> Result<u64, RepoError>;

    /// Per executable, over the hours in `[from, to]` that have a system row.
    fn process_totals(&self, from: Timestamp, to: Timestamp) -> Result<Vec<ProcessTotals>, RepoError>;

    /// Per executable, over the hours in `[from, to]`, leaving out loopback destinations.
    fn traffic_totals(&self, from: Timestamp, to: Timestamp) -> Result<Vec<TrafficTotals>, RepoError>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProgramSummary {
    pub exe: Option<Executable>,
    /// Averaged over EVERY sample of the range (a program that was not among the heaviest processes counts as zero then).
    pub cpu_avg_percent: f64,
    pub cpu_peak_percent: f64,
    pub memory_peak_bytes: u64,
    pub starts: u64,
    /// In how many hours the program was recorded among the heaviest processes.
    pub hours_present: u32,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub flows: u64,
    pub destinations: u32,
    /// Permanent first and last sighting; filled by [`top_programs`].
    pub ledger: Option<LedgerEntry>,
}

impl ProgramSummary {
    fn empty(exe: Option<Executable>) -> Self {
        Self {
            exe,
            cpu_avg_percent: 0.0,
            cpu_peak_percent: 0.0,
            memory_peak_bytes: 0,
            starts: 0,
            hours_present: 0,
            bytes_sent: 0,
            bytes_received: 0,
            flows: 0,
            destinations: 0,
            ledger: None,
        }
    }

    /// The number this program is ranked by for metric.
    pub fn value(&self, metric: ProgramMetric) -> f64 {
        match metric {
            ProgramMetric::CpuAverage => self.cpu_avg_percent,
            ProgramMetric::MemoryPeak => self.memory_peak_bytes as f64,
            ProgramMetric::Starts => self.starts as f64,
            ProgramMetric::NetworkBytes => self.bytes_sent.saturating_add(self.bytes_received) as f64,
        }
    }
}

fn key_of(exe: &Option<Executable>) -> Option<String> {
    exe.as_ref().map(|e| e.key.clone())
}

/// One summary per executable from the two kinds of totals. The CPU average divides by the samples of the whole range.
pub fn merge_programs(
    system_samples: u64,
    process: Vec<ProcessTotals>,
    traffic: Vec<TrafficTotals>,
) -> Vec<ProgramSummary> {
    let mut by_key: BTreeMap<Option<String>, ProgramSummary> = BTreeMap::new();
    for totals in process {
        let summary = by_key.entry(key_of(&totals.exe)).or_insert_with(|| ProgramSummary::empty(totals.exe.clone()));
        summary.cpu_avg_percent = if system_samples == 0 { 0.0 } else { totals.cpu_weighted / system_samples as f64 };
        summary.cpu_peak_percent = totals.cpu_peak;
        summary.memory_peak_bytes = totals.memory_peak;
        summary.starts = totals.starts;
        summary.hours_present = totals.hours_present;
    }
    for totals in traffic {
        let summary = by_key.entry(key_of(&totals.exe)).or_insert_with(|| ProgramSummary::empty(totals.exe.clone()));
        summary.bytes_sent = totals.bytes_sent;
        summary.bytes_received = totals.bytes_received;
        summary.flows = totals.flows;
        summary.destinations = totals.destinations;
    }
    by_key.into_values().collect()
}

/// The `limit` highest by `metric`, highest first. A program with nothing for the metric is left out. Ties are broken by path,
/// with the unattributed bucket last, so the order never depends on how the store happened to return its rows.
pub fn rank_programs(mut programs: Vec<ProgramSummary>, metric: ProgramMetric, limit: usize) -> Vec<ProgramSummary> {
    programs.retain(|program| program.value(metric) > 0.0);
    programs.sort_by(|a, b| {
        let by_value = b.value(metric).total_cmp(&a.value(metric));
        let a_key = a.exe.as_ref().map(|e| e.key.as_str());
        let b_key = b.exe.as_ref().map(|e| e.key.as_str());
        // `None` (unattributed) sorts after every named program.
        by_value.then_with(|| match (a_key, b_key) {
            (Some(x), Some(y)) => x.cmp(y),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        })
    });
    programs.truncate(limit);
    programs
}

/// Everything the store sums up for one range. Reading it is the expensive part (a year is millions of rows); ranking it any way
/// is cheap, so a caller that ranks the same range several ways can keep this and rank it again.
#[derive(Debug, Clone, PartialEq)]
pub struct ProgramTotals {
    pub system_samples: u64,
    pub process: Vec<ProcessTotals>,
    pub traffic: Vec<TrafficTotals>,
}

/// Reads the sums for `[from, to]`.
pub fn collect_totals<R: TrendRepository>(
    repo: &R,
    from: Timestamp,
    to: Timestamp,
) -> Result<ProgramTotals, RepoError> {
    Ok(ProgramTotals {
        system_samples: repo.system_sample_total(from, to)?,
        process: repo.process_totals(from, to)?,
        traffic: repo.traffic_totals(from, to)?,
    })
}

/// The `limit` highest of already-collected sums, with when each was first and last seen.
pub fn rank_totals<R: RollupRepository>(
    repo: &R,
    totals: &ProgramTotals,
    metric: ProgramMetric,
    limit: usize,
) -> Result<Vec<ProgramSummary>, RepoError> {
    let merged = merge_programs(totals.system_samples, totals.process.clone(), totals.traffic.clone());
    let mut top = rank_programs(merged, metric, limit);
    for program in &mut top {
        program.ledger = match &program.exe {
            Some(exe) => repo.executable_ledger(&exe.path)?,
            None => None,
        };
    }
    Ok(top)
}

/// Every program in already-collected sums (none left out for having nothing of one metric), with when each was first and last
/// seen, ordered by path. For callers that fold programs together before ranking.
pub fn all_programs<R: RollupRepository>(repo: &R, totals: &ProgramTotals) -> Result<Vec<ProgramSummary>, RepoError> {
    let mut programs = merge_programs(totals.system_samples, totals.process.clone(), totals.traffic.clone());
    for program in &mut programs {
        program.ledger = match &program.exe {
            Some(exe) => repo.executable_ledger(&exe.path)?,
            None => None,
        };
    }
    Ok(programs)
}

/// The programs that used the most of `metric` between `from` and `to`, with when each was first and last seen.
pub fn top_programs<R: TrendRepository + RollupRepository>(
    repo: &R,
    from: Timestamp,
    to: Timestamp,
    metric: ProgramMetric,
    limit: usize,
) -> Result<Vec<ProgramSummary>, RepoError> {
    rank_totals(repo, &collect_totals(repo, from, to)?, metric, limit)
}
