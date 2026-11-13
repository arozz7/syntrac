//! Rehearsal helper: copies a database (`VACUUM INTO`, read-only on the source) and then repeats its rollup rows once per day for
//! the given number of earlier days, so the trend queries can be timed on about a year of rollups. NEVER touches the source.
//! Usage: cargo run -p syntrac-store --example inflate_rollups -- <source db> <target db (must not exist)> <days>

use rusqlite::{Connection, OpenFlags};

const DAY_MICROS: i64 = 86_400_000_000;

fn main() {
    let mut args = std::env::args().skip(1);
    let (source, target, days) = (
        args.next().expect("source db"),
        args.next().expect("target db"),
        args.next().and_then(|d| d.parse::<i64>().ok()).expect("days"),
    );
    assert!(!std::path::Path::new(&target).exists(), "the target already exists; choose a new name");

    let src = Connection::open_with_flags(&source, OpenFlags::SQLITE_OPEN_READ_ONLY).expect("open source read-only");
    src.execute("VACUUM INTO ?1", [&target]).expect("copy");
    drop(src);

    let dst = Connection::open(&target).expect("open the copy");
    let span: i64 = dst
        .query_row("SELECT MAX(hour_start) - MIN(hour_start) FROM rollup_system_hour", [], |row| row.get(0))
        .expect("span");
    assert!(span < DAY_MICROS, "the existing rollups must span less than a day, or whole-day shifts would overlap");

    // Shift copies of the ORIGINAL rows only (a snapshot), never of rows added meanwhile.
    dst.execute_batch(
        "CREATE TEMP TABLE orig_system AS SELECT * FROM rollup_system_hour;
         CREATE TEMP TABLE orig_process AS SELECT * FROM rollup_process_hour;
         CREATE TEMP TABLE orig_traffic AS SELECT * FROM rollup_traffic_hour;",
    )
    .expect("snapshot");
    for day in 1..=days {
        let shift = day * DAY_MICROS;
        dst.execute(
            "INSERT INTO rollup_system_hour SELECT hour_start - ?1, samples, cpu_avg, cpu_max, memory_used_avg,
                 memory_used_max, memory_total, disk_read, disk_write, net_received, net_sent, others_cpu_avg
             FROM orig_system",
            [shift],
        )
        .expect("system");
        dst.execute(
            "INSERT INTO rollup_process_hour SELECT hour_start - ?1, exe_id, samples_present, cpu_avg, cpu_peak,
                 memory_peak, starts, first_seen - ?1, last_seen - ?1 FROM orig_process",
            [shift],
        )
        .expect("process");
        dst.execute(
            "INSERT INTO rollup_traffic_hour SELECT hour_start - ?1, exe_id, addr, bytes_sent, bytes_received,
                 flows, first_seen - ?1, last_seen - ?1, distinct_ports, top_port FROM orig_traffic",
            [shift],
        )
        .expect("traffic");
    }
    let rows = |table: &str| -> i64 {
        dst.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row.get(0)).expect("count")
    };
    println!(
        "copy written to {target}: {} system hours, {} process rows, {} traffic rows",
        rows("rollup_system_hour"),
        rows("rollup_process_hour"),
        rows("rollup_traffic_hour")
    );
}
