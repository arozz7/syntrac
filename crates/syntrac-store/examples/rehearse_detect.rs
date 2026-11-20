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
//! The known patterns (F5a) are matched against everything seen: counts per pattern (recent days and all time), the two branches of
//! the start-entry pattern counted separately, and the first matches by name. The output names programs on this machine: keep it local.
//! File changes need a history that only exists from schema v14 on, so on a database that has just been upgraded that rule is quiet.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use rusqlite::{Connection, OpenFlags};
use syntrac_core::findings::Tier;
use syntrac_core::{
    built_in_pack, cpu_floor_for_cores, parse_pack, pattern_candidates, Clock, DestinationTrendRepository, Detector,
    IdentityRepository, KnownProductRepository, LauncherRepository, NewThingsDetector, PatternFacts, SpawnRepository,
    SystemClock, Timestamp, UsageDetector, UsageRepository, UsageSettings, RECENT_DAYS,
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
    gpu_report(&copy, now);
    pattern_report(&copy, now);
    println!("\ncopy left at {}", copy.display());
}

/// Two extra patterns that are NOT shipped: the two branches of "a start entry whose target is unsigned or lives in a user-writable
/// folder", counted separately so the noise of each can be judged before deciding what to ship.
const START_ENTRY_BRANCHES: &str = r#"
version = "rehearsal"
[[pattern]]
id = "start_unsigned_anywhere"
title = "Start entry, unsigned target anywhere"
technique = "T1547.001"
tier = "unusual"
applies_to = "start_entry"
why = "Rehearsal only."
benign = "Rehearsal only."
[pattern.when]
entry_kind_is = ["service_auto", "task", "run_key", "run_once", "startup_folder"]
signature_is = ["unsigned", "untrusted", "altered"]
[[pattern]]
id = "start_signed_in_writable"
title = "Start entry, signed target in a user-writable folder"
technique = "T1547.001"
tier = "unusual"
applies_to = "start_entry"
why = "Rehearsal only."
benign = "Rehearsal only."
[pattern.when]
entry_kind_is = ["service_auto", "task", "run_key", "run_once", "startup_folder"]
signature_is = ["valid"]
location_is = ["user_profile", "temporary", "program_data", "network"]
"#;

/// What each known pattern matches TODAY on the copy, for the recent days the service uses and for all time, with the programs, so a
/// person can read the list before `patterns_enabled` is switched on. The output names programs on this machine: keep it local.
fn pattern_report(copy: &std::path::Path, now: Timestamp) {
    let store = SqliteStore::open(copy).expect("open the copy");
    let entries = store.identity_entries().expect("programs");
    let known = store.known_products().expect("known programs");
    let launchers = store.all_launchers().expect("start entries");
    let since = Timestamp::from_micros(now.as_micros() - DAY_MICROS); // as if switched on yesterday: older things are "already here"
    let first_online = store.first_online_since(since).expect("first network use");
    let gpu_hours = store
        .process_hour_rows(Timestamp::from_micros(now.as_micros() - 28 * 3_600_000_000), now)
        .expect("the hourly GPU use");
    let shipped = built_in_pack();
    let branches = parse_pack(START_ENTRY_BRANCHES).expect("rehearsal pack");
    println!("\nknown patterns (pack {}), {} program(s) in the copy", shipped.version, entries.len());
    for (label, recent_days) in
        [("seen in the last 30 days (as the service judges)", RECENT_DAYS), ("all time (comparison only)", 36_500)]
    {
        println!("\n  {label}:");
        let child_names = shipped.started_program_names();
        let recent_since = Timestamp::from_micros(now.as_micros() - recent_days * DAY_MICROS);
        let spawns = store.spawn_pairs(recent_since, &child_names).expect("which programs started which");
        println!(
            "   {} start pair(s) of the programs the patterns care about, {} program(s) first online since yesterday",
            spawns.len(),
            first_online.len()
        );
        for (name, pack) in [("shipped", &shipped), ("branches of the start-entry pattern (not shipped)", &branches)] {
            let found = pattern_candidates(
                pack,
                &PatternFacts {
                    since,
                    now,
                    recent_days,
                    known: &known,
                    entries: &entries,
                    launchers: &launchers,
                    first_online: &first_online,
                    spawns: &spawns,
                    gpu_hours: &gpu_hours,
                },
            );
            let mut by_kind: BTreeMap<(&str, &str), usize> = BTreeMap::new();
            for candidate in &found {
                *by_kind.entry((candidate.kind.as_str(), candidate.tier.as_str())).or_default() += 1;
            }
            println!("   {name}: {} match(es)", found.len());
            for pattern in &pack.patterns {
                let kind = format!("pattern_{}", pattern.id);
                let counts: Vec<usize> =
                    by_kind.iter().filter(|((k, _), _)| *k == kind).map(|(_, count)| *count).collect();
                println!("    {:4}  {}", counts.iter().sum::<usize>(), pattern.id);
            }
            if recent_days == RECENT_DAYS && name == "shipped" {
                for candidate in found.iter().take(40) {
                    println!("      [{}] {} -- {}", candidate.tier.as_str(), candidate.kind, candidate.subject);
                }
            }
        }
    }
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

/// GPU use (docs/ALERTS-PLAN.md, slice G): how much history there is for the rules to judge. A database made before the GPU was
/// recorded has none, and then the GPU rules cannot be judged on this data: this says so instead of printing zeros that look like a result.
fn gpu_report(copy: &std::path::Path, now: Timestamp) {
    let store = SqliteStore::open(copy).expect("open the copy");
    let from = Timestamp::from_micros(now.as_micros() - 14 * DAY_MICROS);
    let rows = store.process_hour_rows(from, now).expect("the hourly rows");
    let mut active_hours: BTreeMap<&str, usize> = BTreeMap::new();
    for row in rows.iter().filter(|row| row.gpu_avg > 0.0 || row.gpu_memory_peak > 0) {
        *active_hours.entry(row.exe.key.as_str()).or_default() += 1;
    }
    let busy = rows.iter().filter(|row| row.gpu_avg >= 30.0).count();
    println!("\nGPU use, last 14 days of hourly rows ({} program-hour rows in all):", rows.len());
    if active_hours.is_empty() {
        println!(
            "  no GPU history yet: no hour of this copy has GPU use recorded (it was made before the GPU was recorded, or the counters are not available).\n  \
             The GPU usage rule and the GPU pattern cannot be judged on this data; run this again after about two weeks of capture."
        );
        return;
    }
    let judgeable = active_hours.values().filter(|hours| **hours >= 48).count();
    println!(
        "  {} program(s) with GPU use in at least one hour; {} with 48 or more active GPU hours (the usage rule can judge them); {busy} program-hour(s) at 30% or more of one engine",
        active_hours.len(),
        judgeable
    );
}
