//! Folding the executables of one product into one row ("Acme, 4 programs"), and ranking the rows. PURE.
//!
//! Which executables belong together is decided by the caller (`product_key` from their path and signature); this module only
//! adds up what the ranking already knows about each member. What can be added exactly is added (CPU averages share the same
//! samples, starts, bytes, flows). What cannot be known without the hourly rows is stated honestly: a peak is the highest peak of
//! any member, distinct destinations are the largest member's count, and "recorded in" is the longest member's.

use std::collections::HashMap;

use crate::exe_location::ProductKey;
use crate::rollup::{Executable, LedgerEntry};
use crate::trend::{ProgramMetric, ProgramSummary};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GroupKey {
    /// The executables of one product (same publisher, same install folder).
    Product(ProductKey),
    /// A program that belongs to no product we can name (a bare name, an unknown path): its own row. Holds the executable key.
    Single(String),
    /// The processes whose program could not be identified.
    Unattributed,
}

impl GroupKey {
    /// For ties: by this key, the unattributed bucket last.
    fn sort_key(&self) -> Option<&str> {
        match self {
            Self::Product(product) => Some(product.root.as_str()),
            Self::Single(key) => Some(key.as_str()),
            Self::Unattributed => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProgramGroup {
    pub key: GroupKey,
    /// The row to show. For a product of several programs `exe` is `None`; for a lone program it is that program unchanged.
    pub summary: ProgramSummary,
    /// Every member, in path order, including those with nothing for the metric being ranked.
    pub members: Vec<ProgramSummary>,
}

fn key_of(program: &ProgramSummary, product_of: &impl Fn(&Executable) -> Option<ProductKey>) -> GroupKey {
    match &program.exe {
        None => GroupKey::Unattributed,
        Some(exe) => product_of(exe).map_or_else(|| GroupKey::Single(exe.key.clone()), GroupKey::Product),
    }
}

fn widen(a: Option<LedgerEntry>, b: Option<LedgerEntry>) -> Option<LedgerEntry> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.widened(b.first_seen, b.last_seen)),
        (one, other) => one.or(other),
    }
}

fn merge(members: &[ProgramSummary]) -> ProgramSummary {
    let mut merged = members[0].clone();
    if members.len() == 1 {
        return merged;
    }
    merged.exe = None;
    for member in &members[1..] {
        merged.cpu_avg_percent += member.cpu_avg_percent;
        merged.cpu_peak_percent = merged.cpu_peak_percent.max(member.cpu_peak_percent);
        merged.memory_peak_bytes = merged.memory_peak_bytes.max(member.memory_peak_bytes);
        merged.gpu_peak_percent = merged.gpu_peak_percent.max(member.gpu_peak_percent);
        merged.gpu_memory_peak_bytes = merged.gpu_memory_peak_bytes.max(member.gpu_memory_peak_bytes);
        merged.starts = merged.starts.saturating_add(member.starts);
        merged.hours_present = merged.hours_present.max(member.hours_present);
        merged.bytes_sent = merged.bytes_sent.saturating_add(member.bytes_sent);
        merged.bytes_received = merged.bytes_received.saturating_add(member.bytes_received);
        merged.flows = merged.flows.saturating_add(member.flows);
        merged.destinations = merged.destinations.max(member.destinations);
        merged.ledger = widen(merged.ledger, member.ledger);
    }
    merged
}

/// Groups the programs by product. `product_of` names the product of an executable, or `None` if it has none.
pub fn group_programs(
    programs: Vec<ProgramSummary>,
    product_of: impl Fn(&Executable) -> Option<ProductKey>,
) -> Vec<ProgramGroup> {
    let mut by_key: HashMap<GroupKey, Vec<ProgramSummary>> = HashMap::new();
    for program in programs {
        by_key.entry(key_of(&program, &product_of)).or_default().push(program);
    }
    let mut groups: Vec<ProgramGroup> = by_key
        .into_iter()
        .map(|(key, mut members)| {
            members.sort_by(|a, b| a.exe.as_ref().map(|e| &e.key).cmp(&b.exe.as_ref().map(|e| &e.key)));
            ProgramGroup { key, summary: merge(&members), members }
        })
        .collect();
    groups.sort_by_key(|group| (group.key.sort_key().is_none(), group.key.sort_key().map(str::to_owned)));
    groups
}

/// The `limit` highest rows by `metric`, highest first; a row with nothing for the metric is left out. Ties by key, so the
/// order never depends on how the rows arrived; the unattributed bucket last.
pub fn rank_groups(mut groups: Vec<ProgramGroup>, metric: ProgramMetric, limit: usize) -> Vec<ProgramGroup> {
    groups.retain(|group| group.summary.value(metric) > 0.0);
    groups.sort_by(|a, b| {
        b.summary.value(metric).total_cmp(&a.summary.value(metric)).then_with(|| {
            match (a.key.sort_key(), b.key.sort_key()) {
                (Some(x), Some(y)) => x.cmp(y),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }
        })
    });
    groups.truncate(limit);
    groups
}
