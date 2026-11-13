//! Rehearses the rollups on a COPY of a real database: upgrades it to the latest schema, rolls up every completed hour, and
//! checks the rollup totals against the raw detail. The source is only read.
//!
//!   cargo run --release -p syntrac-store --example rehearse_rollup -- <source.db> <work-dir>

use std::path::PathBuf;
use std::time::{Duration, Instant};

use rusqlite::{Connection, OpenFlags};
use syntrac_core::Clock;
use syntrac_core::{roll_up_due, RollupRepository, RollupSettings, SystemClock};
use syntrac_store::SqliteStore;

fn mb(bytes: u64) -> String {
    format!("{:.2} MB", bytes as f64 / 1_048_576.0)
}

fn scalar(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get::<_, Option<i64>>(0)).expect(sql).unwrap_or(0)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let source = PathBuf::from(args.next().expect("usage: rehearse_rollup <source.db> <work-dir>"));
    let work = PathBuf::from(args.next().expect("usage: rehearse_rollup <source.db> <work-dir>"));
    std::fs::create_dir_all(&work).unwrap();
    let copy = work.join("rollup-rehearsal.db");
    assert!(!copy.exists(), "{} already exists: use an empty work directory", copy.display());

    println!("== 1. Copy of {} (the source is only read)", source.display());
    Connection::open_with_flags(&source, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .unwrap()
        .execute("VACUUM INTO ?1", [copy.to_string_lossy().as_ref()])
        .unwrap();
    let size_before = std::fs::metadata(&copy).unwrap().len();
    println!("   {} ({})", copy.display(), mb(size_before));

    println!("\n== 2. Open (applies pending migrations) and roll up the backlog");
    let started = Instant::now();
    let mut store = SqliteStore::open(&copy).expect("open");
    println!("   schema v{} after {:.1} s", store.schema_version().unwrap(), started.elapsed().as_secs_f64());
    let earliest = store.earliest_detail().unwrap();
    println!("   earliest detail: {earliest:?}");

    let now = SystemClock.now();
    let settings = RollupSettings::default();
    let mut passes = 0;
    let mut hours = 0;
    let mut slowest = Duration::ZERO;
    let started = Instant::now();
    loop {
        let pass_started = Instant::now();
        let pass = roll_up_due(&mut store, now, &settings).expect("roll up");
        slowest = slowest.max(pass_started.elapsed());
        passes += 1;
        hours += pass.hours_rolled;
        if !pass.backlog {
            break;
        }
    }
    println!(
        "   {hours} hours in {passes} passes, {:.2} s in total, slowest pass {:.0} ms (a pass is up to {} hours)",
        started.elapsed().as_secs_f64(),
        slowest.as_secs_f64() * 1000.0,
        settings.max_hours_per_pass
    );
    println!("   rolled up before: {:?}", store.rolled_up_before().unwrap());
    drop(store);

    println!("\n== 3. What was stored");
    let conn = Connection::open_with_flags(&copy, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    for table in [
        "rollup_system_hour",
        "rollup_process_hour",
        "rollup_traffic_hour",
        "executables",
        "destinations",
        "exe_destinations",
    ] {
        println!("   {table:<24} {:>8} rows", scalar(&conn, &format!("SELECT COUNT(*) FROM {table}")));
    }
    let mut statement = conn.prepare("SELECT name, SUM(pgsize) FROM dbstat WHERE name LIKE 'rollup%' OR name IN ('executables','destinations','exe_destinations') OR name LIKE 'idx_rollup%' OR name LIKE 'sqlite_autoindex_exe%' GROUP BY name ORDER BY 2 DESC").unwrap();
    let mut total = 0_i64;
    for row in statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))).unwrap() {
        let (name, bytes) = row.unwrap();
        total += bytes;
        println!("   {name:<40} {:>10}", mb(bytes as u64));
    }
    println!("   rollup storage total: {} for {hours} hours", mb(total as u64));

    println!("\n== 4. Cross-check against the raw detail (only hours that were rolled up)");
    let done = scalar(&conn, "SELECT hours_done_before FROM rollup_progress WHERE id = 1");
    let mut failures = 0;
    let mut check = |name: &str, raw: i64, rolled: i64| {
        let verdict = if raw == rolled {
            "same"
        } else {
            failures += 1;
            "DIFFERENT"
        };
        println!("   {name:<46} raw {raw:>16}   rollup {rolled:>16}   {verdict}");
    };
    check(
        "system samples",
        scalar(&conn, &format!("SELECT COUNT(*) FROM system_metrics WHERE ts < {done}")),
        scalar(&conn, "SELECT SUM(samples) FROM rollup_system_hour"),
    );
    check(
        "system net bytes received",
        scalar(&conn, &format!("SELECT SUM(net_received) FROM system_metrics WHERE ts < {done}")),
        scalar(&conn, "SELECT SUM(net_received) FROM rollup_system_hour"),
    );
    check(
        "flow bytes sent",
        scalar(&conn, &format!("SELECT SUM(bytes_sent) FROM flows WHERE window_start < {done}")),
        scalar(&conn, "SELECT SUM(bytes_sent) FROM rollup_traffic_hour"),
    );
    check(
        "flow bytes received",
        scalar(&conn, &format!("SELECT SUM(bytes_received) FROM flows WHERE window_start < {done}")),
        scalar(&conn, "SELECT SUM(bytes_received) FROM rollup_traffic_hour"),
    );
    check(
        "flow rows",
        scalar(&conn, &format!("SELECT COUNT(*) FROM flows WHERE window_start < {done}")),
        scalar(&conn, "SELECT SUM(flows) FROM rollup_traffic_hour"),
    );
    check(
        "process starts (process_started events)",
        scalar(&conn, &format!("SELECT COUNT(*) FROM events WHERE kind = 'process_started' AND ts < {done}")),
        scalar(&conn, "SELECT SUM(starts) FROM rollup_process_hour"),
    );
    check(
        "distinct destination addresses",
        scalar(&conn, &format!("SELECT COUNT(DISTINCT remote_addr) FROM flows WHERE window_start < {done}")),
        scalar(&conn, "SELECT COUNT(*) FROM destinations"),
    );
    check(
        "distinct (executable, address) pairs, attributed",
        scalar(&conn, &format!("SELECT COUNT(*) FROM (SELECT DISTINCT lower(p.image_path), f.remote_addr FROM flows f JOIN processes p ON p.id = f.process_instance_id WHERE f.window_start < {done} AND trim(p.image_path) <> '')")),
        scalar(&conn, "SELECT COUNT(*) FROM exe_destinations"),
    );

    println!("\n== 5. The busiest executables by CPU (a sanity check you can read)");
    let mut statement = conn
        .prepare(
            "SELECT e.path, ROUND(SUM(r.cpu_avg) / (SELECT COUNT(*) FROM rollup_system_hour), 2), SUM(r.starts)
             FROM rollup_process_hour r JOIN executables e ON e.id = r.exe_id
             GROUP BY e.id ORDER BY SUM(r.cpu_avg) DESC LIMIT 6",
        )
        .unwrap();
    for row in
        statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?, r.get::<_, i64>(2)?))).unwrap()
    {
        let (path, cpu, starts) = row.unwrap();
        println!("   avg {cpu:>6.2}% of the machine, {starts:>5} starts   {path}");
    }

    println!("\n== Result: {}", if failures == 0 { "every cross-check matched" } else { "MISMATCHES FOUND" });
    println!("   Work directory left in place: {}", work.display());
    std::process::exit(i32::from(failures != 0));
}
