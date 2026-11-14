//! View models for the Programs history: which programs used the most of something over a range, and one program's hours.
//! Same conventions as `history.rs`: camelCase on the wire, times in milliseconds since the Unix epoch.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use syntrac_core::rollup::hour_start;
use syntrac_core::storage_guard::StorageStats;
use syntrac_core::{
    all_programs, collect_totals, group_programs, install_root, product_key, rank_groups, rank_totals,
    IdentityRepository, KnownProductRepository, ProgramMetric, ProgramSummary, ProgramTotals, RepoError,
    RollupRepository, Timestamp, TrendRepository,
};

use crate::history::{clamp_history_secs, ms, range_start};
use crate::identity_view::{
    badge, detail, merged_badge, product_name, Identities, IdentityBadgeView, IdentityDetailView,
};
use crate::known_view::{merged_standing, StandingView};
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

/// Whether programs of one product (the four Acme executables) are shown as one row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Grouping {
    #[default]
    None,
    ByProduct,
}

/// One program of a product row: enough to name it and open it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramMemberView {
    pub name: String,
    pub path: String,
    pub identity: IdentityBadgeView,
    /// Where it stands against the user's known programs.
    pub standing: StandingView,
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
    /// Who made it and where it runs from, as far as the service has looked at the file.
    pub identity: IdentityBadgeView,
    /// Where it stands against the user's known programs.
    pub standing: StandingView,
    /// The programs of a product row (empty for an ordinary row). A product row has no path.
    pub members: Vec<ProgramMemberView>,
}

/// An ordinary row: one program (or the unknown-program bucket).
fn plain_row(program: ProgramSummary, identities: &Identities) -> ProgramRowView {
    let path = program.exe.map(|exe| exe.path);
    let identity = path.as_deref().map_or_else(|| badge("", None), |p| badge(p, identities.of(p)));
    let standing = path.as_deref().map_or_else(StandingView::unlisted, |p| identities.standing(p));
    ProgramRowView {
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
        identity,
        standing,
        members: Vec::new(),
    }
}

/// A product row: the members added up, named by what they agree on.
fn product_row(merged: ProgramSummary, members: Vec<ProgramSummary>, identities: &Identities) -> ProgramRowView {
    let member_rows: Vec<ProgramMemberView> = members
        .into_iter()
        .filter_map(|member| member.exe)
        .map(|exe| ProgramMemberView {
            name: display_name(&exe.path, 0),
            identity: badge(&exe.path, identities.of(&exe.path)),
            standing: identities.standing(&exe.path),
            path: exe.path,
        })
        .collect();
    let badges: Vec<IdentityBadgeView> = member_rows.iter().map(|m| m.identity.clone()).collect();
    let paths: Vec<&str> = member_rows.iter().map(|m| m.path.as_str()).collect();
    let mut row = plain_row(merged, identities);
    row.name = product_name(&paths, &badges);
    row.identity = merged_badge(&badges);
    let standings: Vec<StandingView> = member_rows.iter().map(|m| m.standing.clone()).collect();
    row.standing = merged_standing(&standings);
    row.members = member_rows;
    row
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

/// The rows of a ranking: one per program, or (grouped) one per product with several programs.
fn ranked_rows<R>(
    repo: &R,
    totals: &ProgramTotals,
    metric: ProgramMetric,
    limit: usize,
    grouping: Grouping,
    identities: &Identities,
) -> Result<Vec<ProgramRowView>, RepoError>
where
    R: RollupRepository,
{
    if grouping == Grouping::None {
        let top = rank_totals(repo, totals, metric, limit)?;
        return Ok(top.into_iter().map(|program| plain_row(program, identities)).collect());
    }
    // A program whose file has not been read has no known maker: a shared folder alone is only a guess, and folding on it
    // would come apart again as the files are read. It stays on its own row until it has facts.
    let product_of = |exe: &syntrac_core::Executable| {
        let facts = identities.of(&exe.path).and_then(|record| record.facts.as_ref())?;
        product_key(&exe.path, Some(facts))
    };
    let groups = rank_groups(group_programs(all_programs(repo, totals)?, product_of), metric, limit);
    let rows: Vec<ProgramRowView> = groups
        .into_iter()
        .map(|group| {
            if group.members.len() > 1 {
                product_row(group.summary, group.members, identities)
            } else {
                plain_row(group.summary, identities)
            }
        })
        .collect();
    Ok(tell_apart(rows))
}

/// Two product rows can share a name (Windows programs signed under two different signer names are both Microsoft Windows
/// Operating System). A row that shares its name says who made it, or else where it lives, so the rows can be told apart.
fn tell_apart(mut rows: Vec<ProgramRowView>) -> Vec<ProgramRowView> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    for row in rows.iter().filter(|row| !row.members.is_empty()) {
        *seen.entry(row.name.clone()).or_default() += 1;
    }
    for row in rows.iter_mut().filter(|row| !row.members.is_empty()) {
        if seen.get(&row.name).copied().unwrap_or(0) < 2 {
            continue;
        }
        let qualifier = row
            .identity
            .publisher
            .clone()
            .or_else(|| row.members.first().and_then(|member| install_root(&member.path)));
        if let Some(qualifier) = qualifier {
            row.name = format!("{} ({qualifier})", row.name);
        }
    }
    rows
}

/// Ranks collected sums by one metric.
pub fn rank_range<R>(
    repo: &R,
    range: &RangeTotals,
    metric: ProgramMetricParam,
    limit: usize,
    grouping: Grouping,
) -> Result<ProgramsView, RepoError>
where
    R: RollupRepository + StorageStats + IdentityRepository + KnownProductRepository,
{
    let limit = limit.clamp(1, MAX_PROGRAM_ROWS);
    let identities = Identities::load(repo)?;
    let programs = ranked_rows(repo, &range.totals, metric.into(), limit, grouping, &identities)?;
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
    grouping: Grouping,
) -> Result<ProgramsView, RepoError>
where
    R: RollupRepository + TrendRepository + StorageStats + IdentityRepository + KnownProductRepository,
{
    rank_range(repo, &range_totals(repo, now, range_secs)?, metric, limit, grouping)
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
    /// What the service found out about the file (or that it has not looked yet).
    pub identity: IdentityDetailView,
    /// Where it stands against the user's known programs.
    pub standing: StandingView,
    pub hours: Vec<ProgramHourView>,
}

pub fn program_history<R>(
    repo: &R,
    now: Timestamp,
    range_secs: u64,
    path: &str,
) -> Result<ProgramHistoryView, RepoError>
where
    R: RollupRepository + IdentityRepository + KnownProductRepository,
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
        identity: detail(path, repo.identity(path)?.as_ref()),
        standing: Identities::load(repo)?.standing(path),
        hours,
    })
}
