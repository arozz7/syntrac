//! View models for the Network history: where the machine talked to over a range, which destinations are new, and one
//! destination's history. Same conventions as `history_programs.rs`: camelCase on the wire, times in milliseconds since the
//! Unix epoch.
//!
//! A DNS name here is what some program ASKED for when it reached the address: never an owner. Names come from the permanent
//! address-to-name table, so they are still there after the detail has been purged.

use std::collections::BTreeMap;
use std::net::IpAddr;

use serde::Serialize;
use syntrac_core::rollup::HOUR_MICROS;
use syntrac_core::storage_guard::StorageStats;
use syntrac_core::{
    classify_addr, new_destination_cutoff, rank_destination_programs, rank_destinations, select_new,
    top_destination_programs, AddrClass, AddressScope, DestinationOrder, DestinationProgram, DestinationTotals,
    DestinationTrendRepository, Executable, RepoError, RollupRepository, Timestamp,
};

use crate::history::{clamp_history_secs, ms, range_start};
pub use crate::history_programs::UNKNOWN_PROGRAM;
use crate::names::display_name;

/// What destinations are ranked by, as the UI names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DestinationOrderParam {
    Bytes,
    Flows,
    Programs,
}

impl From<DestinationOrderParam> for DestinationOrder {
    fn from(param: DestinationOrderParam) -> Self {
        match param {
            DestinationOrderParam::Bytes => DestinationOrder::Bytes,
            DestinationOrderParam::Flows => DestinationOrder::Flows,
            DestinationOrderParam::Programs => DestinationOrder::Programs,
        }
    }
}

pub const DEFAULT_DESTINATION_ROWS: usize = 25;
pub const MAX_DESTINATION_ROWS: usize = 200;

/// The class of an address as the UI names it.
fn class_name(class: AddrClass) -> &'static str {
    match class {
        AddrClass::Public => "public",
        AddrClass::Private => "private",
        AddrClass::LinkLocal => "link_local",
        AddrClass::Multicast => "multicast",
        AddrClass::Other => "other",
    }
}

/// The most recently asked name of an address, and how many other names it has.
fn names_summary<R: RollupRepository>(repo: &R, addr: IpAddr) -> Result<(Option<String>, u32), RepoError> {
    let names = repo.address_names(addr)?;
    let other = u32::try_from(names.len().saturating_sub(1)).unwrap_or(u32::MAX);
    Ok((names.into_iter().next().map(|n| n.name), other))
}

fn program_name(exe: &Option<Executable>) -> (String, Option<String>) {
    match exe {
        Some(exe) => (display_name(&exe.path, 0), Some(exe.path.clone())),
        None => (UNKNOWN_PROGRAM.to_owned(), None),
    }
}

// ---- the ranking

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DestinationRowView {
    pub addr: String,
    /// The name asked for most recently, when any was recorded.
    pub name: Option<String>,
    pub other_names: u32,
    pub class: &'static str,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub flows: u64,
    /// How many different programs talked to it.
    pub programs: u32,
    /// The biggest of those programs by traffic (at most `PROGRAMS_PER_ROW`), so a row says who without a click.
    pub top_programs: Vec<RowProgramView>,
    /// How many programs are not in `top_programs`.
    pub more_programs: u32,
    pub first_seen_ms: Option<i64>,
    pub last_seen_ms: Option<i64>,
}

/// A program named in a row of the ranking.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RowProgramView {
    pub name: String,
    /// `None` for the bucket of programs that could not be identified.
    pub path: Option<String>,
}

/// How many programs a row names before saying "+N more".
pub const PROGRAMS_PER_ROW: usize = 3;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DestinationsView {
    pub from_ms: i64,
    pub to_ms: i64,
    pub history_starts_ms: Option<i64>,
    /// Hours of the range that have data at all.
    pub hours_in_range: u32,
    pub destinations: Vec<DestinationRowView>,
}

/// What the store summed for one range: the expensive part of a ranking. Ranking it any way afterwards is cheap, so the backend
/// keeps it until a new hour is rolled up (see `Backend::top_destinations`).
#[derive(Debug, Clone, PartialEq)]
pub struct DestinationRange {
    pub from: Timestamp,
    pub to: Timestamp,
    pub hours_in_range: u32,
    pub totals: Vec<DestinationTotals>,
}

/// Reads the sums for the range. The start is moved back to a whole hour, so asking again within the hour reads the same range.
pub fn destination_range<R>(repo: &R, now: Timestamp, range_secs: u64) -> Result<DestinationRange, RepoError>
where
    R: RollupRepository + DestinationTrendRepository,
{
    let from = syntrac_core::rollup::hour_start(range_start(now, range_secs));
    Ok(DestinationRange {
        from,
        to: now,
        hours_in_range: u32::try_from(repo.system_hours(from, now)?.len()).unwrap_or(u32::MAX),
        totals: repo.destination_totals(from, now)?,
    })
}

/// Ranks collected sums.
pub fn rank_destination_range<R>(
    repo: &R,
    range: &DestinationRange,
    order: DestinationOrder,
    scope: AddressScope,
    limit: usize,
) -> Result<DestinationsView, RepoError>
where
    R: RollupRepository + DestinationTrendRepository + StorageStats,
{
    let limit = limit.clamp(1, MAX_DESTINATION_ROWS);
    let ranked = rank_destinations(range.totals.clone(), order, scope, limit);
    // One read for the whole page, not one per row.
    let addrs: Vec<IpAddr> = ranked.iter().map(|row| row.addr).collect();
    let mut by_addr: BTreeMap<IpAddr, Vec<DestinationProgram>> = BTreeMap::new();
    for (addr, program) in repo.programs_of_destinations(&addrs, range.from, range.to)? {
        by_addr.entry(addr).or_default().push(program);
    }
    let mut destinations = Vec::new();
    for row in ranked {
        let (name, other_names) = names_summary(repo, row.addr)?;
        let ledger = repo.destination_ledger(row.addr)?;
        let (top, more_programs) =
            top_destination_programs(by_addr.remove(&row.addr).unwrap_or_default(), PROGRAMS_PER_ROW);
        let top_programs = top
            .iter()
            .map(|program| {
                let (name, path) = program_name(&program.exe);
                RowProgramView { name, path }
            })
            .collect();
        destinations.push(DestinationRowView {
            addr: row.addr.to_string(),
            name,
            other_names,
            class: class_name(classify_addr(row.addr)),
            bytes_sent: row.bytes_sent,
            bytes_received: row.bytes_received,
            flows: row.flows,
            programs: row.programs,
            top_programs,
            more_programs,
            first_seen_ms: ledger.map(|l| ms(l.first_seen)),
            last_seen_ms: ledger.map(|l| ms(l.last_seen)),
        });
    }
    Ok(DestinationsView {
        from_ms: ms(range.from),
        to_ms: ms(range.to),
        history_starts_ms: StorageStats::usage(repo)?.oldest_rollup.map(ms),
        hours_in_range: range.hours_in_range,
        destinations,
    })
}

pub fn destination_ranking<R>(
    repo: &R,
    now: Timestamp,
    range_secs: u64,
    order: DestinationOrder,
    scope: AddressScope,
    limit: usize,
) -> Result<DestinationsView, RepoError>
where
    R: RollupRepository + DestinationTrendRepository + StorageStats,
{
    rank_destination_range(repo, &destination_range(repo, now, range_secs)?, order, scope, limit)
}

// ---- what is new

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewDestinationRowView {
    pub addr: String,
    pub name: Option<String>,
    pub other_names: u32,
    pub class: &'static str,
    pub first_seen_ms: i64,
    pub last_seen_ms: i64,
    /// The program that was first to talk to it (none when only unidentified programs ever did).
    pub first_program: Option<String>,
    pub first_program_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewDestinationsView {
    /// Only addresses first seen at or after this count as new (`None`: there is no history yet).
    pub since_ms: Option<i64>,
    /// Everything first seen before this is the baseline, not news (the first day of history).
    pub baseline_until_ms: Option<i64>,
    pub history_starts_ms: Option<i64>,
    pub destinations: Vec<NewDestinationRowView>,
}

/// The destinations first seen within the last `window_secs`, never counting the baseline (the first day of history) as new.
pub fn new_destinations_view<R>(
    repo: &R,
    now: Timestamp,
    window_secs: u64,
    scope: AddressScope,
    limit: usize,
) -> Result<NewDestinationsView, RepoError>
where
    R: RollupRepository + DestinationTrendRepository + StorageStats,
{
    let limit = limit.clamp(1, MAX_DESTINATION_ROWS);
    let history_start = StorageStats::usage(repo)?.oldest_rollup;
    let cutoff = new_destination_cutoff(history_start, range_start(now, window_secs));
    let mut destinations = Vec::new();
    for row in select_new(repo.new_destinations(cutoff)?, cutoff, scope, limit) {
        let (name, other_names) = names_summary(repo, row.addr)?;
        let (first_program, first_program_path) = match &row.first_program {
            Some(_) => {
                let (name, path) = program_name(&row.first_program);
                (Some(name), path)
            }
            None => (None, None),
        };
        destinations.push(NewDestinationRowView {
            addr: row.addr.to_string(),
            name,
            other_names,
            class: class_name(classify_addr(row.addr)),
            first_seen_ms: ms(row.first_seen),
            last_seen_ms: ms(row.last_seen),
            first_program,
            first_program_path,
        });
    }
    Ok(NewDestinationsView {
        since_ms: history_start.map(|_| ms(cutoff)),
        baseline_until_ms: history_start.map(|start| {
            ms(Timestamp::from_micros(
                start.as_micros() + syntrac_core::destination_trend::BASELINE_HOURS * HOUR_MICROS,
            ))
        }),
        history_starts_ms: history_start.map(ms),
        destinations,
    })
}

// ---- one destination

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DestinationNameView {
    pub name: String,
    pub first_seen_ms: i64,
    pub last_seen_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DestinationProgramView {
    pub name: String,
    /// `None` for the bucket of programs that could not be identified.
    pub path: Option<String>,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub flows: u64,
    pub first_seen_ms: i64,
    pub last_seen_ms: i64,
}

/// One hour of traffic to one address, all programs together. Present for every hour that has system data, so a quiet hour is
/// zero (not a gap).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DestinationHourView {
    pub hour_ms: i64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub flows: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DestinationHistoryView {
    pub addr: String,
    pub class: &'static str,
    pub from_ms: i64,
    pub to_ms: i64,
    /// Permanent first and last sighting.
    pub first_seen_ms: Option<i64>,
    pub last_seen_ms: Option<i64>,
    /// Every name asked for when this address was reached, newest first.
    pub names: Vec<DestinationNameView>,
    /// The programs that talked to it within the range, most bytes first.
    pub programs: Vec<DestinationProgramView>,
    pub hours: Vec<DestinationHourView>,
}

pub fn destination_history<R>(
    repo: &R,
    now: Timestamp,
    range_secs: u64,
    addr: &str,
) -> Result<DestinationHistoryView, RepoError>
where
    R: RollupRepository + DestinationTrendRepository,
{
    let addr: IpAddr = addr.trim().parse().map_err(|_| RepoError::Backend(format!("{addr:?} is not an IP address")))?;
    let addr = addr.to_canonical();
    let from = range_start(now, clamp_history_secs(range_secs));

    let mut per_hour: BTreeMap<i64, DestinationHourView> = BTreeMap::new();
    for system in repo.system_hours(from, now)? {
        let hour_ms = ms(system.hour);
        per_hour.insert(hour_ms, DestinationHourView { hour_ms, bytes_sent: 0, bytes_received: 0, flows: 0 });
    }
    for row in repo.destination_hours(addr, from, now)? {
        let hour_ms = ms(row.hour);
        let entry = per_hour.entry(hour_ms).or_insert(DestinationHourView {
            hour_ms,
            bytes_sent: 0,
            bytes_received: 0,
            flows: 0,
        });
        entry.bytes_sent = entry.bytes_sent.saturating_add(row.bytes_sent);
        entry.bytes_received = entry.bytes_received.saturating_add(row.bytes_received);
        entry.flows += u64::from(row.flows);
    }

    let programs = rank_destination_programs(repo.destination_programs(addr, from, now)?)
        .into_iter()
        .map(|program| {
            let (name, path) = program_name(&program.exe);
            DestinationProgramView {
                name,
                path,
                bytes_sent: program.bytes_sent,
                bytes_received: program.bytes_received,
                flows: program.flows,
                first_seen_ms: ms(program.first_seen),
                last_seen_ms: ms(program.last_seen),
            }
        })
        .collect();
    let names = repo
        .address_names(addr)?
        .into_iter()
        .map(|n| DestinationNameView { name: n.name, first_seen_ms: ms(n.first_seen), last_seen_ms: ms(n.last_seen) })
        .collect();
    let ledger = repo.destination_ledger(addr)?;
    Ok(DestinationHistoryView {
        addr: addr.to_string(),
        class: class_name(classify_addr(addr)),
        from_ms: ms(from),
        to_ms: ms(now),
        first_seen_ms: ledger.map(|l| ms(l.first_seen)),
        last_seen_ms: ledger.map(|l| ms(l.last_seen)),
        names,
        programs,
        hours: per_hour.into_values().collect(),
    })
}
