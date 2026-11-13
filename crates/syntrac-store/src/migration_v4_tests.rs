//! The v3 -> v4 migration (compact layout): a real v3 database, built exactly as the old code wrote it (text ids, flows
//! duplicated into `events`), is upgraded and every value is checked through the repositories.

use std::path::Path;

use rusqlite::{params, Connection};
use syntrac_core::{
    EventEnvelope, EventQuery, EventRepository, FlowQueries, MetricsRepository, ProcessInstanceId, ProcessRecord,
    ProcessRepository, Severity, Timestamp,
};
use syntrac_testkit::fixtures::{
    at_secs, dns_query, net_flow, process_started, running_process, EventFactory, BEACON_TARGET,
};

use crate::migrations::MIGRATIONS;
use crate::{inspect_database, SqliteStore};

const V3: usize = 3;

/// What was put into the v3 database, to compare with what the upgraded one returns.
struct Seeded {
    events: Vec<EventEnvelope>,
    parent: ProcessRecord,
    child: ProcessRecord,
    owner: ProcessInstanceId,
}

fn text(id: ProcessInstanceId) -> String {
    id.as_uuid().to_string()
}

fn insert_v3_event(conn: &Connection, event: &EventEnvelope, with_flow_row: bool) {
    conn.execute(
        "INSERT INTO events (id, ts, category, kind, process_instance_id, severity, payload)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            event.id.to_string(),
            event.ts.as_micros(),
            event.payload.category().as_str(),
            event.payload.kind(),
            event.process_instance_id.map(text),
            event.severity.map(|s| serde_json::to_value(s).unwrap().as_str().unwrap().to_owned()),
            serde_json::to_string(&event.payload).unwrap(),
        ],
    )
    .unwrap();
    if let (
        true,
        syntrac_core::EventPayload::NetworkFlow { pid, remote_addr, remote_port, window_start, window_end, .. },
    ) = (with_flow_row, &event.payload)
    {
        conn.execute(
            "INSERT INTO flows (event_id, window_start, window_end, process_instance_id, pid, protocol, remote_addr,
                                remote_port, bytes_sent, bytes_received, opened, closed, failed)
             VALUES (?1, ?2, ?3, ?4, ?5, 'tcp', ?6, ?7, 100, 200, 1, 0, 0)",
            params![
                event.id.to_string(),
                window_start.as_micros(),
                window_end.as_micros(),
                event.process_instance_id.map(text),
                pid,
                remote_addr.to_string(),
                remote_port
            ],
        )
        .unwrap();
    }
}

fn insert_v3_process(conn: &Connection, record: &ProcessRecord) {
    conn.execute(
        "INSERT INTO processes (id, pid, parent_pid, parent_id, image_path, command_line, started, ended, exit_code,
                                ended_approximate)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, NULL, 0)",
        params![
            text(record.id),
            record.pid,
            record.parent_pid,
            record.parent_id.map(text),
            record.image_path,
            record.command_line,
            record.started.as_micros()
        ],
    )
    .unwrap();
}

/// A v3 database holding: a process start, a flow with its flows row, a flow event WITHOUT a flows row (its payload must
/// survive), a DNS query with a severity, two processes (one the parent of the other), and metrics with and without an owner.
fn build_v3_database(path: &Path) -> Seeded {
    let conn = Connection::open(path).unwrap();
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    for sql in &MIGRATIONS[..V3] {
        conn.execute_batch(sql).unwrap();
    }
    conn.pragma_update(None, "user_version", V3 as u32).unwrap();

    let parent = running_process(10, at_secs(5));
    let mut child = running_process(11, at_secs(6));
    child.parent_pid = 10;
    child.parent_id = Some(parent.id);
    child.command_line = Some("child.exe --flag".into());
    insert_v3_process(&conn, &parent);
    insert_v3_process(&conn, &child);

    let owner = parent.id;
    let mut f = EventFactory::default();
    let mut started = f.make(at_secs(10), process_started(10, 4, 5, r"C:\test\proc10.exe"));
    started.process_instance_id = Some(owner);
    let mut flow = f.make(at_secs(30), net_flow(10, BEACON_TARGET, 100, 200, 30));
    flow.process_instance_id = Some(owner);
    let orphan_flow = f.make(at_secs(40), net_flow(10, BEACON_TARGET, 1, 2, 40)); // no flows row, no owner
    let mut dns = f.make(at_secs(50), dns_query(10, "example.org", &[BEACON_TARGET]));
    dns.severity = Some(Severity::Suspicious);

    insert_v3_event(&conn, &started, false);
    insert_v3_event(&conn, &flow, true);
    insert_v3_event(&conn, &orphan_flow, false);
    insert_v3_event(&conn, &dns, false);

    conn.execute(
        "INSERT INTO system_metrics VALUES (?1, 30.0, 8000, 16000, 1, 2, 3, 4, 0, 0.0, 0)",
        [at_secs(20).as_micros()],
    )
    .unwrap();
    for (instance, pid, cpu) in [(Some(text(owner)), 10, 20.0), (None, 99, 8.0)] {
        conn.execute(
            "INSERT INTO process_metrics (ts, instance, pid, cpu_percent, memory_bytes) VALUES (?1, ?2, ?3, ?4, 5000)",
            params![at_secs(20).as_micros(), instance, pid, cpu],
        )
        .unwrap();
    }
    Seeded { events: vec![started, flow, orphan_flow, dns], parent, child, owner }
}

fn upgraded(dir: &Path) -> (SqliteStore, Seeded) {
    let path = dir.join("syntrac.db");
    let seeded = build_v3_database(&path);
    (SqliteStore::open(&path).unwrap(), seeded)
}

#[test]
fn the_upgrade_reaches_the_latest_version() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = upgraded(dir.path());
    assert_eq!(store.schema_version().unwrap(), SqliteStore::LATEST_SCHEMA_VERSION);
}

#[test]
fn every_event_comes_back_unchanged_including_flows_rebuilt_from_their_rows() {
    let dir = tempfile::tempdir().unwrap();
    let (store, seeded) = upgraded(dir.path());

    assert_eq!(store.query(&EventQuery::default()).unwrap(), seeded.events);
}

#[test]
fn a_flow_with_a_flows_row_is_stored_once_and_one_without_keeps_its_payload() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = upgraded(dir.path());

    let without_payload: i64 =
        store.conn.query_row("SELECT COUNT(*) FROM events WHERE payload IS NULL", [], |r| r.get(0)).unwrap();
    assert_eq!(without_payload, 1, "only the flow that has a flows row loses its duplicate payload");
    let flows: i64 = store.conn.query_row("SELECT COUNT(*) FROM flows", [], |r| r.get(0)).unwrap();
    assert_eq!(flows, 1);
    let same_number: i64 = store
        .conn
        .query_row("SELECT COUNT(*) FROM flows f JOIN events e ON e.seq = f.seq AND e.kind = 'network_flow'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(same_number, 1, "a flow shares its event's number");
}

#[test]
fn ids_are_16_byte_blobs_everywhere() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = upgraded(dir.path());
    for (table, column) in [
        ("events", "id"),
        ("events", "process_instance_id"),
        ("flows", "process_instance_id"),
        ("processes", "id"),
        ("processes", "parent_id"),
        ("process_metrics", "instance"),
    ] {
        let bad: i64 = store
            .conn
            .query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE {column} IS NOT NULL AND (typeof({column}) <> 'blob' OR length({column}) <> 16)"),
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(bad, 0, "{table}.{column}");
    }
}

#[test]
fn processes_keep_their_parent_link_and_resolve_by_pid() {
    let dir = tempfile::tempdir().unwrap();
    let (store, seeded) = upgraded(dir.path());
    assert_eq!(store.get(seeded.child.id).unwrap(), Some(seeded.child.clone()));
    assert_eq!(store.get(seeded.parent.id).unwrap(), Some(seeded.parent.clone()));
    assert_eq!(store.resolve(11, at_secs(100)).unwrap(), Some(seeded.child.id));
}

#[test]
fn the_alive_list_is_still_honoured_after_the_upgrade() {
    let dir = tempfile::tempdir().unwrap();
    let (mut store, seeded) = upgraded(dir.path());
    // Only the parent is alive: the child (not in the list) must be closed, the parent must not.
    let closed =
        store.reconcile_running(&[seeded.parent.id], Timestamp::from_micros(at_secs(100).as_micros())).unwrap();
    assert_eq!(closed, 1);
    assert_eq!(store.get(seeded.parent.id).unwrap().unwrap().ended, None);
    assert!(store.get(seeded.child.id).unwrap().unwrap().ended.is_some());
}

#[test]
fn flow_queries_and_metrics_read_the_upgraded_tables() {
    let dir = tempfile::tempdir().unwrap();
    let (store, seeded) = upgraded(dir.path());

    let destinations = store.top_destinations(at_secs(0), at_secs(100), 10).unwrap();
    assert_eq!(destinations.len(), 1);
    assert_eq!((destinations[0].bytes_sent, destinations[0].bytes_received, destinations[0].flow_count), (100, 200, 1));

    let flows = store.recent_flows(at_secs(0), at_secs(100), 10).unwrap();
    assert_eq!(flows.len(), 1);
    assert_eq!(flows[0].process, Some(seeded.owner));
    assert_eq!(flows[0].image_path.as_deref(), Some(r"C:\test\proc10.exe"));

    let breakdown = store.cpu_breakdown(at_secs(0), at_secs(100), 10).unwrap();
    assert_eq!(breakdown.window_samples, 1);
    assert!(breakdown.processes.iter().any(|p| p.instance == Some(seeded.owner) && p.pid == 10));
    assert!(breakdown.processes.iter().any(|p| p.instance.is_none() && p.pid == 99));
}

#[test]
fn indexes_nothing_uses_are_gone_and_the_needed_ones_remain() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = upgraded(dir.path());
    let names: Vec<String> = store
        .conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'index' AND name LIKE 'idx_%'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    for gone in ["idx_flows_process", "idx_flows_remote", "idx_process_metrics_instance_ts"] {
        assert!(!names.iter().any(|n| n == gone), "{gone} should have been dropped");
    }
    for kept in [
        "idx_events_ts",
        "idx_events_category_ts",
        "idx_events_process_ts",
        "idx_flows_window",
        "idx_process_metrics_ts",
        "idx_processes_pid_started",
    ] {
        assert!(names.iter().any(|n| n == kept), "{kept} should remain");
    }
}

#[test]
fn a_copy_of_the_v3_database_is_kept_next_to_it_before_the_rewrite() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    build_v3_database(&path);
    let before = inspect_database(&path).unwrap();
    assert_eq!(before.schema_version, 3);

    drop(SqliteStore::open(&path).unwrap());

    let backup = inspect_database(&dir.path().join("syntrac.db.v3-backup")).unwrap();
    assert_eq!(backup.schema_version, 3, "the backup is the database as it was");
    assert!(backup.integrity_ok);
    assert_eq!(backup.tables, before.tables, "same rows in every table");
}

#[test]
fn an_existing_backup_is_never_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    build_v3_database(&path);
    let earlier = dir.path().join("syntrac.db.v3-backup");
    std::fs::write(&earlier, b"an earlier backup the user is keeping").unwrap();

    drop(SqliteStore::open(&path).unwrap());

    assert_eq!(std::fs::read(&earlier).unwrap(), b"an earlier backup the user is keeping");
    assert_eq!(inspect_database(&dir.path().join("syntrac.db.v3-backup-2")).unwrap().schema_version, 3);
}

#[test]
fn a_new_or_current_database_makes_no_backup() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    drop(SqliteStore::open(&path).unwrap()); // brand new
    drop(SqliteStore::open(&path).unwrap()); // already current
    let backups: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().contains("backup"))
        .collect();
    assert!(backups.is_empty(), "no backup is needed when nothing is rewritten");
}

#[test]
fn the_upgrade_gives_the_space_back() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    build_v3_database(&path);
    // Bulk up the old layout so the rewrite has something to reclaim.
    {
        let conn = Connection::open(&path).unwrap();
        let mut f = EventFactory::after(1_000);
        for second in 0..3_000 {
            let mut event = f.make(at_secs(1_000 + second), net_flow(10, BEACON_TARGET, 100, 200, 1_000 + second));
            event.process_instance_id = Some(ProcessInstanceId::from_pid_and_creation(10, at_secs(5)));
            insert_v3_event(&conn, &event, true);
        }
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)").unwrap();
    }
    let before = std::fs::metadata(&path).unwrap().len();

    drop(SqliteStore::open(&path).unwrap());

    let after = std::fs::metadata(&path).unwrap().len();
    assert!(after * 10 < before * 8, "expected at least a 20% smaller file, was {before} bytes and is now {after}");
}
