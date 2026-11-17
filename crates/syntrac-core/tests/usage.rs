//! The usage rules (docs/ALERTS-PLAN.md, F3): a program using clearly more than it usually does, judged against its OWN last 14 days.
//! All pure: hourly rows in, candidates out.

use syntrac_core::findings::Tier;
use syntrac_core::usage::{
    cpu_floor_for_cores, usage_candidates, FlaggedSpan, ProcessHourRow, TrafficHourRow, UsageFacts, UsageSettings,
};
use syntrac_core::{Candidate, Executable, Timestamp};

const HOUR: i64 = 3_600_000_000;
const MB: u64 = 1024 * 1024;
const PROGRAM: &str = r"C:\Apps\worker.exe";

fn at_hour(n: i64) -> Timestamp {
    Timestamp::from_micros(n * HOUR)
}

fn exe() -> Executable {
    Executable::from_path(PROGRAM).unwrap()
}

/// 14 days of history (hours 0..336), then the two hours being judged (336 and 337); `now` is shortly after the last of them.
const LATEST: i64 = 337;
const HISTORY_HOURS: i64 = 336;

fn now() -> Timestamp {
    Timestamp::from_micros((LATEST + 1) * HOUR + 10 * 60 * 1_000_000)
}

struct Series {
    process: Vec<ProcessHourRow>,
    traffic: Vec<TrafficHourRow>,
}

impl Series {
    /// Steady use every hour of the history: 2% CPU, 100 MB peak memory, 5 MB up and 20 MB down.
    fn steady() -> Self {
        let mut series = Self { process: Vec::new(), traffic: Vec::new() };
        for hour in 0..HISTORY_HOURS {
            series.set(hour, 2.0, 100 * MB, 5 * MB, 20 * MB);
        }
        series
    }

    fn set(&mut self, hour: i64, cpu: f64, memory: u64, sent: u64, received: u64) {
        self.process.retain(|row| row.hour != at_hour(hour));
        self.traffic.retain(|row| row.hour != at_hour(hour));
        self.process.push(ProcessHourRow { exe: exe(), hour: at_hour(hour), cpu_avg: cpu, memory_peak: memory });
        self.traffic.push(TrafficHourRow {
            exe: exe(),
            hour: at_hour(hour),
            bytes_sent: sent,
            bytes_received: received,
        });
    }

    fn two_busy_hours(mut self, cpu: f64, memory: u64, sent: u64, received: u64) -> Self {
        self.set(LATEST - 1, cpu, memory, sent, received);
        self.set(LATEST, cpu, memory, sent, received);
        self
    }

    fn run(&self) -> Vec<Candidate> {
        self.run_with(&UsageSettings::default(), &[], now())
    }

    fn run_with(&self, settings: &UsageSettings, flagged: &[FlaggedSpan], now: Timestamp) -> Vec<Candidate> {
        usage_candidates(&UsageFacts { now, settings, process: &self.process, traffic: &self.traffic, flagged })
    }
}

fn of(found: &[Candidate], metric: &str) -> Vec<Candidate> {
    found.iter().filter(|c| c.subject.ends_with(&format!("|{metric}"))).cloned().collect()
}

#[test]
fn steady_use_raises_nothing() {
    let mut series = Series::steady();
    series.set(LATEST - 1, 2.0, 100 * MB, 5 * MB, 20 * MB);
    series.set(LATEST, 2.0, 100 * MB, 5 * MB, 20 * MB);

    assert!(series.run().is_empty());
}

#[test]
fn two_hours_of_much_more_cpu_than_ever_before_is_raised_with_the_numbers() {
    let found = Series::steady().two_busy_hours(60.0, 100 * MB, 5 * MB, 20 * MB).run();

    let cpu = of(&found, "cpu");
    assert_eq!(cpu.len(), 1);
    assert_eq!(cpu[0].kind, "usage");
    assert_eq!(cpu[0].subject, format!("{}|cpu", PROGRAM.to_lowercase()));
    assert_eq!(cpu[0].tier, Tier::Unusual);
    assert_eq!(cpu[0].level, Some(60.0));
    assert!(cpu[0].why.contains("60.0%"), "{}", cpu[0].why);
    assert!(cpu[0].why.contains("95%"), "{}", cpu[0].why);
    assert!(cpu[0].evidence.iter().any(|line| line.contains(PROGRAM)));
    assert!(cpu[0].evidence.iter().any(|line| line.contains("336 active hours")));
    assert!(of(&found, "memory").is_empty() && of(&found, "sent").is_empty(), "only the metric that moved");
}

#[test]
fn one_busy_hour_is_not_enough_for_cpu_and_neither_is_a_rise_below_the_floor() {
    let mut one = Series::steady();
    one.set(LATEST, 60.0, 100 * MB, 5 * MB, 20 * MB);
    assert!(of(&one.run(), "cpu").is_empty(), "sustained means two hours in a row");

    let small = Series::steady().two_busy_hours(20.0, 100 * MB, 5 * MB, 20 * MB);
    assert!(of(&small.run(), "cpu").is_empty(), "ten times the usual, but still under the 25% floor");
}

#[test]
fn a_busy_hour_before_the_last_one_does_not_count_when_the_last_one_was_quiet() {
    let mut series = Series::steady();
    series.set(LATEST - 1, 60.0, 100 * MB, 5 * MB, 20 * MB);
    // The last hour has no row at all: it was quiet.
    assert!(of(&series.run(), "cpu").is_empty());
}

#[test]
fn a_program_without_enough_history_is_not_judged() {
    let mut series = Series { process: Vec::new(), traffic: Vec::new() };
    for hour in (HISTORY_HOURS - 30)..HISTORY_HOURS {
        series.set(hour, 2.0, 100 * MB, 5 * MB, 20 * MB);
    }
    let series = series.two_busy_hours(80.0, 100 * MB, 5 * MB, 20 * MB);

    assert!(series.run().is_empty(), "30 active hours is not a baseline");
}

#[test]
fn a_rise_in_memory_needs_to_be_well_above_the_usual_peak_and_by_the_floor() {
    let big = Series::steady().two_busy_hours(2.0, 800 * MB, 5 * MB, 20 * MB);
    let found = of(&big.run(), "memory");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].level, Some((800 * MB) as f64));
    assert!(found[0].why.contains("800.0 MB"), "{}", found[0].why);

    let small = Series::steady().two_busy_hours(2.0, 300 * MB, 5 * MB, 20 * MB);
    assert!(of(&small.run(), "memory").is_empty(), "three times the usual, but not by 500 MB");
}

#[test]
fn sustained_network_use_is_raised_and_one_enormous_hour_of_uploading_is_raised_alone() {
    let sustained = Series::steady().two_busy_hours(2.0, 100 * MB, 400 * MB, 20 * MB);
    assert_eq!(of(&sustained.run(), "sent").len(), 1);

    let mut spike = Series::steady();
    spike.set(LATEST, 2.0, 100 * MB, 800 * MB, 20 * MB);
    let sent = of(&spike.run(), "sent");
    assert_eq!(sent.len(), 1, "ten times the busiest hour ever, in one hour");
    assert!(sent[0].why.contains("upload") || sent[0].title.contains("upload"));

    let mut download = Series::steady();
    download.set(LATEST, 2.0, 100 * MB, 5 * MB, 800 * MB);
    assert!(of(&download.run(), "received").is_empty(), "one hour of downloading is not the exfiltration shape");
}

#[test]
fn hours_that_already_raised_a_finding_are_left_out_of_the_baseline() {
    let mut series = Series::steady();
    // Three earlier busy hours (50%) that the user was told about.
    for hour in 100..103 {
        series.set(hour, 50.0, 100 * MB, 5 * MB, 20 * MB);
    }
    let series = series.two_busy_hours(60.0, 100 * MB, 5 * MB, 20 * MB);

    assert!(of(&series.run(), "cpu").is_empty(), "the earlier busy hours stretch the usual maximum to 50%");

    let flagged =
        [FlaggedSpan { subject: format!("{}|cpu", PROGRAM.to_lowercase()), from: at_hour(100), to: at_hour(103) }];
    let found = series.run_with(&UsageSettings::default(), &flagged, now());
    assert_eq!(of(&found, "cpu").len(), 1, "without them the usual is about 2%");
}

#[test]
fn rollups_that_stopped_long_ago_raise_nothing() {
    let series = Series::steady().two_busy_hours(60.0, 100 * MB, 5 * MB, 20 * MB);
    let much_later = Timestamp::from_micros((LATEST + 30) * HOUR);

    assert!(series.run_with(&UsageSettings::default(), &[], much_later).is_empty());
}

#[test]
fn the_same_thing_all_day_is_one_finding_and_the_next_day_is_another() {
    let series = Series::steady().two_busy_hours(60.0, 100 * MB, 5 * MB, 20 * MB);
    let first = of(&series.run(), "cpu")[0].dedupe_key.clone();

    // One hour later the same two-hour spell (now hours 337 and 338), still the same UTC day.
    let mut later = Series::steady();
    later.set(LATEST, 60.0, 100 * MB, 5 * MB, 20 * MB);
    later.set(LATEST + 1, 60.0, 100 * MB, 5 * MB, 20 * MB);
    let later_now = Timestamp::from_micros((LATEST + 2) * HOUR + 600_000_000);
    let second = of(&later.run_with(&UsageSettings::default(), &[], later_now), "cpu")[0].dedupe_key.clone();

    assert_eq!(first, second);

    // A day later it is a new finding.
    let mut next_day = Series::steady();
    next_day.set(LATEST + 24, 60.0, 100 * MB, 5 * MB, 20 * MB);
    next_day.set(LATEST + 25, 60.0, 100 * MB, 5 * MB, 20 * MB);
    let next_now = Timestamp::from_micros((LATEST + 26) * HOUR + 600_000_000);
    let third = of(&next_day.run_with(&UsageSettings::default(), &[], next_now), "cpu");
    assert_eq!(third.len(), 1);
    assert_ne!(first, third[0].dedupe_key);
}

#[test]
fn the_floors_and_the_history_needed_come_from_the_settings() {
    let series = Series::steady().two_busy_hours(20.0, 100 * MB, 5 * MB, 20 * MB);
    let sensitive = UsageSettings { cpu_floor_percent: 10.0, ..UsageSettings::default() };

    assert_eq!(of(&series.run_with(&sensitive, &[], now()), "cpu").len(), 1);

    let demanding = UsageSettings { min_active_hours: 1000, ..UsageSettings::default() };
    let busy = Series::steady().two_busy_hours(60.0, 100 * MB, 5 * MB, 20 * MB);
    assert!(busy.run_with(&demanding, &[], now()).is_empty());
}

#[test]
fn the_cpu_floor_is_half_of_one_core_of_this_machine_never_below_one_and_a_half_percent() {
    assert_eq!(cpu_floor_for_cores(4), 12.5);
    assert_eq!(cpu_floor_for_cores(16), 3.125);
    assert_eq!(cpu_floor_for_cores(64), 1.5, "many cores: the floor stops falling");
    assert_eq!(cpu_floor_for_cores(0), 50.0, "an unknown count is treated as one core");
}
