//! `DestinationTrendRepository` for the in-memory repository: the reference the SQLite one must agree with.

use std::collections::{BTreeMap, HashSet};
use std::net::IpAddr;

use syntrac_core::destination_trend::{DestinationProgram, DestinationTotals, NewDestination, ProgramDestination};
use syntrac_core::rollup::executable_key;
use syntrac_core::{DestinationTrendRepository, Executable, RepoError, Timestamp};

use crate::memory::InMemoryRepository;

impl DestinationTrendRepository for InMemoryRepository {
    fn program_destinations(
        &self,
        path: &str,
        from: Timestamp,
        to: Timestamp,
    ) -> Result<Vec<ProgramDestination>, RepoError> {
        let Some(key) = executable_key(path) else { return Ok(Vec::new()) };
        let mut by_addr: BTreeMap<IpAddr, ProgramDestination> = BTreeMap::new();
        for ((hour, row_key, addr), row) in &self.rollups.traffic {
            if *hour < from || *hour > to || addr.is_loopback() || row_key.as_deref() != Some(key.as_str()) {
                continue;
            }
            let entry = by_addr.entry(*addr).or_insert_with(|| ProgramDestination {
                addr: *addr,
                bytes_sent: 0,
                bytes_received: 0,
                flows: 0,
                first_seen: row.first_seen,
                last_seen: row.last_seen,
            });
            entry.bytes_sent += row.bytes_sent;
            entry.bytes_received += row.bytes_received;
            entry.flows += u64::from(row.flows);
            entry.first_seen = entry.first_seen.min(row.first_seen);
            entry.last_seen = entry.last_seen.max(row.last_seen);
        }
        Ok(by_addr.into_values().collect())
    }

    fn destination_totals(&self, from: Timestamp, to: Timestamp) -> Result<Vec<DestinationTotals>, RepoError> {
        let mut by_addr: BTreeMap<IpAddr, (DestinationTotals, HashSet<Option<String>>)> = BTreeMap::new();
        for ((hour, key, addr), row) in &self.rollups.traffic {
            if *hour < from || *hour > to || addr.is_loopback() {
                continue;
            }
            let (totals, programs) = by_addr.entry(*addr).or_insert_with(|| {
                (
                    DestinationTotals { addr: *addr, bytes_sent: 0, bytes_received: 0, flows: 0, programs: 0 },
                    HashSet::new(),
                )
            });
            totals.bytes_sent += row.bytes_sent;
            totals.bytes_received += row.bytes_received;
            totals.flows += u64::from(row.flows);
            programs.insert(key.clone());
        }
        Ok(by_addr
            .into_values()
            .map(|(mut totals, programs)| {
                totals.programs = u32::try_from(programs.len()).unwrap_or(u32::MAX);
                totals
            })
            .collect())
    }

    fn new_destinations(&self, since: Timestamp) -> Result<Vec<NewDestination>, RepoError> {
        let state = &self.rollups;
        let mut rows = Vec::new();
        for (addr, ledger) in &state.destinations {
            if ledger.first_seen < since || addr.is_loopback() {
                continue;
            }
            // The first program to talk to it: the earliest first sighting of the pair, then the path that sorts first.
            let first = state
                .pairs
                .iter()
                .filter(|((_, pair_addr), _)| pair_addr == addr)
                .min_by(|((key_a, _), entry_a), ((key_b, _), entry_b)| {
                    entry_a.first_seen.cmp(&entry_b.first_seen).then_with(|| key_a.cmp(key_b))
                })
                .map(|((key, _), _)| Executable {
                    key: key.clone(),
                    path: state.paths.get(key).cloned().unwrap_or_else(|| key.clone()),
                });
            rows.push(NewDestination {
                addr: *addr,
                first_seen: ledger.first_seen,
                last_seen: ledger.last_seen,
                first_program: first,
            });
        }
        Ok(rows)
    }

    fn destination_programs(
        &self,
        addr: IpAddr,
        from: Timestamp,
        to: Timestamp,
    ) -> Result<Vec<DestinationProgram>, RepoError> {
        let mut by_key: BTreeMap<Option<String>, DestinationProgram> = BTreeMap::new();
        for ((hour, key, row_addr), row) in &self.rollups.traffic {
            if *hour < from || *hour > to || *row_addr != addr {
                continue;
            }
            let entry = by_key.entry(key.clone()).or_insert_with(|| DestinationProgram {
                exe: row.exe.clone(),
                bytes_sent: 0,
                bytes_received: 0,
                flows: 0,
                first_seen: row.first_seen,
                last_seen: row.last_seen,
            });
            entry.bytes_sent += row.bytes_sent;
            entry.bytes_received += row.bytes_received;
            entry.flows += u64::from(row.flows);
            entry.first_seen = entry.first_seen.min(row.first_seen);
            entry.last_seen = entry.last_seen.max(row.last_seen);
        }
        Ok(by_key.into_values().collect())
    }

    fn programs_of_destinations(
        &self,
        addrs: &[IpAddr],
        from: Timestamp,
        to: Timestamp,
    ) -> Result<Vec<(IpAddr, DestinationProgram)>, RepoError> {
        let mut found = Vec::new();
        for addr in addrs.iter().copied().collect::<HashSet<_>>() {
            found.extend(self.destination_programs(addr, from, to)?.into_iter().map(|program| (addr, program)));
        }
        Ok(found)
    }
}
