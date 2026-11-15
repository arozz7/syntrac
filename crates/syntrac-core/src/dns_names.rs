//! Which names were asked for when an address was reached: the permanent link between a destination address and the DNS names
//! some program looked up for it.
//!
//! The raw DNS events are purged after the detail retention window (14 days by default). What is kept for good is only a pair
//! (address, name) with when it was first and last seen. Both repositories use these pure functions, so they cannot disagree.
//!
//! A name is what a program ASKED for. It is not the owner of the address: a content network answers thousands of names from one
//! address and one name from many addresses. The app labels names that way.
//!
//! Only first and last seen are stored (no counters), so storing the same answers again, in any order, changes nothing.

use std::collections::BTreeMap;
use std::net::IpAddr;

use crate::ports::RepoError;
use crate::rollup::{next_hour, HourInput};
use crate::rollup_run::RollupRepository;
use crate::time::Timestamp;

/// At most this many names are kept for one address (the most recently seen win), so a shared address cannot grow without limit.
pub const MAX_NAMES_PER_ADDRESS: usize = 50;

/// How far back the one-time backfill looks, however old the oldest event is.
pub const BACKFILL_LOOKBACK_HOURS: i64 = 30 * 24;

/// One DNS lookup as stored: the name asked for and the addresses it was answered with (empty: no answer).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnsAnswerRow {
    pub ts: Timestamp,
    pub query: String,
    pub addresses: Vec<IpAddr>,
}

/// One address and one name that was asked for, with the first and last time that pair was seen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressName {
    pub addr: IpAddr,
    pub name: String,
    pub first_seen: Timestamp,
    pub last_seen: Timestamp,
}

/// The name in its stored form: trimmed, lower case, without a trailing dot. `None` when nothing is left.
pub fn normalize_name(raw: &str) -> Option<String> {
    let name = raw.trim().trim_end_matches('.').to_lowercase();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

type Pairs = BTreeMap<(IpAddr, String), (Timestamp, Timestamp)>;

fn widen(pairs: &mut Pairs, addr: IpAddr, name: String, first: Timestamp, last: Timestamp) {
    pairs
        .entry((addr, name))
        .and_modify(|(f, l)| {
            *f = (*f).min(first);
            *l = (*l).max(last);
        })
        .or_insert((first, last));
}

fn into_rows(pairs: Pairs) -> Vec<AddressName> {
    pairs
        .into_iter()
        .map(|((addr, name), (first_seen, last_seen))| AddressName { addr, name, first_seen, last_seen })
        .collect()
}

/// The (address, name) pairs in a set of DNS answers, sorted by address then name. Lookups with no answer and blank names give
/// nothing. Addresses are in canonical form (an IPv4-mapped IPv6 address is the IPv4 address), as flows are.
pub fn address_names(answers: &[DnsAnswerRow]) -> Vec<AddressName> {
    let mut pairs = Pairs::new();
    for answer in answers {
        let Some(name) = normalize_name(&answer.query) else { continue };
        for addr in &answer.addresses {
            widen(&mut pairs, addr.to_canonical(), name.clone(), answer.ts, answer.ts);
        }
    }
    into_rows(pairs)
}

/// Merges two sets of pairs, widening first and last seen. Sorted by address then name.
pub fn merge_names(rows: impl IntoIterator<Item = AddressName>) -> Vec<AddressName> {
    let mut pairs = Pairs::new();
    for row in rows {
        widen(&mut pairs, row.addr, row.name, row.first_seen, row.last_seen);
    }
    into_rows(pairs)
}

/// Keeps the [`MAX_NAMES_PER_ADDRESS`] most recently seen names of each address (a tie on the time keeps the name that sorts
/// first). Sorted by address then name.
pub fn cap_newest(rows: Vec<AddressName>) -> Vec<AddressName> {
    let mut by_addr: BTreeMap<IpAddr, Vec<AddressName>> = BTreeMap::new();
    for row in merge_names(rows) {
        by_addr.entry(row.addr).or_default().push(row);
    }
    let mut kept = Vec::new();
    for (_, mut names) in by_addr {
        names.sort_by(|a, b| b.last_seen.cmp(&a.last_seen).then_with(|| a.name.cmp(&b.name)));
        names.truncate(MAX_NAMES_PER_ADDRESS);
        kept.extend(names);
    }
    kept.sort_by(|a, b| (a.addr, &a.name).cmp(&(b.addr, &b.name)));
    kept
}

impl HourInput {
    /// The names asked for in this hour. Lookups outside the hour are ignored, like every other row.
    pub fn address_names(&self) -> Vec<AddressName> {
        let end = next_hour(self.hour);
        let inside: Vec<DnsAnswerRow> = self.dns.iter().filter(|a| a.ts >= self.hour && a.ts < end).cloned().collect();
        address_names(&inside)
    }
}

/// What a backfill pass did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct NamesBackfill {
    /// False when an earlier run had already done it: nothing was read or written.
    pub ran: bool,
    /// Pairs stored by this run.
    pub names: usize,
}

/// Reads the DNS lookups that are still retained and stores their names, ONCE: the repository remembers that it was done, in
/// the same step that stores the names. Hours already rolled up cannot be rolled again (their detail may be gone), so this is
/// the only way their names get in. A database with no detail at all has nothing to backfill and is marked done.
pub fn backfill_names<R: RollupRepository + ?Sized>(repo: &mut R, now: Timestamp) -> Result<NamesBackfill, RepoError> {
    if repo.names_backfilled()? {
        return Ok(NamesBackfill::default());
    }
    let newest_hour = crate::rollup::hour_start(now);
    let oldest_allowed = Timestamp::from_micros(
        newest_hour.as_micros().saturating_sub(BACKFILL_LOOKBACK_HOURS * crate::rollup::HOUR_MICROS),
    );
    let mut merged: Vec<AddressName> = Vec::new();
    if let Some(oldest) = repo.earliest_detail()? {
        let mut hour = crate::rollup::hour_start(oldest).max(oldest_allowed);
        while hour <= newest_hour {
            let answers = repo.load_dns_answers(hour, next_hour(hour))?;
            merged = merge_names(merged.into_iter().chain(address_names(&answers)));
            hour = next_hour(hour);
        }
    }
    let kept = cap_newest(merged);
    repo.store_backfilled_names(&kept)?;
    Ok(NamesBackfill { ran: true, names: kept.len() })
}
