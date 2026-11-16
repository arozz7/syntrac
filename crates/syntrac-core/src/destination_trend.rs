//! Trends for network destinations over the hourly rollups: where the machine talked to the most, who talked to one address, and
//! which addresses are new.
//!
//! Like `trend`, the stores only SUM and LIST (one group-by per question, so a year of hourly rows never reaches the app).
//! Ranking, ties, the choice of scope and the "new" rule are pure functions here, so the in-memory and SQLite stores cannot
//! disagree about the maths.
//!
//! Traffic to this machine itself (loopback) is never included, as in the program ranking.

use std::cmp::Ordering;
use std::net::IpAddr;

use crate::addr_class::classify_addr;
use crate::ports::RepoError;
use crate::rollup::{Executable, HOUR_MICROS};
use crate::time::Timestamp;

/// Destinations first seen within this many hours of the start of history are the baseline, not news: on the first day
/// everything is new, which tells nobody anything.
pub const BASELINE_HOURS: i64 = 24;

/// One destination address, summed over a range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DestinationTotals {
    pub addr: IpAddr,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub flows: u64,
    /// How many different programs talked to it (the bucket of unidentified programs counts as one).
    pub programs: u32,
}

impl DestinationTotals {
    pub fn bytes(&self) -> u64 {
        self.bytes_sent.saturating_add(self.bytes_received)
    }
}

/// An address first seen at some time, with the program that was first to talk to it (`None`: no identified program).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewDestination {
    pub addr: IpAddr,
    pub first_seen: Timestamp,
    pub last_seen: Timestamp,
    pub first_program: Option<Executable>,
}

/// One program's share of the traffic to one address over a range. `exe: None` is the bucket of unidentified programs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DestinationProgram {
    pub exe: Option<Executable>,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub flows: u64,
    /// First and last time within the range.
    pub first_seen: Timestamp,
    pub last_seen: Timestamp,
}

/// The read side of destination trends. Implementations only group, sum and list.
pub trait DestinationTrendRepository {
    /// Every destination over the hours in `[from, to]`, loopback left out, in any order.
    fn destination_totals(&self, from: Timestamp, to: Timestamp) -> Result<Vec<DestinationTotals>, RepoError>;

    /// Every destination whose permanent first sighting is at or after `since`, loopback left out, in any order. The program
    /// is the one first seen talking to it (a tie: the path that sorts first).
    fn new_destinations(&self, since: Timestamp) -> Result<Vec<NewDestination>, RepoError>;

    /// The programs that talked to `addr` over the hours in `[from, to]`, in any order.
    fn destination_programs(
        &self,
        addr: IpAddr,
        from: Timestamp,
        to: Timestamp,
    ) -> Result<Vec<DestinationProgram>, RepoError>;

    /// The programs that talked to each of `addrs` over the hours in `[from, to]`, in ONE read (a page of the ranking needs
    /// them all at once). One entry per (address, program); an address nobody talked to has none; any order.
    fn programs_of_destinations(
        &self,
        addrs: &[IpAddr],
        from: Timestamp,
        to: Timestamp,
    ) -> Result<Vec<(IpAddr, DestinationProgram)>, RepoError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DestinationOrder {
    /// Bytes sent plus received.
    Bytes,
    Flows,
    /// The number of different programs that talked to it.
    Programs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressScope {
    /// Addresses on the internet only (the default: the router and multicast chatter are not what is asked about).
    Internet,
    /// Every address, including this network's own and multicast.
    All,
}

impl AddressScope {
    fn includes(self, addr: IpAddr) -> bool {
        match self {
            Self::All => true,
            Self::Internet => classify_addr(addr).is_internet(),
        }
    }
}

fn value(row: &DestinationTotals, order: DestinationOrder) -> u64 {
    match order {
        DestinationOrder::Bytes => row.bytes(),
        DestinationOrder::Flows => row.flows,
        DestinationOrder::Programs => u64::from(row.programs),
    }
}

/// The `limit` highest destinations by `order`, highest first, inside `scope`. The scope is applied BEFORE the limit, so
/// addresses that are hidden do not use up rows. An address with nothing for the order is left out. A tie is broken by address.
pub fn rank_destinations(
    mut rows: Vec<DestinationTotals>,
    order: DestinationOrder,
    scope: AddressScope,
    limit: usize,
) -> Vec<DestinationTotals> {
    rows.retain(|row| scope.includes(row.addr) && value(row, order) > 0);
    rows.sort_by(|a, b| value(b, order).cmp(&value(a, order)).then_with(|| a.addr.cmp(&b.addr)));
    rows.truncate(limit);
    rows
}

/// The `keep` biggest programs of one address (ranked as `rank_destination_programs`) and how many more there were.
pub fn top_destination_programs(rows: Vec<DestinationProgram>, keep: usize) -> (Vec<DestinationProgram>, u32) {
    let mut ranked = rank_destination_programs(rows);
    let more = u32::try_from(ranked.len().saturating_sub(keep)).unwrap_or(u32::MAX);
    ranked.truncate(keep);
    (ranked, more)
}

/// The programs that talked to one address, most bytes first. A tie is broken by path, the unidentified bucket last.
pub fn rank_destination_programs(mut rows: Vec<DestinationProgram>) -> Vec<DestinationProgram> {
    rows.sort_by(|a, b| {
        let bytes = |p: &DestinationProgram| p.bytes_sent.saturating_add(p.bytes_received);
        bytes(b).cmp(&bytes(a)).then_with(|| match (&a.exe, &b.exe) {
            (Some(x), Some(y)) => x.key.cmp(&y.key),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        })
    });
    rows
}

/// The instant from which a first sighting counts as new: the start of the window the user asked for, but never inside the
/// baseline (the first [`BASELINE_HOURS`] of history). With no history at all nothing can be new, so the cutoff is the end of
/// time.
pub fn new_destination_cutoff(history_start: Option<Timestamp>, window_start: Timestamp) -> Timestamp {
    match history_start {
        Some(start) => {
            let baseline_end = Timestamp::from_micros(start.as_micros().saturating_add(BASELINE_HOURS * HOUR_MICROS));
            window_start.max(baseline_end)
        }
        None => Timestamp::from_micros(i64::MAX),
    }
}

/// The destinations first seen at or after `cutoff`, newest first (a tie: by address), inside `scope`, at most `limit`.
pub fn select_new(
    mut rows: Vec<NewDestination>,
    cutoff: Timestamp,
    scope: AddressScope,
    limit: usize,
) -> Vec<NewDestination> {
    rows.retain(|row| row.first_seen >= cutoff && scope.includes(row.addr));
    rows.sort_by(|a, b| b.first_seen.cmp(&a.first_seen).then_with(|| a.addr.cmp(&b.addr)));
    rows.truncate(limit);
    rows
}
