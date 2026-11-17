//! Backtest of the "new thing" rules on a COPY of a real database. The source is only read.
//!
//!   cargo run --release -p syntrac-store --example rehearse_detect -- <source.db> <work-dir> [days-ago ...]
//!
//! The copy is made with `VACUUM INTO` and upgraded to the latest schema. For each number of days (default 1 2 3 5) it asks: if
//! detection had been switched on that many days ago, what would the rules raise NOW? It prints how many findings each rule would
//! have raised, by tier, and a sample of the sentences, so the rules are read by a person before detection is switched on
//! (`detect_enabled = false` until then). Nothing here deletes anything; the work directory is left for you to inspect and remove.
//! The usage rules are replayed too: the detector is run at every sixth hour of the last days and what it would have raised is added
//! up (the same thing on one day counts once), so the noise of the thresholds can be judged.
//! File changes need a history that only exists from schema v14 on, so on a database that has just been upgraded that rule is quiet.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use rusqlite::{Connection, OpenFlags};
use syntrac_core::findings::Tier;
use syntrac_core::{
    cpu_floor_for_cores, Clock, Detector, NewThingsDetector, SystemClock, Timestamp, UsageDetector, UsageSettings,
};
use syntrac_store::SqliteStore;

const DAY_MICROS: i64 = 86_400 * 1_000_000;

fn main() {
    let mut args = std::env::args().skip(1);
    let source = PathBuf::from(args.next().expect("source database"));
    let work = PathBuf::from(args.next().expect("work directory"));
    let mut days: Vec<i64> = args.filter_map(|a| a.parse().ok()).collect();
    if days.is_empty() {
        days = vec![1, 2, 3, 5];
    }
    std::fs::create_dir_all(&work).expect("create work dir");
    let copy = work.join("detect-rehearsal.db");
    assert!(!copy.exists(), "{} already exists; remove it first", copy.display());

    let started = Instant::now();
    {
        let src =
            Connection::open_with_flags(&source, OpenFlags::SQLITE_OPEN_READ_ONLY).expect("open source read-only");
        src.execute("VACUUM INTO ?1", [copy.to_string_lossy().as_ref()]).expect("copy");
    }
    println!("copied in {:.1}s", started.elapsed().as_secs_f64());
    let store = SqliteStore::open(&copy).expect("open and upgrade the copy");
    println!("schema v{}", store.schema_version().unwrap());
    let detector = NewThingsDetector::new(store);
    let now = SystemClock.now();

    let replay_days = days.iter().copied().max().unwrap_or(5);
    for ago in days {
        let since = Timestamp::from_micros(now.as_micros() - ago * DAY_MICROS);
        let started = Instant::now();
        let found = detector.detect(since, now).expect("the rules ran");
        println!(
            "\nswitched on {ago} day(s) ago: {} finding(s) in {:.0} ms",
            found.len(),
            started.elapsed().as_secs_f64() * 1000.0
        );
        let mut by_kind: BTreeMap<(&str, &str), usize> = BTreeMap::new();
        for candidate in &found {
            *by_kind.entry((candidate.kind.as_str(), candidate.tier.as_str())).or_default() += 1;
        }
        for ((kind, tier), count) in &by_kind {
            println!("  {count:4}  {kind} ({tier})");
        }
        for candidate in found
            .iter()
            .filter(|c| c.tier != Tier::Unusual)
            .take(6)
            .chain(found.iter().filter(|c| c.tier == Tier::Unusual).take(4))
        {
            println!("  [{}] {} -- {}", candidate.tier.as_str(), candidate.title, candidate.why);
        }
    }
    let cores = std::thread::available_parallelism().map_or(1, usize::from);
    let shipped = UsageSettings { cpu_floor_percent: cpu_floor_for_cores(cores), ..UsageSettings::default() };
    usage_replay("as shipped", &copy, now, replay_days, shipped);
    // For comparison only: how close the data comes to the limits. Not a suggestion to run it so.
    let sensitive = UsageSettings {
        cpu_floor_percent: 1.0,
        memory_floor_bytes: 50 * 1024 * 1024,
        net_floor_bytes: 10 * 1024 * 1024,
        p95_multiplier: 1.5,
        max_multiplier: 1.1,
        ..shipped
    };
    usage_replay("much more sensitive (comparison only)", &copy, now, replay_days, sensitive);
    println!("\ncopy left at {}", copy.display());
}

/// Runs the usage rules at every sixth hour of the last `days` days and reports what they would have raised.
fn usage_replay(label: &str, copy: &std::path::Path, now: Timestamp, days: i64, settings: UsageSettings) {
    let detector = UsageDetector::new(SqliteStore::open(copy).expect("open the copy"), settings);
    let mut raised: BTreeMap<String, (String, String, String)> = BTreeMap::new();
    let started = Instant::now();
    let mut runs = 0;
    let mut at = now.as_micros();
    while at > now.as_micros() - days * DAY_MICROS {
        let found =
            detector.detect(Timestamp::from_micros(0), Timestamp::from_micros(at)).expect("the usage rules ran");
        runs += 1;
        for candidate in found {
            raised.entry(candidate.dedupe_key.clone()).or_insert((
                candidate.subject.clone(),
                candidate.title.clone(),
                candidate.why.clone(),
            ));
        }
        at -= 6 * 3_600_000_000;
    }
    println!(
        "\nusage rules ({label}) replayed {runs} times over {days} day(s) in {:.1} s: {} finding(s) (one per program, metric and day)",
        started.elapsed().as_secs_f64(),
        raised.len()
    );
    let mut by_metric: BTreeMap<&str, usize> = BTreeMap::new();
    for (subject, _, _) in raised.values() {
        *by_metric.entry(subject.rsplit('|').next().unwrap_or("?")).or_default() += 1;
    }
    for (metric, count) in &by_metric {
        println!("  {count:4}  {metric}");
    }
    for (_, title, why) in raised.values().take(8) {
        println!("  {title} -- {why}");
    }
}
