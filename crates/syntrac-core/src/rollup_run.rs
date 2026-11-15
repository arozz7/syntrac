//! Rolling up what is due: the repository port for rollups, and the pure scheduling rule around it.
//!
//! The repository only LOADS the raw rows of one hour, STORES the result (rows, ledgers and the watermark in one
//! transaction) and READS history back. It does no aggregation: that is [`roll_up_hour`], so every implementation agrees.

use std::net::IpAddr;
use std::time::Duration;

use crate::dns_names::{AddressName, DnsAnswerRow};
use crate::ports::RepoError;
use crate::rollup::{
    hour_start, next_hour, roll_up_hour, HourInput, HourRollup, LedgerEntry, ProcessHour, SystemHour, TrafficHour,
};
use crate::time::Timestamp;

pub trait RollupRepository {
    /// Every hour that starts before this instant has been rolled up. `None`: nothing has been rolled up yet.
    fn rolled_up_before(&self) -> Result<Option<Timestamp>, RepoError>;

    /// The time of the oldest detail row of any kind, or `None` when there is no detail at all.
    fn earliest_detail(&self) -> Result<Option<Timestamp>, RepoError>;

    /// The raw rows of the hour starting at `hour` (an exact UTC hour), with each owner resolved from its process INSTANCE.
    fn load_hour(&self, hour: Timestamp) -> Result<HourInput, RepoError>;

    /// ONE atomic step: stores the hour's rows (replacing any that exist for it), widens the ledgers, and moves the
    /// watermark to the end of the hour. An hour at or before the watermark is REFUSED: once detail has been purged, a
    /// re-roll would overwrite good rows with thinner ones. The watermark never moves backwards.
    fn store_hour(&mut self, rollup: &HourRollup) -> Result<(), RepoError>;

    /// System rows whose hour lies in `[from, to]`, oldest first.
    fn system_hours(&self, from: Timestamp, to: Timestamp) -> Result<Vec<SystemHour>, RepoError>;

    /// One executable's hours in `[from, to]`, oldest first. `path` is matched case-insensitively.
    fn executable_hours(&self, path: &str, from: Timestamp, to: Timestamp) -> Result<Vec<ProcessHour>, RepoError>;

    /// Everything that talked to `addr` in `[from, to]`, oldest first (then by executable).
    fn destination_hours(&self, addr: IpAddr, from: Timestamp, to: Timestamp) -> Result<Vec<TrafficHour>, RepoError>;

    /// When this executable was first and last seen. Permanent: still answers after its detail has been purged.
    fn executable_ledger(&self, path: &str) -> Result<Option<LedgerEntry>, RepoError>;

    /// When anything was first and last seen talking to this address.
    fn destination_ledger(&self, addr: IpAddr) -> Result<Option<LedgerEntry>, RepoError>;

    /// When this executable was first and last seen talking to this address.
    fn pair_ledger(&self, path: &str, addr: IpAddr) -> Result<Option<LedgerEntry>, RepoError>;

    /// Whether the one-time backfill of names from the retained DNS lookups has been done.
    fn names_backfilled(&self) -> Result<bool, RepoError>;

    /// The DNS lookups with `from <= ts < to`, oldest first.
    fn load_dns_answers(&self, from: Timestamp, to: Timestamp) -> Result<Vec<DnsAnswerRow>, RepoError>;

    /// ONE atomic step: widens the stored names with these pairs (keeping at most `MAX_NAMES_PER_ADDRESS` per address) and
    /// records that the backfill is done.
    fn store_backfilled_names(&mut self, names: &[AddressName]) -> Result<(), RepoError>;

    /// The names asked for when `addr` was reached, most recently seen first (a tie: by name). Permanent.
    fn address_names(&self, addr: IpAddr) -> Result<Vec<AddressName>, RepoError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RollupSettings {
    /// An hour is rolled up only once it ended this long ago, so late events (flows are stored after a grace period) are in.
    pub settle: Duration,
    /// At most this many hours per pass, so a backlog cannot stall the collector's loop.
    pub max_hours_per_pass: usize,
}

impl Default for RollupSettings {
    fn default() -> Self {
        Self { settle: Duration::from_secs(5 * 60), max_hours_per_pass: 2 }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RollupPass {
    pub hours_rolled: usize,
    /// More hours are already due: run again soon instead of waiting for the usual interval.
    pub backlog: bool,
}

/// Rolls up the completed hours that are due, oldest first, up to the pass limit. The first pass starts at the hour of the
/// earliest detail. Hours with no data (the collector was down) store no rows but still advance the watermark.
pub fn roll_up_due<R: RollupRepository + ?Sized>(
    repo: &mut R,
    now: Timestamp,
    settings: &RollupSettings,
) -> Result<RollupPass, RepoError> {
    let mut next = match repo.rolled_up_before()? {
        Some(done_before) => done_before,
        None => match repo.earliest_detail()? {
            Some(oldest) => hour_start(oldest),
            None => return Ok(RollupPass::default()),
        },
    };

    let settle_micros = i64::try_from(settings.settle.as_micros()).unwrap_or(i64::MAX);
    let settled_until = Timestamp::from_micros(now.as_micros().saturating_sub(settle_micros));
    let is_due = |hour: Timestamp| next_hour(hour) <= settled_until;

    let mut rolled = 0;
    while rolled < settings.max_hours_per_pass && is_due(next) {
        let rollup = roll_up_hour(&repo.load_hour(next)?);
        repo.store_hour(&rollup)?;
        next = next_hour(next);
        rolled += 1;
    }
    Ok(RollupPass { hours_rolled: rolled, backlog: is_due(next) })
}
