//! The v15 -> v16 migration (GPU use per program, docs/ALERTS-PLAN.md slice G1) is purely additive: columns with defaults on the
//! metric and rollup tables; nothing existing is touched, and no backup is needed.

use rusqlite::Connection;
use syntrac_core::{MetricsRepository, MetricsSample, OtherProcesses, ProcessMetric, SystemMetrics, Timestamp};

use crate::migrations::MIGRATIONS;
use crate::SqliteStore;

const V15: usize = 15;

fn old_database(path: &std::path::Path) {
    let conn = Connection::open(path).unwrap();
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    for sql in &MIGRATIONS[..V15] {
        conn.execute_batch(sql).unwrap();
    }
    conn.pragma_update(None, "user_version", V15 as u32).unwrap();
    conn.execute(
        "INSERT INTO system_metrics (ts, cpu_percent, memory_used, memory_total, disk_read, disk_write, net_received, net_sent,
                                     others_count, others_cpu, others_memory)
         VALUES (1000, 12.5, 100, 200, 1, 2, 3, 4, 5, 6.5, 7)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO process_metrics (ts, instance, pid, cpu_percent, memory_bytes) VALUES (1000, NULL, 42, 3.5, 99)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO rollup_system_hour (hour_start, samples, cpu_avg, cpu_max, memory_used_avg, memory_used_max, memory_total,
                                         disk_read, disk_write, net_received, net_sent, others_cpu_avg)
         VALUES (3600000000, 10, 1.0, 2.0, 3, 4, 5, 6, 7, 8, 9, 0.5)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO executables (id, path_key, path, first_seen, last_seen) VALUES (2, 'c:\\a.exe', 'C:\\a.exe', 1, 2)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO rollup_process_hour (hour_start, exe_id, samples_present, cpu_avg, cpu_peak, memory_peak, starts, first_seen, last_seen)
         VALUES (3600000000, 2, 4, 1.5, 2.5, 77, 1, 1, 2)",
        [],
    )
    .unwrap();
}

#[test]
fn upgrading_keeps_every_row_gives_old_rows_no_gpu_data_and_makes_no_backup() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    old_database(&path);

    let mut store = SqliteStore::open(&path).unwrap();
    assert_eq!(store.schema_version().unwrap(), SqliteStore::LATEST_SCHEMA_VERSION);

    let conn = Connection::open(&path).unwrap();
    let system: (f64, Option<f64>, f64, i64) = conn
        .query_row(
            "SELECT cpu_percent, gpu_percent, others_gpu_percent, others_gpu_memory FROM system_metrics WHERE ts = 1000",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(system, (12.5, None, 0.0, 0), "an old system row has NO GPU reading (not an idle GPU)");
    let process: (f64, f64, i64) = conn
        .query_row("SELECT cpu_percent, gpu_percent, gpu_memory FROM process_metrics WHERE pid = 42", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .unwrap();
    assert_eq!(process, (3.5, 0.0, 0));
    let hour: (f64, Option<f64>, Option<f64>) = conn
        .query_row("SELECT cpu_avg, gpu_avg, gpu_max FROM rollup_system_hour", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .unwrap();
    assert_eq!(hour, (1.0, None, None));
    let program: (f64, f64, f64, i64) = conn
        .query_row("SELECT cpu_avg, gpu_avg, gpu_peak, gpu_memory_peak FROM rollup_process_hour", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap();
    assert_eq!(program, (1.5, 0.0, 0.0, 0));

    // New samples with GPU figures are stored next to the old rows.
    store
        .append_metrics(&MetricsSample {
            ts: Timestamp::from_micros(2000),
            system: SystemMetrics { gpu_percent: Some(87.5), ..SystemMetrics::default() },
            processes: vec![ProcessMetric {
                pid: 43,
                instance: None,
                cpu_percent: 1.0,
                memory_bytes: 1,
                gpu_percent: 66.0,
                gpu_memory_bytes: 123_456,
            }],
            others: OtherProcesses { gpu_percent: 4.5, gpu_memory_bytes: 9, ..OtherProcesses::default() },
        })
        .unwrap();
    let stored: (Option<f64>, f64, i64, f64, i64) = conn
        .query_row(
            "SELECT s.gpu_percent, p.gpu_percent, p.gpu_memory, s.others_gpu_percent, s.others_gpu_memory
             FROM system_metrics s JOIN process_metrics p ON p.ts = s.ts WHERE s.ts = 2000",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )
        .unwrap();
    assert_eq!(stored, (Some(87.5), 66.0, 123_456, 4.5, 9));

    let backups = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().contains("backup"))
        .count();
    assert_eq!(backups, 0, "an additive migration needs no backup copy");
}
