//! Rehearses the v8 upgrade and the names backfill on a COPY of a real database. The source is only read.
//!
//!   cargo run --release -p syntrac-store --example rehearse_names -- <source.db> <work-dir>
//!
//! `<work-dir>` must be on a drive with room for the database and must not be the live data folder. The copy is made with
//! `VACUUM INTO`, which reads a consistent snapshot while the service keeps writing. Prints how long the upgrade and the
//! backfill took, then compares the stored pairs with an independent count made straight from the DNS events in SQL.
//! Nothing here deletes anything; the work directory is left for you to inspect and remove.

use std::path::PathBuf;
use std::time::Instant;

use rusqlite::{Connection, OpenFlags};
use syntrac_core::Clock;
use syntrac_core::{backfill_names, SystemClock};
use syntrac_store::SqliteStore;

fn scalar(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get::<_, Option<i64>>(0)).expect(sql).unwrap_or(0)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let source = PathBuf::from(args.next().expect("source database"));
    let work = PathBuf::from(args.next().expect("work directory"));
    std::fs::create_dir_all(&work).expect("create work dir");
    let copy = work.join("names-rehearsal.db");
    assert!(!copy.exists(), "{} already exists; remove it first", copy.display());

    let started = Instant::now();
    {
        let src =
            Connection::open_with_flags(&source, OpenFlags::SQLITE_OPEN_READ_ONLY).expect("open source read-only");
        src.execute("VACUUM INTO ?1", [copy.to_string_lossy().as_ref()]).expect("copy");
    }
    println!("copied in {:.1}s", started.elapsed().as_secs_f64());

    let started = Instant::now();
    let mut store = SqliteStore::open(&copy).expect("open and upgrade the copy");
    println!("upgrade to v{} in {:.2}s", store.schema_version().unwrap(), started.elapsed().as_secs_f64());

    let now = SystemClock.now();
    let started = Instant::now();
    let done = backfill_names(&mut store, now).expect("backfill");
    println!("backfill: ran={} pairs={} in {:.2}s", done.ran, done.names, started.elapsed().as_secs_f64());
    let again = backfill_names(&mut store, now).expect("second backfill");
    println!("second backfill ran={} (must be false)", again.ran);
    drop(store);

    let conn = Connection::open_with_flags(&copy, OpenFlags::SQLITE_OPEN_READ_ONLY).expect("open copy");
    let stored = scalar(&conn, "SELECT COUNT(*) FROM address_names");
    let addrs = scalar(&conn, "SELECT COUNT(DISTINCT addr) FROM address_names");
    let widest = scalar(&conn, "SELECT MAX(n) FROM (SELECT COUNT(*) AS n FROM address_names GROUP BY addr)");
    // An independent count, straight from the events (no cap, names lower-cased and trimmed of a trailing dot).
    let expected = scalar(
        &conn,
        "SELECT COUNT(*) FROM (SELECT DISTINCT a.value, rtrim(lower(trim(json_extract(e.payload, '$.query'))), '.')
                               FROM events e, json_each(e.payload, '$.addresses') a
                               WHERE e.category = 'dns' AND rtrim(lower(trim(json_extract(e.payload, '$.query'))), '.') <> '')",
    );
    println!("stored pairs={stored} addresses={addrs} most names on one address={widest}");
    println!("independent count of distinct (address, name) in the events={expected} (stored may be lower only by the cap and by mapped addresses)");

    let mut statement =
        conn.prepare("SELECT addr, name, last_seen FROM address_names ORDER BY last_seen DESC LIMIT 8").unwrap();
    let rows = statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).unwrap();
    for row in rows {
        let (addr, name) = row.unwrap();
        println!("  {addr}  {name}");
    }
    println!("copy left at {}", copy.display());
}
