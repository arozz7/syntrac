//! Rehearses the schema upgrade on a COPY of a real database and proves nothing was lost. The source is only read.
//!
//!   cargo run --release -p syntrac-store --example rehearse_migration -- <source.db> <work-dir>
//!
//! `<work-dir>` must be on a drive with room for about three times the database (the copy, its backup, and VACUUM's
//! scratch space) and must not be the live data folder. Prints: sizes before and after, how long the upgrade took, row
//! counts, every event/process compared value by value through the repositories, and the plans of the real queries.
//! Nothing here deletes anything; the work directory is left for you to inspect and remove.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use rusqlite::{Connection, OpenFlags};
use syntrac_core::{
    EventEnvelope, EventPayload, EventQuery, EventRepository, ProcessFilter, ProcessInstanceId, ProcessQueries,
    ProcessRecord, Severity, Timestamp,
};
use syntrac_store::{inspect_database, SqliteStore};
use uuid::Uuid;

fn mb(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_048_576.0)
}

fn file_size(path: &PathBuf) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn open_ro(path: &PathBuf) -> Connection {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).expect("open read-only")
}

fn scalar(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get::<_, Option<i64>>(0)).expect(sql).unwrap_or(0)
}

/// Every event as the v3 layout stored it (text ids, full payload JSON), in the order the old code returned them.
fn read_v3_events(conn: &Connection) -> Vec<EventEnvelope> {
    let mut statement = conn
        .prepare("SELECT id, ts, process_instance_id, severity, payload FROM events ORDER BY ts ASC, rowid ASC")
        .unwrap();
    statement
        .query_map([], |row| {
            let id: String = row.get(0)?;
            let owner: Option<String> = row.get(2)?;
            let severity: Option<String> = row.get(3)?;
            let payload: String = row.get(4)?;
            Ok(EventEnvelope {
                id: Uuid::parse_str(&id).unwrap(),
                ts: Timestamp::from_micros(row.get(1)?),
                process_instance_id: owner.map(|t| ProcessInstanceId::from_uuid(Uuid::parse_str(&t).unwrap())),
                severity: severity.map(|s| serde_json::from_value::<Severity>(serde_json::Value::String(s)).unwrap()),
                payload: serde_json::from_str::<EventPayload>(&payload).unwrap(),
            })
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn read_v3_processes(conn: &Connection) -> HashMap<ProcessInstanceId, ProcessRecord> {
    let id_of = |t: String| ProcessInstanceId::from_uuid(Uuid::parse_str(&t).unwrap());
    let mut statement = conn.prepare("SELECT * FROM processes").unwrap();
    statement
        .query_map([], |row| {
            let record = ProcessRecord {
                id: id_of(row.get("id")?),
                pid: row.get("pid")?,
                parent_pid: row.get("parent_pid")?,
                parent_id: row.get::<_, Option<String>>("parent_id")?.map(id_of),
                image_path: row.get("image_path")?,
                command_line: row.get("command_line")?,
                started: Timestamp::from_micros(row.get("started")?),
                ended: row.get::<_, Option<i64>>("ended")?.map(Timestamp::from_micros),
                ended_approximate: row.get::<_, i64>("ended_approximate")? != 0,
                exit_code: row.get("exit_code")?,
            };
            Ok((record.id, record))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// Aggregates that do not depend on how ids are stored: if the rewrite lost or altered a value, one of these moves.
fn fingerprint(conn: &Connection) -> Vec<(&'static str, i64)> {
    vec![
        ("flows rows", scalar(conn, "SELECT COUNT(*) FROM flows")),
        ("flows sum(bytes_sent)", scalar(conn, "SELECT SUM(bytes_sent) FROM flows")),
        ("flows sum(bytes_received)", scalar(conn, "SELECT SUM(bytes_received) FROM flows")),
        (
            "flows sum(pid+port+opened+closed+failed)",
            scalar(conn, "SELECT SUM(pid + remote_port + opened + closed + failed) FROM flows"),
        ),
        ("flows sum(window_start mod 1000003)", scalar(conn, "SELECT SUM(window_start % 1000003) FROM flows")),
        ("flows owners set", scalar(conn, "SELECT COUNT(DISTINCT process_instance_id) FROM flows")),
        ("flows distinct addresses", scalar(conn, "SELECT COUNT(DISTINCT remote_addr) FROM flows")),
        ("process_metrics rows", scalar(conn, "SELECT COUNT(*) FROM process_metrics")),
        (
            "process_metrics sum(cpu*1000)",
            scalar(conn, "SELECT CAST(SUM(cpu_percent) * 1000 AS INTEGER) FROM process_metrics"),
        ),
        ("process_metrics sum(memory)", scalar(conn, "SELECT SUM(memory_bytes) FROM process_metrics")),
        ("process_metrics without owner", scalar(conn, "SELECT COUNT(*) FROM process_metrics WHERE instance IS NULL")),
        ("process_metrics distinct owners", scalar(conn, "SELECT COUNT(DISTINCT instance) FROM process_metrics")),
        ("system_metrics rows", scalar(conn, "SELECT COUNT(*) FROM system_metrics")),
    ]
}

/// The queries the application really runs (copied from the repositories; keep in step), for `EXPLAIN QUERY PLAN`.
const REAL_QUERIES: &[(&str, &str)] = &[
    (
        "top_destinations",
        "SELECT remote_addr, remote_port, SUM(bytes_sent), SUM(bytes_received), COUNT(*),
                COUNT(DISTINCT COALESCE(process_instance_id, 'pid:' || pid))
         FROM flows WHERE window_start >= 0 AND window_start <= 9e18 GROUP BY remote_addr, remote_port
         ORDER BY SUM(bytes_sent) + SUM(bytes_received) DESC LIMIT 20",
    ),
    (
        "recent_flows",
        "SELECT f.window_start, f.process_instance_id, p.image_path FROM flows f
         LEFT JOIN processes p ON p.id = f.process_instance_id
         WHERE f.window_start >= 0 AND f.window_start <= 9e18 ORDER BY f.window_start DESC, f.rowid DESC LIMIT 200",
    ),
    ("cpu_breakdown: totals", "SELECT SUM(cpu_percent) FROM process_metrics WHERE ts >= 0 AND ts <= 9e18"),
    (
        "cpu_breakdown: per process",
        "SELECT instance, pid, SUM(cpu_percent), MAX(cpu_percent), COUNT(*) FROM process_metrics
         WHERE ts >= 0 AND ts <= 9e18 GROUP BY instance, pid ORDER BY SUM(cpu_percent) DESC LIMIT 20",
    ),
    ("events by time", "SELECT * FROM events e LEFT JOIN flows f ON f.seq = e.seq WHERE e.ts >= 0 AND e.ts <= 9e18 ORDER BY e.ts, e.seq LIMIT 100"),
    ("events by category", "SELECT * FROM events e WHERE e.category = 'dns' AND e.ts >= 0 ORDER BY e.ts, e.seq LIMIT 100"),
    ("events by process", "SELECT * FROM events e WHERE e.process_instance_id = x'00' ORDER BY e.ts, e.seq LIMIT 100"),
];

fn print_plans(conn: &Connection, only_valid_for_current_layout: bool) {
    for (name, sql) in REAL_QUERIES {
        if !only_valid_for_current_layout && name.starts_with("events") {
            continue; // the v3 layout has no `seq`
        }
        let mut statement = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        let steps: Vec<String> =
            statement.query_map([], |r| r.get::<_, String>(3)).unwrap().map(Result::unwrap).collect();
        println!("  {name}\n      {}", steps.join("\n      "));
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let source = PathBuf::from(args.next().expect("usage: rehearse_migration <source.db> <work-dir>"));
    let work = PathBuf::from(args.next().expect("usage: rehearse_migration <source.db> <work-dir>"));
    std::fs::create_dir_all(&work).expect("create the work directory");
    let copy = work.join("rehearsal.db");
    assert!(!copy.exists(), "{} already exists: use an empty work directory", copy.display());

    println!("== 1. Consistent copy of {} (the source is only read)", source.display());
    let started = Instant::now();
    open_ro(&source).execute("VACUUM INTO ?1", [copy.to_string_lossy().as_ref()]).expect("copy");
    println!("   copied in {:.1} s -> {} ({})", started.elapsed().as_secs_f64(), copy.display(), mb(file_size(&copy)));

    println!("\n== 2. Before the upgrade");
    let before = inspect_database(&copy).expect("inspect before");
    println!("   schema v{}, integrity {}", before.schema_version, before.integrity_message);
    for (table, rows) in &before.tables {
        println!("   {table:<16} {rows:>9} rows");
    }
    let conn = open_ro(&copy);
    println!(
        "   flow events without a flows row:  {}",
        scalar(
            &conn,
            "SELECT COUNT(*) FROM events WHERE kind = 'network_flow' AND id NOT IN (SELECT event_id FROM flows)"
        )
    );
    println!(
        "   flows rows without an event:      {}",
        scalar(&conn, "SELECT COUNT(*) FROM flows WHERE event_id NOT IN (SELECT id FROM events)")
    );
    println!("   (both should be 0; the first keeps its payload, the second would be dropped)");
    let fingerprint_before = fingerprint(&conn);
    let events_before = read_v3_events(&conn);
    let processes_before = read_v3_processes(&conn);
    println!("   read {} events and {} processes in the old layout", events_before.len(), processes_before.len());
    println!("   query plans on the OLD layout:");
    print_plans(&conn, false);
    drop(conn);
    let size_before = file_size(&copy);

    println!("\n== 3. Upgrade (backup, rewrite, VACUUM)");
    let started = Instant::now();
    let store = SqliteStore::open(&copy).expect("upgrade");
    let seconds = started.elapsed().as_secs_f64();
    println!("   took {seconds:.1} s; schema now v{}", store.schema_version().unwrap());
    let size_after = file_size(&copy);
    println!(
        "   size {} -> {}  ({:.0}% of the original)",
        mb(size_before),
        mb(size_after),
        100.0 * size_after as f64 / size_before as f64
    );
    let backup = work.join("rehearsal.db.v3-backup");
    println!("   backup {} exists: {} ({})", backup.display(), backup.exists(), mb(file_size(&backup)));

    println!("\n== 4. After the upgrade");
    let after = inspect_database(&copy).expect("inspect after");
    println!("   integrity {}", after.integrity_message);
    for (table, rows) in &after.tables {
        let old = before.tables.iter().find(|(t, _)| t == table).map(|(_, n)| *n);
        println!("   {table:<16} {rows:>9} rows   (was {})", old.map_or("-".to_owned(), |n| n.to_string()));
    }

    println!("\n== 5. Value-by-value comparison");
    let mut failures = 0;
    let events_after = store.query(&EventQuery::default()).expect("read events");
    let same_events = events_after == events_before;
    println!(
        "   events: {} before, {} after, identical in order: {}",
        events_before.len(),
        events_after.len(),
        same_events
    );
    if !same_events {
        failures += 1;
        for (a, b) in events_before.iter().zip(&events_after).filter(|(a, b)| a != b).take(3) {
            println!("   FIRST DIFFERENCE\n     before {a:?}\n     after  {b:?}");
        }
    }
    let processes_after: HashMap<_, _> = store
        .list_processes(&ProcessFilter { running_only: false, name_contains: None, limit: usize::MAX })
        .expect("read processes")
        .into_iter()
        .map(|p| (p.id, p))
        .collect();
    let same_processes = processes_after == processes_before;
    println!(
        "   processes: {} before, {} after, identical: {}",
        processes_before.len(),
        processes_after.len(),
        same_processes
    );
    if !same_processes {
        failures += 1;
    }
    let fingerprint_after = fingerprint(&open_ro(&copy));
    for ((name, was), (_, now)) in fingerprint_before.iter().zip(&fingerprint_after) {
        let verdict = if was == now {
            "same"
        } else {
            failures += 1;
            "DIFFERENT"
        };
        println!("   {name:<44} {was:>16} -> {now:>16}  {verdict}");
    }

    println!("\n== 6. Where the space is now");
    let conn = open_ro(&copy);
    let mut statement =
        conn.prepare("SELECT name, SUM(pgsize) FROM dbstat GROUP BY name ORDER BY SUM(pgsize) DESC").unwrap();
    for row in statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))).unwrap() {
        let (name, bytes) = row.unwrap();
        println!("   {name:<34} {:>9}", mb(bytes as u64));
    }
    println!("\n   query plans on the NEW layout:");
    print_plans(&conn, true);

    println!(
        "\n== Result: {}",
        if failures == 0 { "every comparison matched" } else { "MISMATCHES FOUND, do not migrate the real database" }
    );
    println!("   Work directory left in place for inspection: {}", work.display());
    std::process::exit(if failures == 0 { 0 } else { 1 });
}
