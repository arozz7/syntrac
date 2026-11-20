//! The usage rules (docs/ALERTS-PLAN.md, F3): a program using clearly more than it usually does, judged against its OWN recent history.
//! Explainable statistics only: for each program and metric the last two completed hours are compared with the program's earlier
//! hours (the 95th percentile and the maximum of its ACTIVE hours), and the numbers are in the sentence. No machine learning.
//!
//! Covers per program: CPU (average share of the whole machine in the hour), memory (peak in the hour), GPU (average share of the
//! program's busiest GPU engine in the hour, NOT of the machine) and GPU memory (peak), bytes sent and bytes
//! received. NOT disk per program: the hourly rollups hold no per-program I/O.
//!
//! A finding needs ALL of: more than `max(3 x the 95th percentile, 1.5 x the highest hour)`, over an absolute floor, and sustained
//! for the last two hours in a row. One exception: ONE hour of uploading more than ten times the highest hour (the shape of data
//! leaving) is raised alone. Hours that already raised a finding are left out of the baseline, so a spell the user was told about
//! does not make the next one look normal. A program with fewer than `min_active_hours` active hours is not judged at all.

use std::collections::{BTreeMap, HashMap};

use crate::findings::{Candidate, Tier};
use crate::findings_run::{DetectError, Detector, FindingRepository};
use crate::ports::RepoError;
use crate::rollup::Executable;
use crate::time::Timestamp;

const HOUR: i64 = 3_600_000_000;
const DAY: i64 = 24 * HOUR;
const MB: u64 = 1024 * 1024;

/// The most usage findings one run returns; the rest is noise, not information.
pub const MAX_USAGE_PER_RUN: usize = 100;

/// More than this long after the newest hour of data, the rollups are considered stopped and nothing is raised.
pub(crate) const STALE_AFTER: i64 = 4 * HOUR;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UsageSettings {
    /// Hours of history that make the baseline (the hours right before the two being judged).
    pub window_hours: i64,
    /// Active hours (hours with any use) a program needs before it is judged.
    pub min_active_hours: usize,
    /// CPU must be at least this share of the whole machine.
    pub cpu_floor_percent: f64,
    /// Memory must be at least this much above the program's highest peak.
    pub memory_floor_bytes: u64,
    /// GPU (a share of ONE engine, never scaled by cores) must be at least this.
    pub gpu_floor_percent: f64,
    /// GPU memory must be at least this much above the program's highest peak.
    pub gpu_memory_floor_bytes: u64,
    /// Bytes sent or received in an hour must be at least this.
    pub net_floor_bytes: u64,
    pub p95_multiplier: f64,
    pub max_multiplier: f64,
    /// One hour of uploading more than this times the highest hour is raised alone.
    pub single_hour_multiplier: f64,
}

impl Default for UsageSettings {
    fn default() -> Self {
        Self {
            window_hours: 14 * 24,
            min_active_hours: 48,
            cpu_floor_percent: 25.0,
            memory_floor_bytes: 500 * MB,
            gpu_floor_percent: 20.0,
            gpu_memory_floor_bytes: 500 * MB,
            net_floor_bytes: 100 * MB,
            p95_multiplier: 3.0,
            max_multiplier: 1.5,
            single_hour_multiplier: 10.0,
        }
    }
}

/// The default CPU floor for a machine with this many logical cores: half of one core, as a share of the whole machine, but never below
/// 1.5% (a program's CPU is recorded as a share of the whole machine, so a flat percentage means very different things on 4 and on 32 cores).
pub fn cpu_floor_for_cores(cores: usize) -> f64 {
    (50.0 / cores.max(1) as f64).max(1.5)
}

/// One hour of one program's CPU, memory and GPU, from the hourly rollups.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessHourRow {
    pub exe: Executable,
    /// The start of the hour.
    pub hour: Timestamp,
    /// Average CPU as a percentage of the whole machine.
    pub cpu_avg: f64,
    pub memory_peak: u64,
    /// Average share of the program's busiest GPU engine in the hour (see `gpu`); 0 also means "no GPU data" (an hour rolled up before the GPU was recorded).
    pub gpu_avg: f64,
    /// Highest dedicated GPU memory in the hour.
    pub gpu_memory_peak: u64,
}

/// One hour of one program's network traffic (all addresses added up, loopback left out).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrafficHourRow {
    pub exe: Executable,
    pub hour: Timestamp,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

/// Where the hourly rows are read from.
pub trait UsageRepository {
    /// Every program's CPU and memory for the hours starting in `[from, to]`.
    fn process_hour_rows(&self, from: Timestamp, to: Timestamp) -> Result<Vec<ProcessHourRow>, RepoError>;

    /// Every program's traffic for the hours starting in `[from, to]`.
    fn traffic_hour_rows(&self, from: Timestamp, to: Timestamp) -> Result<Vec<TrafficHourRow>, RepoError>;
}

/// A stretch of time in which the user was already told about a usage finding for this subject; its hours are not part of the
/// baseline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlaggedSpan {
    pub subject: String,
    pub from: Timestamp,
    pub to: Timestamp,
}

pub struct UsageFacts<'a> {
    pub now: Timestamp,
    pub settings: &'a UsageSettings,
    pub process: &'a [ProcessHourRow],
    pub traffic: &'a [TrafficHourRow],
    pub flagged: &'a [FlaggedSpan],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Metric {
    Cpu,
    Memory,
    /// A share of the program's busiest GPU engine (not of the machine).
    Gpu,
    GpuMemory,
    Sent,
    Received,
}

impl Metric {
    const ALL: [Metric; 6] = [Self::Cpu, Self::Memory, Self::Gpu, Self::GpuMemory, Self::Sent, Self::Received];

    fn slug(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Memory => "memory",
            Self::Gpu => "gpu",
            Self::GpuMemory => "gpu_memory",
            Self::Sent => "sent",
            Self::Received => "received",
        }
    }

    fn title(self, name: &str) -> String {
        match self {
            Self::Cpu => format!("{name} used more CPU than usual"),
            Self::Memory => format!("{name} used more memory than usual"),
            Self::Gpu => format!("{name} used more of the GPU than usual"),
            Self::GpuMemory => format!("{name} used more GPU memory than usual"),
            Self::Sent => format!("{name} uploaded more than usual"),
            Self::Received => format!("{name} downloaded more than usual"),
        }
    }

    fn show(self, value: f64) -> String {
        match self {
            Self::Cpu | Self::Gpu => format!("{value:.1}%"),
            _ => bytes(value),
        }
    }
}

fn bytes(value: f64) -> String {
    const KB: f64 = 1024.0;
    if value >= KB * KB * KB {
        format!("{:.1} GB", value / (KB * KB * KB))
    } else if value >= KB * KB {
        format!("{:.1} MB", value / (KB * KB))
    } else if value >= KB {
        format!("{:.1} KB", value / KB)
    } else {
        format!("{value:.0} B")
    }
}

/// Hour start (micros) to value, per program and metric.
type Series = HashMap<(String, Metric), BTreeMap<i64, f64>>;

fn series(facts: &UsageFacts<'_>) -> (Series, HashMap<String, Executable>) {
    let mut by_metric: Series = HashMap::new();
    let mut programs: HashMap<String, Executable> = HashMap::new();
    for row in facts.process {
        programs.entry(row.exe.key.clone()).or_insert_with(|| row.exe.clone());
        let hour = row.hour.as_micros();
        by_metric.entry((row.exe.key.clone(), Metric::Cpu)).or_default().insert(hour, row.cpu_avg);
        by_metric.entry((row.exe.key.clone(), Metric::Memory)).or_default().insert(hour, row.memory_peak as f64);
        by_metric.entry((row.exe.key.clone(), Metric::Gpu)).or_default().insert(hour, row.gpu_avg);
        by_metric.entry((row.exe.key.clone(), Metric::GpuMemory)).or_default().insert(hour, row.gpu_memory_peak as f64);
    }
    for row in facts.traffic {
        programs.entry(row.exe.key.clone()).or_insert_with(|| row.exe.clone());
        let hour = row.hour.as_micros();
        by_metric.entry((row.exe.key.clone(), Metric::Sent)).or_default().insert(hour, row.bytes_sent as f64);
        by_metric.entry((row.exe.key.clone(), Metric::Received)).or_default().insert(hour, row.bytes_received as f64);
    }
    (by_metric, programs)
}

/// The 95th percentile by nearest rank of a non-empty list.
fn percentile_95(sorted: &[f64]) -> f64 {
    let rank = ((sorted.len() as f64) * 0.95).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

fn file_name(path: &str) -> String {
    path.trim().rsplit(['\\', '/']).next().unwrap_or_default().to_owned()
}

struct Baseline {
    active_hours: usize,
    p95: f64,
    highest: f64,
}

fn baseline(
    values: &BTreeMap<i64, f64>,
    from: i64,
    until: i64,
    flagged: &[&FlaggedSpan],
    min_active: usize,
) -> Option<Baseline> {
    let mut active: Vec<f64> = values
        .range(from..until)
        .filter(|(hour, value)| {
            **value > 0.0
                && !flagged.iter().any(|span| span.from.as_micros() <= **hour && **hour <= span.to.as_micros())
        })
        .map(|(_, value)| *value)
        .collect();
    if active.len() < min_active.max(1) {
        return None;
    }
    active.sort_by(f64::total_cmp);
    Some(Baseline { active_hours: active.len(), p95: percentile_95(&active), highest: active[active.len() - 1] })
}

/// Every usage candidate of one run: the programs whose last two completed hours are well above their own earlier hours.
pub fn usage_candidates(facts: &UsageFacts<'_>) -> Vec<Candidate> {
    let settings = facts.settings;
    let latest = facts
        .process
        .iter()
        .map(|row| row.hour.as_micros())
        .chain(facts.traffic.iter().map(|row| row.hour.as_micros()))
        .max();
    let Some(latest) = latest else { return Vec::new() };
    if facts.now.as_micros() - latest > STALE_AFTER {
        return Vec::new();
    }
    let previous = latest - HOUR;
    let window_from = previous - settings.window_hours * HOUR;
    let day = latest / DAY;
    let (by_metric, programs) = series(facts);

    let mut found = Vec::new();
    for (key, exe) in &programs {
        for metric in Metric::ALL {
            let Some(values) = by_metric.get(&(key.clone(), metric)) else { continue };
            let subject = format!("{key}|{}", metric.slug());
            let flagged: Vec<&FlaggedSpan> = facts.flagged.iter().filter(|span| span.subject == subject).collect();
            let Some(usual) = baseline(values, window_from, previous, &flagged, settings.min_active_hours) else {
                continue;
            };
            let now_value = values.get(&latest).copied().unwrap_or(0.0);
            let before_value = values.get(&previous).copied().unwrap_or(0.0);
            let threshold = (settings.p95_multiplier * usual.p95).max(settings.max_multiplier * usual.highest);
            let floor_ok = |value: f64| match metric {
                Metric::Cpu => value >= settings.cpu_floor_percent,
                Metric::Memory => value >= usual.highest + settings.memory_floor_bytes as f64,
                Metric::Gpu => value >= settings.gpu_floor_percent,
                Metric::GpuMemory => value >= usual.highest + settings.gpu_memory_floor_bytes as f64,
                Metric::Sent | Metric::Received => value >= settings.net_floor_bytes as f64,
            };
            let clearly = |value: f64| value > threshold && floor_ok(value);
            let sustained = clearly(now_value) && clearly(before_value);
            let single = metric == Metric::Sent
                && now_value > settings.single_hour_multiplier * usual.highest
                && now_value >= settings.net_floor_bytes as f64;
            if !sustained && !single {
                continue;
            }
            let level = if sustained { now_value.max(before_value) } else { now_value };
            let name = file_name(&exe.path);
            let history = format!(
                "in the {} active hours before, 95% of hours were under {} and the busiest was {}",
                usual.active_hours,
                metric.show(usual.p95),
                metric.show(usual.highest)
            );
            let what = match metric {
                Metric::Cpu => "It used",
                Metric::Memory => "Its memory peaked at",
                Metric::Gpu => "It used",
                Metric::GpuMemory => "Its GPU memory peaked at",
                Metric::Sent => "It sent",
                Metric::Received => "It received",
            };
            let how = match metric {
                Metric::Cpu => " of the machine's CPU on average",
                Metric::Gpu => " of one GPU engine on average",
                _ => "",
            };
            let why = if sustained {
                format!("{what} {}{how} in each of the last two hours; {history}.", metric.show(level))
            } else {
                format!(
                    "{what} {}{how} in the last hour alone, more than ten times its busiest hour before; {history}.",
                    metric.show(level)
                )
            };
            found.push(Candidate {
                kind: "usage".into(),
                subject: subject.clone(),
                subject_label: name.clone(),
                tier: Tier::Unusual,
                title: metric.title(&name),
                why,
                evidence: vec![
                    format!("Program: {}", exe.path),
                    format!("Last two hours: {} then {}", metric.show(before_value), metric.show(now_value)),
                    format!(
                        "History: {} active hours in the {} hours before",
                        usual.active_hours, settings.window_hours
                    ),
                    format!(
                        "Usual: 95% of hours under {}, busiest {}",
                        metric.show(usual.p95),
                        metric.show(usual.highest)
                    ),
                    format!("Limit that was crossed: {}", metric.show(threshold)),
                ],
                dedupe_key: format!("usage:{subject}:{day}"),
                level: Some(level),
                at: facts.now,
            });
        }
    }
    found.sort_by(|a, b| a.dedupe_key.cmp(&b.dedupe_key));
    found.truncate(MAX_USAGE_PER_RUN);
    found
}

/// The usage rules as a detector: reads the hourly rows through `UsageRepository`, and the usage findings already raised (so their
/// hours stay out of the baseline) through `FindingRepository`.
pub struct UsageDetector<R> {
    repo: R,
    settings: UsageSettings,
}

impl<R> UsageDetector<R> {
    pub fn new(repo: R, settings: UsageSettings) -> Self {
        Self { repo, settings }
    }
}

impl<R: UsageRepository + FindingRepository> Detector for UsageDetector<R> {
    fn name(&self) -> &'static str {
        "usage"
    }

    fn detect(&self, _since: Timestamp, now: Timestamp) -> Result<Vec<Candidate>, DetectError> {
        let failed =
            |what: &'static str| move |error: RepoError| DetectError(format!("could not read {what}: {error}"));
        let from = Timestamp::from_micros(now.as_micros() - (self.settings.window_hours + 4) * HOUR);
        let process = self.repo.process_hour_rows(from, now).map_err(failed("the hourly program usage"))?;
        let traffic = self.repo.traffic_hour_rows(from, now).map_err(failed("the hourly program traffic"))?;
        let flagged: Vec<FlaggedSpan> = self
            .repo
            .findings(None, 500)
            .map_err(failed("the findings"))?
            .into_iter()
            .filter(|finding| finding.kind == "usage")
            .map(|finding| FlaggedSpan {
                subject: finding.subject,
                from: Timestamp::from_micros(finding.first_at.as_micros() - 2 * HOUR),
                to: Timestamp::from_micros(finding.last_at.as_micros() + HOUR),
            })
            .collect();
        Ok(usage_candidates(&UsageFacts {
            now,
            settings: &self.settings,
            process: &process,
            traffic: &traffic,
            flagged: &flagged,
        }))
    }
}
