//! Hourly rollups: the compact, permanent summary of what the detail tables hold for one completed hour (UTC).
//!
//! This module is PURE: it turns the raw rows of one hour into rollup rows, nothing else. All aggregation maths lives
//! here so the in-memory and SQLite repositories cannot disagree, and the result is deterministic (rows are sorted before
//! they are summed), which is what makes re-running an hour give byte-identical rows.
//!
//! What is kept (docs/STORAGE-PLAN.md, S3):
//! - system: samples, CPU average and peak, memory, disk and network totals;
//! - per executable: how long it was among the heaviest processes, CPU and memory, how many times it started;
//! - per executable and destination address: bytes, flows, first and last seen, how many distinct ports and the busiest one.
//!   The port is NOT part of the key: most ports are ephemeral and random, and keying on them would cost about 1.6 GB a year.
//!
//! An executable is identified by its lower-cased image path. Rows whose owner or path is unknown are "unattributed" (`None`).

use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;

use crate::dns_names::{AddressName, DnsAnswerRow};
use crate::metrics::SystemPoint;
use crate::time::Timestamp;

pub const HOUR_MICROS: i64 = 3_600_000_000;

/// The start of the UTC hour containing `ts`. A timestamp exactly on the boundary belongs to the NEW hour.
pub fn hour_start(ts: Timestamp) -> Timestamp {
    Timestamp::from_micros(ts.as_micros().div_euclid(HOUR_MICROS) * HOUR_MICROS)
}

pub fn next_hour(hour: Timestamp) -> Timestamp {
    Timestamp::from_micros(hour.as_micros().saturating_add(HOUR_MICROS))
}

/// The identity of an executable across processes and restarts: its image path, lower-cased (Windows paths are not case
/// sensitive). An empty path is no identity: `None`.
pub fn executable_key(path: &str) -> Option<String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_lowercase())
    }
}

/// An executable as the rollups name it: the key, plus a path as it was written (the smallest spelling seen in the hour, so
/// the choice is deterministic).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Executable {
    pub key: String,
    pub path: String,
}

impl Executable {
    pub fn from_path(path: &str) -> Option<Self> {
        executable_key(path).map(|key| Self { key, path: path.trim().to_owned() })
    }
}

/// One process sample, with its executable already resolved from the process INSTANCE (never from the PID, so a reused PID
/// cannot inherit another program's history).
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessSampleRow {
    pub ts: Timestamp,
    pub exe_path: Option<String>,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
}

/// A `process_started` event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartRow {
    pub ts: Timestamp,
    pub exe_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowInputRow {
    pub window_start: Timestamp,
    pub exe_path: Option<String>,
    pub remote_addr: IpAddr,
    pub remote_port: u16,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

/// Everything the detail tables hold for one hour, as loaded by a repository.
#[derive(Debug, Clone, PartialEq)]
pub struct HourInput {
    pub hour: Timestamp,
    pub system: Vec<SystemPoint>,
    pub process_samples: Vec<ProcessSampleRow>,
    pub starts: Vec<StartRow>,
    pub flows: Vec<FlowInputRow>,
    /// The DNS lookups of the hour (see `dns_names`).
    pub dns: Vec<DnsAnswerRow>,
}

impl HourInput {
    pub fn empty(hour: Timestamp) -> Self {
        Self {
            hour,
            system: Vec::new(),
            process_samples: Vec::new(),
            starts: Vec::new(),
            flows: Vec::new(),
            dns: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SystemHour {
    pub hour: Timestamp,
    pub samples: u32,
    pub cpu_avg: f64,
    pub cpu_max: f64,
    pub memory_used_avg: u64,
    pub memory_used_max: u64,
    pub memory_total: u64,
    /// Totals for the hour (the sources report bytes since the previous sample).
    pub disk_read: u64,
    pub disk_write: u64,
    pub net_received: u64,
    pub net_sent: u64,
    pub others_cpu_avg: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProcessHour {
    pub hour: Timestamp,
    /// `None`: processes whose executable is unknown.
    pub exe: Option<Executable>,
    /// In how many samples this executable appeared (it is recorded while among the heaviest processes).
    pub samples_present: u32,
    /// The executable's CPU summed over its instances, averaged over ALL the hour's samples (absent counts as zero).
    pub cpu_avg: f64,
    /// The highest per-sample sum over its instances.
    pub cpu_peak: f64,
    pub memory_peak: u64,
    /// `process_started` events in the hour.
    pub starts: u32,
    pub first_seen: Timestamp,
    pub last_seen: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrafficHour {
    pub hour: Timestamp,
    pub exe: Option<Executable>,
    pub addr: IpAddr,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub flows: u32,
    pub first_seen: Timestamp,
    pub last_seen: Timestamp,
    /// Distinct remote ports in THIS hour (cannot be added across hours).
    pub distinct_ports: u32,
    /// The remote port that carried the most bytes; the lowest port on a tie.
    pub top_port: u16,
}

/// When something was first and last seen. Permanent: it outlives the detail and the rollup rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LedgerEntry {
    pub first_seen: Timestamp,
    pub last_seen: Timestamp,
}

impl LedgerEntry {
    /// Widens the entry to cover another observation. Taking the min and max makes updating idempotent and order independent.
    pub fn widened(self, first_seen: Timestamp, last_seen: Timestamp) -> Self {
        Self { first_seen: self.first_seen.min(first_seen), last_seen: self.last_seen.max(last_seen) }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HourRollup {
    pub hour: Timestamp,
    pub system: Option<SystemHour>,
    /// Sorted by executable (unattributed first).
    pub processes: Vec<ProcessHour>,
    /// Sorted by executable (unattributed first), then address.
    pub traffic: Vec<TrafficHour>,
    /// The (address, name) pairs asked for in the hour, sorted by address then name. Stored with the rollup, permanently.
    pub names: Vec<AddressName>,
}

fn in_hour(hour: Timestamp, ts: Timestamp) -> bool {
    ts >= hour && ts < next_hour(hour)
}

fn system_hour(hour: Timestamp, points: &[&SystemPoint]) -> Option<SystemHour> {
    if points.is_empty() {
        return None;
    }
    let n = points.len() as u128;
    let mut cpu_sum = 0.0_f64;
    let mut others_sum = 0.0_f64;
    let mut memory_sum = 0_u128;
    let mut rollup = SystemHour {
        hour,
        samples: points.len() as u32,
        cpu_avg: 0.0,
        cpu_max: f64::MIN,
        memory_used_avg: 0,
        memory_used_max: 0,
        memory_total: 0,
        disk_read: 0,
        disk_write: 0,
        net_received: 0,
        net_sent: 0,
        others_cpu_avg: 0.0,
    };
    for point in points {
        let cpu = f64::from(point.system.cpu_percent);
        cpu_sum += cpu;
        rollup.cpu_max = rollup.cpu_max.max(cpu);
        others_sum += f64::from(point.others.cpu_percent);
        memory_sum += u128::from(point.system.memory_used_bytes);
        rollup.memory_used_max = rollup.memory_used_max.max(point.system.memory_used_bytes);
        rollup.memory_total = rollup.memory_total.max(point.system.memory_total_bytes);
        rollup.disk_read = rollup.disk_read.saturating_add(point.system.disk_read_bytes);
        rollup.disk_write = rollup.disk_write.saturating_add(point.system.disk_write_bytes);
        rollup.net_received = rollup.net_received.saturating_add(point.system.net_received_bytes);
        rollup.net_sent = rollup.net_sent.saturating_add(point.system.net_sent_bytes);
    }
    rollup.cpu_avg = cpu_sum / n as f64;
    rollup.others_cpu_avg = others_sum / n as f64;
    rollup.memory_used_avg = u64::try_from(memory_sum / n).unwrap_or(u64::MAX);
    Some(rollup)
}

/// Per-executable working state while an hour is summed. `BTreeMap` keys keep every iteration in one fixed order.
#[derive(Default)]
struct ProcessAccumulator {
    paths: BTreeSet<String>,
    /// Per sample timestamp: (cpu summed over the executable's instances, memory summed likewise).
    per_sample: BTreeMap<Timestamp, (f64, u64)>,
    starts: u32,
    first_seen: Option<Timestamp>,
    last_seen: Option<Timestamp>,
}

impl ProcessAccumulator {
    fn see(&mut self, ts: Timestamp) {
        self.first_seen = Some(self.first_seen.map_or(ts, |first| first.min(ts)));
        self.last_seen = Some(self.last_seen.map_or(ts, |last| last.max(ts)));
    }
}

fn executable_of(paths: &BTreeSet<String>, key: &Option<String>) -> Option<Executable> {
    let key = key.clone()?;
    let path = paths.iter().next().cloned().unwrap_or_else(|| key.clone());
    Some(Executable { key, path })
}

fn process_hours(input: &HourInput, hour_samples: u32) -> Vec<ProcessHour> {
    let mut by_exe: BTreeMap<Option<String>, ProcessAccumulator> = BTreeMap::new();

    // Sorted first, so floating-point sums are added in one fixed order whatever order the repository returned the rows in.
    let mut samples: Vec<&ProcessSampleRow> =
        input.process_samples.iter().filter(|s| in_hour(input.hour, s.ts)).collect();
    samples.sort_by(|a, b| {
        (a.ts, executable_key_of(&a.exe_path), a.cpu_percent.to_bits(), a.memory_bytes).cmp(&(
            b.ts,
            executable_key_of(&b.exe_path),
            b.cpu_percent.to_bits(),
            b.memory_bytes,
        ))
    });
    for sample in samples {
        let entry = by_exe.entry(executable_key_of(&sample.exe_path)).or_default();
        if let Some(path) = sample.exe_path.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
            entry.paths.insert(path.to_owned());
        }
        let slot = entry.per_sample.entry(sample.ts).or_insert((0.0, 0));
        slot.0 += f64::from(sample.cpu_percent);
        slot.1 = slot.1.saturating_add(sample.memory_bytes);
        entry.see(sample.ts);
    }
    for start in input.starts.iter().filter(|s| in_hour(input.hour, s.ts)) {
        let entry = by_exe.entry(executable_key_of(&start.exe_path)).or_default();
        if let Some(path) = start.exe_path.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
            entry.paths.insert(path.to_owned());
        }
        entry.starts += 1;
        entry.see(start.ts);
    }

    by_exe
        .into_iter()
        .filter_map(|(key, acc)| {
            let (first_seen, last_seen) = (acc.first_seen?, acc.last_seen?);
            let cpu_total: f64 = acc.per_sample.values().map(|(cpu, _)| cpu).sum();
            Some(ProcessHour {
                hour: input.hour,
                exe: executable_of(&acc.paths, &key),
                samples_present: acc.per_sample.len() as u32,
                cpu_avg: if hour_samples == 0 { 0.0 } else { cpu_total / f64::from(hour_samples) },
                cpu_peak: acc.per_sample.values().map(|(cpu, _)| *cpu).fold(0.0, f64::max),
                memory_peak: acc.per_sample.values().map(|(_, memory)| *memory).max().unwrap_or(0),
                starts: acc.starts,
                first_seen,
                last_seen,
            })
        })
        .collect()
}

fn executable_key_of(path: &Option<String>) -> Option<String> {
    path.as_deref().and_then(executable_key)
}

#[derive(Default)]
struct TrafficAccumulator {
    paths: BTreeSet<String>,
    bytes_sent: u64,
    bytes_received: u64,
    flows: u32,
    first_seen: Option<Timestamp>,
    last_seen: Option<Timestamp>,
    bytes_by_port: BTreeMap<u16, u64>,
}

fn traffic_hours(input: &HourInput) -> Vec<TrafficHour> {
    let mut by_pair: BTreeMap<(Option<String>, IpAddr), TrafficAccumulator> = BTreeMap::new();
    for flow in input.flows.iter().filter(|f| in_hour(input.hour, f.window_start)) {
        let acc = by_pair.entry((executable_key_of(&flow.exe_path), flow.remote_addr)).or_default();
        if let Some(path) = flow.exe_path.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
            acc.paths.insert(path.to_owned());
        }
        acc.bytes_sent = acc.bytes_sent.saturating_add(flow.bytes_sent);
        acc.bytes_received = acc.bytes_received.saturating_add(flow.bytes_received);
        acc.flows += 1;
        acc.first_seen = Some(acc.first_seen.map_or(flow.window_start, |t| t.min(flow.window_start)));
        acc.last_seen = Some(acc.last_seen.map_or(flow.window_start, |t| t.max(flow.window_start)));
        let port_bytes = acc.bytes_by_port.entry(flow.remote_port).or_insert(0);
        *port_bytes = port_bytes.saturating_add(flow.bytes_sent).saturating_add(flow.bytes_received);
    }
    by_pair
        .into_iter()
        .filter_map(|((key, addr), acc)| {
            // BTreeMap iterates ports in ascending order and `max_by_key` keeps the LAST maximum, so search in reverse
            // to keep the lowest port on a tie.
            let top_port = acc.bytes_by_port.iter().rev().max_by_key(|(_, bytes)| **bytes).map(|(port, _)| *port)?;
            Some(TrafficHour {
                hour: input.hour,
                exe: executable_of(&acc.paths, &key),
                addr,
                bytes_sent: acc.bytes_sent,
                bytes_received: acc.bytes_received,
                flows: acc.flows,
                first_seen: acc.first_seen?,
                last_seen: acc.last_seen?,
                distinct_ports: acc.bytes_by_port.len() as u32,
                top_port,
            })
        })
        .collect()
}

/// Turns one hour of detail rows into its rollup. Rows outside the hour are ignored, so a loader that returns too much cannot
/// leak another hour into this one.
pub fn roll_up_hour(input: &HourInput) -> HourRollup {
    let mut points: Vec<&SystemPoint> = input.system.iter().filter(|p| in_hour(input.hour, p.ts)).collect();
    points.sort_by_key(|p| p.ts);
    let system = system_hour(input.hour, &points);

    // Absent processes count as zero, so the denominator is the number of samples in the hour. System samples are the
    // reference; if there are none (they always accompany process samples), fall back to the distinct process sample times.
    let hour_samples = match &system {
        Some(system) => system.samples,
        None => input
            .process_samples
            .iter()
            .filter(|s| in_hour(input.hour, s.ts))
            .map(|s| s.ts)
            .collect::<BTreeSet<_>>()
            .len() as u32,
    };

    HourRollup {
        hour: input.hour,
        system,
        processes: process_hours(input, hour_samples),
        traffic: traffic_hours(input),
        names: input.address_names(),
    }
}
