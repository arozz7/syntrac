//! What one hour adds to the permanent ledgers (first and last seen). Both repositories apply exactly this list, so the
//! in-memory and SQLite ledgers cannot disagree.

use std::collections::BTreeMap;
use std::net::IpAddr;

use crate::rollup::{Executable, HourRollup, LedgerEntry};
use crate::time::Timestamp;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LedgerObservations {
    pub executables: Vec<(Executable, LedgerEntry)>,
    pub destinations: Vec<(IpAddr, LedgerEntry)>,
    pub pairs: Vec<(Executable, IpAddr, LedgerEntry)>,
}

fn widen<K: Ord, V: Clone>(
    map: &mut BTreeMap<K, (V, LedgerEntry)>,
    key: K,
    value: &V,
    first: Timestamp,
    last: Timestamp,
) {
    map.entry(key)
        .and_modify(|(_, entry)| *entry = entry.widened(first, last))
        .or_insert_with(|| (value.clone(), LedgerEntry { first_seen: first, last_seen: last }));
}

impl HourRollup {
    /// First and last sightings per executable, per destination and per pair, merged over the hour's rows. Unattributed rows
    /// have no executable entry (they still count towards the destination).
    pub fn ledger_observations(&self) -> LedgerObservations {
        let mut executables: BTreeMap<String, (Executable, LedgerEntry)> = BTreeMap::new();
        let mut destinations: BTreeMap<IpAddr, ((), LedgerEntry)> = BTreeMap::new();
        let mut pairs: BTreeMap<(String, IpAddr), (Executable, LedgerEntry)> = BTreeMap::new();

        for process in &self.processes {
            if let Some(exe) = &process.exe {
                widen(&mut executables, exe.key.clone(), exe, process.first_seen, process.last_seen);
            }
        }
        for traffic in &self.traffic {
            widen(&mut destinations, traffic.addr, &(), traffic.first_seen, traffic.last_seen);
            if let Some(exe) = &traffic.exe {
                widen(&mut executables, exe.key.clone(), exe, traffic.first_seen, traffic.last_seen);
                widen(&mut pairs, (exe.key.clone(), traffic.addr), exe, traffic.first_seen, traffic.last_seen);
            }
        }
        LedgerObservations {
            executables: executables.into_values().collect(),
            destinations: destinations.into_iter().map(|(addr, ((), entry))| (addr, entry)).collect(),
            pairs: pairs.into_iter().map(|((_, addr), (exe, entry))| (exe, addr, entry)).collect(),
        }
    }
}
