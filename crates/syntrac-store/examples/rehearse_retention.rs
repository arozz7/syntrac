//! Rehearses S4 on a COPY of a real database: the one-time switch to incremental auto-vacuum, the gated purge with a short
//! retention, the storage guard trimming, and whether the file really shrinks, all with a read-only connection open (as the UI
//! keeps one). The source is only read.
//!
//!   cargo run --release -p syntrac-store --example rehearse_retention -- <source.db> <work-dir>

use std::path::PathBuf;
use std::time::{Duration, Instant};

use rusqlite::{Connection, OpenFlags};
use syntrac_collector::drive_space::DriveFreeSpace;
use syntrac_core::retention::{purge_cutoff, GuardLevel, GuardLimits};
use syntrac_core::storage_guard::{check_storage, FreeSpace, StorageStats};
use syntrac_core::{Clock, Retention, RollupRepository, SystemClock};
use syntrac_store::{inspect_database, SqliteStore};

fn mb(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_048_576.0)
}

fn file_bytes(path: &PathBuf) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn scalar(path: &PathBuf, sql: &str) -> i64 {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    conn.query_row(sql, [], |r| r.get::<_, Option<i64>>(0)).unwrap().unwrap_or(0)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let source = PathBuf::from(args.next().expect("usage: rehearse_retention <source.db> <work-dir>"));
    let work = PathBuf::from(args.next().expect("usage: rehearse_retention <source.db> <work-dir>"));
    std::fs::create_dir_all(&work).unwrap();
    let copy = work.join("retention-rehearsal.db");
    assert!(!copy.exists(), "{} already exists: use an empty work directory", copy.display());

    println!("== 1. Copy of {} (the source is only read)", source.display());
    Connection::open_with_flags(&source, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .unwrap()
        .execute("VACUUM INTO ?1", [copy.to_string_lossy().as_ref()])
        .unwrap();
    println!("   {} ({})", copy.display(), mb(file_bytes(&copy)));
    let mut failures = 0;

    println!("\n== 2. Open: the one-time switch to incremental auto-vacuum, with a reader already connected");
    let early_reader = Connection::open_with_flags(&copy, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let _: i64 = early_reader.query_row("SELECT COUNT(*) FROM system_metrics", [], |r| r.get(0)).unwrap();
    let before_mode: i64 = early_reader.pragma_query_value(None, "auto_vacuum", |r| r.get(0)).unwrap();
    let started = Instant::now();
    let mut store = SqliteStore::open(&copy).expect("open");
    println!(
        "   auto_vacuum {before_mode} -> {} in {:.1} s",
        store.usage().map(|_| "ok").unwrap_or("?"),
        started.elapsed().as_secs_f64()
    );
    for notice in store.take_notices() {
        println!("   notice: {notice}");
    }
    let mode = scalar(&copy, "PRAGMA auto_vacuum");
    println!("   PRAGMA auto_vacuum now reads {mode} (2 = incremental)");
    if mode != 2 {
        println!("   (the reader that was connected may have blocked the switch; the real service retries at its next start)");
    }
    drop(early_reader);

    println!("\n== 3. The gated purge with a short retention (the watermark must stop it)");
    let now = SystemClock.now();
    let watermark = store.rolled_up_before().unwrap();
    println!("   rolled up before: {watermark:?}");
    // A retention shorter than the time since the watermark would let the AGE reach past it: the gate must hold it back.
    let held_back = purge_cutoff(now, Duration::from_secs(60), watermark).expect("a watermark exists");
    println!(
        "   retention 1 minute: the gate gives {held_back:?}, which is the watermark: {}",
        Some(held_back) == watermark
    );
    if Some(held_back) != watermark {
        failures += 1;
    }
    // The real purge uses a 6-hour retention, so the age is what limits it and a good part of the detail stays for the guard.
    let cutoff = purge_cutoff(now, Duration::from_secs(6 * 3600), watermark).expect("a watermark exists");
    println!("   retention 6 hours: the cutoff is {cutoff:?}");
    let events_after_cutoff = scalar(&copy, &format!("SELECT COUNT(*) FROM events WHERE ts >= {}", cutoff.as_micros()));
    let rollup_rows = scalar(&copy, "SELECT (SELECT COUNT(*) FROM rollup_system_hour) + (SELECT COUNT(*) FROM rollup_process_hour) + (SELECT COUNT(*) FROM rollup_traffic_hour)");
    let unrolled_before =
        scalar(&copy, &format!("SELECT COUNT(*) FROM events WHERE ts >= {}", watermark.unwrap().as_micros()));

    let reader = SqliteStore::open_read_only(&copy).unwrap(); // the UI's connection, idle
    let size_before_purge = file_bytes(&copy);
    let started = Instant::now();
    let counts = store.purge_before(cutoff).unwrap();
    println!(
        "   purged {} events, {} processes, {} samples in {:.2} s",
        counts.events,
        counts.processes,
        counts.metric_samples,
        started.elapsed().as_secs_f64()
    );
    let usage = store.usage().unwrap();
    println!(
        "   after the purge: live {} + free pages {} (file {})",
        mb(usage.live_bytes),
        mb(usage.reclaimable_bytes),
        mb(usage.file_bytes)
    );
    let started = Instant::now();
    store.reclaim_space().unwrap();
    println!(
        "   reclaim took {:.2} s; file {} -> {} (reader connected)",
        started.elapsed().as_secs_f64(),
        mb(size_before_purge),
        mb(file_bytes(&copy))
    );
    let events_after = scalar(&copy, &format!("SELECT COUNT(*) FROM events WHERE ts >= {}", cutoff.as_micros()));
    let unrolled_after =
        scalar(&copy, &format!("SELECT COUNT(*) FROM events WHERE ts >= {}", watermark.unwrap().as_micros()));
    let rollup_rows_after = scalar(&copy, "SELECT (SELECT COUNT(*) FROM rollup_system_hour) + (SELECT COUNT(*) FROM rollup_process_hour) + (SELECT COUNT(*) FROM rollup_traffic_hour)");
    for (name, ok) in [
        ("events at or after the cutoff untouched", events_after_cutoff == events_after),
        ("events not yet rolled up untouched", unrolled_before == unrolled_after),
        ("rollup rows untouched", rollup_rows == rollup_rows_after),
        ("the file is smaller after the purge and reclaim", file_bytes(&copy) < size_before_purge),
    ] {
        println!("   {} {name}", if ok { "ok  " } else { "FAIL" });
        if !ok {
            failures += 1;
        }
    }
    let _ = reader.schema_version();

    println!(
        "\n== 4. The storage guard with a size cap far below the database (it must trim, and stop at the watermark)"
    );
    let usage = store.usage().unwrap();
    let limits =
        GuardLimits { max_db_bytes: usage.live_bytes / 4, min_free_bytes: 1_000_000, hard_floor_bytes: 100_000 };
    let volume = DriveFreeSpace::new(work.clone());
    println!(
        "   drive free {} ; cap {} ; database live {}",
        mb(volume.free_bytes().unwrap()),
        mb(limits.max_db_bytes),
        mb(usage.live_bytes)
    );
    let mut level = GuardLevel::Ok;
    let mut steps = 0;
    for check_number in 1..=40 {
        let started = Instant::now();
        let check = check_storage(&mut store, &volume, &limits, level).unwrap();
        level = check.level;
        steps += check.trimmed_steps;
        println!(
            "   check {check_number}: level {:?}, trimmed {} h, live {}, took {:.2} s",
            check.level,
            check.trimmed_steps,
            mb(check.usage.live_bytes),
            started.elapsed().as_secs_f64()
        );
        if check.trimmed_steps == 0 {
            break;
        }
    }
    let final_usage = store.usage().unwrap();
    let unrolled_final =
        scalar(&copy, &format!("SELECT COUNT(*) FROM events WHERE ts >= {}", watermark.unwrap().as_micros()));
    println!(
        "   trimmed {steps} hours in total; oldest detail now {:?}; live {}; file {}",
        final_usage.oldest_detail,
        mb(final_usage.live_bytes),
        mb(file_bytes(&copy))
    );
    for (name, ok) in [
        ("the guard trimmed something", steps > 0),
        ("detail not yet rolled up was never touched", unrolled_final == unrolled_after),
        (
            "the oldest remaining detail is not past the watermark",
            final_usage.oldest_detail.is_none_or(|t| t <= watermark.unwrap()),
        ),
    ] {
        println!("   {} {name}", if ok { "ok  " } else { "FAIL" });
        if !ok {
            failures += 1;
        }
    }

    println!("\n== 5. Integrity");
    drop(store);
    drop(reader);
    let report = inspect_database(&copy).unwrap();
    println!("   schema v{}, integrity {}", report.schema_version, report.integrity_message);
    if !report.integrity_ok {
        failures += 1;
    }
    println!("\n== Result: {}", if failures == 0 { "every check passed" } else { "FAILURES FOUND" });
    println!("   Work directory left in place: {}", work.display());
    std::process::exit(i32::from(failures != 0));
}
