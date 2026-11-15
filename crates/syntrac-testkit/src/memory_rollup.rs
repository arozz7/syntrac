//! `RollupRepository` for the in-memory repository: the reference the SQLite one must agree with.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::IpAddr;

use syntrac_core::dns_names::cap_newest;
use syntrac_core::rollup::{executable_key, next_hour, FlowInputRow, ProcessSampleRow, StartRow};
use syntrac_core::{
    AddressName, DnsAnswerRow, EventPayload, HourInput, HourRollup, LedgerEntry, ProcessHour, ProcessInstanceId,
    RepoError, RollupRepository, SystemHour, SystemPoint, Timestamp, TrafficHour,
};
use syntrac_core::{ProcessTotals, TrafficTotals};

use crate::memory::InMemoryRepository;

/// Everything the rollups keep. Ledgers are separate maps because they outlive the rows.
#[derive(Debug, Default)]
pub struct RollupState {
    rolled_up_before: Option<Timestamp>,
    system: BTreeMap<Timestamp, SystemHour>,
    processes: BTreeMap<(Timestamp, Option<String>), ProcessHour>,
    traffic: BTreeMap<(Timestamp, Option<String>, IpAddr), TrafficHour>,
    /// The spelling of each executable's path as first seen.
    paths: HashMap<String, String>,
    executables: HashMap<String, LedgerEntry>,
    destinations: HashMap<IpAddr, LedgerEntry>,
    pairs: HashMap<(String, IpAddr), LedgerEntry>,
    /// Executable identities (permanent, like the ledgers), by executable key.
    identities: HashMap<String, syntrac_core::IdentityRecord>,
    /// The names asked for, by (address, name): first and last seen. Permanent.
    names: BTreeMap<(IpAddr, String), (Timestamp, Timestamp)>,
    names_backfilled: bool,
}

impl RollupState {
    /// Widens the stored pairs and keeps at most `MAX_NAMES_PER_ADDRESS` names for each address that was touched, by the same
    /// rule as the SQLite store (`dns_names::cap_newest`).
    fn store_names(&mut self, rows: &[AddressName]) {
        let touched: HashSet<IpAddr> = rows.iter().map(|r| r.addr.to_canonical()).collect();
        for row in rows {
            let key = (row.addr.to_canonical(), row.name.clone());
            self.names
                .entry(key)
                .and_modify(|(first, last)| {
                    *first = (*first).min(row.first_seen);
                    *last = (*last).max(row.last_seen);
                })
                .or_insert((row.first_seen, row.last_seen));
        }
        for addr in touched {
            let current: Vec<AddressName> = self.names_of(addr);
            self.names.retain(|(a, _), _| *a != addr);
            for kept in cap_newest(current) {
                self.names.insert((kept.addr, kept.name), (kept.first_seen, kept.last_seen));
            }
        }
    }

    /// The names of one address, most recently seen first (a tie: by name).
    fn names_of(&self, addr: IpAddr) -> Vec<AddressName> {
        let addr = addr.to_canonical();
        let mut rows: Vec<AddressName> = self
            .names
            .iter()
            .filter(|((a, _), _)| *a == addr)
            .map(|((a, name), (first_seen, last_seen))| AddressName {
                addr: *a,
                name: name.clone(),
                first_seen: *first_seen,
                last_seen: *last_seen,
            })
            .collect();
        rows.sort_by(|a, b| b.last_seen.cmp(&a.last_seen).then_with(|| a.name.cmp(&b.name)));
        rows
    }

    /// Every executable the rollups know: key and the spelling first seen.
    pub(crate) fn known_executables(&self) -> impl Iterator<Item = (&String, &String)> {
        self.paths.iter()
    }

    pub(crate) fn executable_ledger_of(&self, key: &str) -> Option<LedgerEntry> {
        self.executables.get(key).copied()
    }

    pub(crate) fn identity_of(&self, key: &str) -> Option<&syntrac_core::IdentityRecord> {
        self.identities.get(key)
    }

    /// Stores an identity under the executable's key; false when the executable was never seen.
    pub(crate) fn save_identity(&mut self, record: &syntrac_core::IdentityRecord) -> bool {
        let Some(path) = self.paths.get(&record.exe.key) else { return false };
        let mut stored = record.clone();
        stored.exe = syntrac_core::Executable { key: record.exe.key.clone(), path: path.clone() };
        self.identities.insert(record.exe.key.clone(), stored);
        true
    }
}

fn widen<K: std::hash::Hash + Eq>(map: &mut HashMap<K, LedgerEntry>, key: K, observed: LedgerEntry) {
    map.entry(key)
        .and_modify(|entry| *entry = entry.widened(observed.first_seen, observed.last_seen))
        .or_insert(observed);
}

impl InMemoryRepository {
    fn exe_path_of(&self, owner: Option<ProcessInstanceId>) -> Option<String> {
        owner.and_then(|id| self.processes.get(&id)).map(|process| process.image_path.clone())
    }
}

impl RollupRepository for InMemoryRepository {
    fn rolled_up_before(&self) -> Result<Option<Timestamp>, RepoError> {
        Ok(self.rollups.rolled_up_before)
    }

    fn earliest_detail(&self) -> Result<Option<Timestamp>, RepoError> {
        let events = self.events.iter().map(|e| e.ts);
        let samples = self.metrics.iter().map(|s| s.ts);
        Ok(events.chain(samples).min())
    }

    fn load_hour(&self, hour: Timestamp) -> Result<HourInput, RepoError> {
        let end = next_hour(hour);
        let in_hour = |ts: Timestamp| ts >= hour && ts < end;
        let mut input = HourInput::empty(hour);

        for sample in self.metrics.iter().filter(|s| in_hour(s.ts)) {
            input.system.push(SystemPoint { ts: sample.ts, system: sample.system, others: sample.others });
            for process in &sample.processes {
                input.process_samples.push(ProcessSampleRow {
                    ts: sample.ts,
                    exe_path: self.exe_path_of(process.instance),
                    cpu_percent: process.cpu_percent,
                    memory_bytes: process.memory_bytes,
                });
            }
        }
        for event in &self.events {
            match &event.payload {
                EventPayload::ProcessStarted { image_path, .. } if in_hour(event.ts) => {
                    input.starts.push(StartRow { ts: event.ts, exe_path: Some(image_path.clone()) });
                }
                EventPayload::NetworkFlow {
                    remote_addr,
                    remote_port,
                    bytes_sent,
                    bytes_received,
                    window_start,
                    ..
                } if in_hour(*window_start) => {
                    input.flows.push(FlowInputRow {
                        window_start: *window_start,
                        exe_path: self.exe_path_of(event.process_instance_id),
                        remote_addr: *remote_addr,
                        remote_port: *remote_port,
                        bytes_sent: *bytes_sent,
                        bytes_received: *bytes_received,
                    });
                }
                EventPayload::DnsQuery { query, addresses, .. } if in_hour(event.ts) => {
                    input.dns.push(DnsAnswerRow { ts: event.ts, query: query.clone(), addresses: addresses.clone() });
                }
                _ => {}
            }
        }
        Ok(input)
    }

    fn store_hour(&mut self, rollup: &HourRollup) -> Result<(), RepoError> {
        let end = next_hour(rollup.hour);
        if self.rollups.rolled_up_before.is_some_and(|done| end <= done) {
            return Err(RepoError::Backend(format!(
                "the hour starting at {} is already rolled up",
                rollup.hour.as_micros()
            )));
        }
        let state = &mut self.rollups;
        state.system.remove(&rollup.hour);
        state.processes.retain(|(hour, _), _| *hour != rollup.hour);
        state.traffic.retain(|(hour, _, _), _| *hour != rollup.hour);

        if let Some(system) = &rollup.system {
            state.system.insert(rollup.hour, system.clone());
        }
        // The path shown for an executable is the spelling first seen (the same rule as the SQLite `executables` table).
        let mut named = |exe: &Option<syntrac_core::Executable>| {
            exe.as_ref().map(|e| {
                let path = state.paths.entry(e.key.clone()).or_insert_with(|| e.path.clone()).clone();
                syntrac_core::Executable { key: e.key.clone(), path }
            })
        };
        for row in &rollup.processes {
            let mut row = row.clone();
            row.exe = named(&row.exe);
            state.processes.insert((rollup.hour, row.exe.as_ref().map(|e| e.key.clone())), row);
        }
        for row in &rollup.traffic {
            let mut row = row.clone();
            row.exe = named(&row.exe);
            state.traffic.insert((rollup.hour, row.exe.as_ref().map(|e| e.key.clone()), row.addr), row);
        }

        let observed = rollup.ledger_observations();
        for (exe, entry) in observed.executables {
            widen(&mut state.executables, exe.key, entry);
        }
        for (addr, entry) in observed.destinations {
            widen(&mut state.destinations, addr, entry);
        }
        for (exe, addr, entry) in observed.pairs {
            widen(&mut state.pairs, (exe.key, addr), entry);
        }
        state.store_names(&rollup.names);
        state.rolled_up_before = Some(state.rolled_up_before.map_or(end, |done| done.max(end)));
        Ok(())
    }

    fn system_hours(&self, from: Timestamp, to: Timestamp) -> Result<Vec<SystemHour>, RepoError> {
        Ok(self.rollups.system.range(from..=to).map(|(_, row)| row.clone()).collect())
    }

    fn executable_hours(&self, path: &str, from: Timestamp, to: Timestamp) -> Result<Vec<ProcessHour>, RepoError> {
        let Some(key) = executable_key(path) else { return Ok(Vec::new()) };
        Ok(self
            .rollups
            .processes
            .iter()
            .filter(|((hour, exe), _)| *hour >= from && *hour <= to && exe.as_deref() == Some(key.as_str()))
            .map(|(_, row)| row.clone())
            .collect())
    }

    fn destination_hours(&self, addr: IpAddr, from: Timestamp, to: Timestamp) -> Result<Vec<TrafficHour>, RepoError> {
        // The map is ordered by (hour, executable, address), the order the contract promises.
        Ok(self
            .rollups
            .traffic
            .iter()
            .filter(|((hour, _, row_addr), _)| *hour >= from && *hour <= to && *row_addr == addr)
            .map(|(_, row)| row.clone())
            .collect())
    }

    fn executable_ledger(&self, path: &str) -> Result<Option<LedgerEntry>, RepoError> {
        Ok(executable_key(path).and_then(|key| self.rollups.executables.get(&key).copied()))
    }

    fn destination_ledger(&self, addr: IpAddr) -> Result<Option<LedgerEntry>, RepoError> {
        Ok(self.rollups.destinations.get(&addr).copied())
    }

    fn pair_ledger(&self, path: &str, addr: IpAddr) -> Result<Option<LedgerEntry>, RepoError> {
        Ok(executable_key(path).and_then(|key| self.rollups.pairs.get(&(key, addr)).copied()))
    }

    fn names_backfilled(&self) -> Result<bool, RepoError> {
        Ok(self.rollups.names_backfilled)
    }

    fn load_dns_answers(&self, from: Timestamp, to: Timestamp) -> Result<Vec<DnsAnswerRow>, RepoError> {
        let mut answers: Vec<DnsAnswerRow> = self
            .events
            .iter()
            .filter(|event| event.ts >= from && event.ts < to)
            .filter_map(|event| match &event.payload {
                EventPayload::DnsQuery { query, addresses, .. } => {
                    Some(DnsAnswerRow { ts: event.ts, query: query.clone(), addresses: addresses.clone() })
                }
                _ => None,
            })
            .collect();
        answers.sort_by_key(|answer| answer.ts);
        Ok(answers)
    }

    fn store_backfilled_names(&mut self, names: &[AddressName]) -> Result<(), RepoError> {
        self.rollups.store_names(names);
        self.rollups.names_backfilled = true;
        Ok(())
    }

    fn address_names(&self, addr: IpAddr) -> Result<Vec<AddressName>, RepoError> {
        Ok(self.rollups.names_of(addr))
    }
}

impl syntrac_core::TrendRepository for InMemoryRepository {
    fn system_sample_total(&self, from: Timestamp, to: Timestamp) -> Result<u64, RepoError> {
        Ok(self.rollups.system.range(from..=to).map(|(_, row)| u64::from(row.samples)).sum())
    }

    fn process_totals(&self, from: Timestamp, to: Timestamp) -> Result<Vec<ProcessTotals>, RepoError> {
        let mut by_key: BTreeMap<Option<String>, ProcessTotals> = BTreeMap::new();
        for ((hour, key), row) in &self.rollups.processes {
            // Only hours with a system row count: its samples are the weights.
            let Some(system) = self.rollups.system.get(hour).filter(|_| *hour >= from && *hour <= to) else {
                continue;
            };
            let totals = by_key.entry(key.clone()).or_insert_with(|| ProcessTotals {
                exe: row.exe.clone(),
                hours_present: 0,
                samples_present: 0,
                cpu_weighted: 0.0,
                cpu_peak: 0.0,
                memory_peak: 0,
                starts: 0,
            });
            totals.hours_present += 1;
            totals.samples_present += u64::from(row.samples_present);
            totals.cpu_weighted += row.cpu_avg * f64::from(system.samples);
            totals.cpu_peak = totals.cpu_peak.max(row.cpu_peak);
            totals.memory_peak = totals.memory_peak.max(row.memory_peak);
            totals.starts += u64::from(row.starts);
        }
        Ok(by_key.into_values().collect())
    }

    fn traffic_totals(&self, from: Timestamp, to: Timestamp) -> Result<Vec<TrafficTotals>, RepoError> {
        let mut by_key: BTreeMap<Option<String>, (TrafficTotals, HashSet<IpAddr>)> = BTreeMap::new();
        for ((hour, key, addr), row) in &self.rollups.traffic {
            if *hour < from || *hour > to || addr.is_loopback() {
                continue;
            }
            let (totals, addrs) = by_key.entry(key.clone()).or_insert_with(|| {
                (
                    TrafficTotals { exe: row.exe.clone(), bytes_sent: 0, bytes_received: 0, flows: 0, destinations: 0 },
                    HashSet::new(),
                )
            });
            totals.bytes_sent += row.bytes_sent;
            totals.bytes_received += row.bytes_received;
            totals.flows += u64::from(row.flows);
            addrs.insert(*addr);
        }
        Ok(by_key
            .into_values()
            .map(|(mut totals, addrs)| {
                totals.destinations = u32::try_from(addrs.len()).unwrap_or(u32::MAX);
                totals
            })
            .collect())
    }
}

impl syntrac_core::storage_guard::StorageStats for InMemoryRepository {
    /// Sizes are rough per-row estimates: enough for the guard's logic, which only needs them to rise and fall with the data.
    fn usage(&self) -> Result<syntrac_core::storage_guard::DatabaseUsage, RepoError> {
        let bytes =
            (self.events.len() as u64) * 200 + (self.metrics.len() as u64) * 100 + (self.processes.len() as u64) * 200;
        Ok(syntrac_core::storage_guard::DatabaseUsage {
            file_bytes: bytes,
            live_bytes: bytes,
            reclaimable_bytes: 0,
            oldest_detail: self.earliest_detail()?,
            oldest_rollup: self.rollups.system.keys().next().copied(),
        })
    }

    fn reclaim_space(&mut self) -> Result<(), RepoError> {
        Ok(())
    }
}
