//! Rehearses the owner data on a COPY of a real database. The source is only read. THIS DOWNLOADS the real dataset once
//! (the first network use), so run it only when you mean to.
//!
//!   cargo run --release -p syntrac-store --example rehearse_owner -- <source.db> <work-dir>
//!
//! The copy is made with `VACUUM INTO`, upgraded to the latest schema, then the real file is fetched from the compiled-in address,
//! parsed, stored (database growth is reported), and every destination the copy has seen is looked up. Prints each step's time and
//! size. The work directory is left for you to inspect and remove.

use std::path::PathBuf;
use std::time::Instant;

use rusqlite::{Connection, OpenFlags};
use syntrac_collector::owner_fetch::WinHttpFetcher;
use syntrac_core::{parse_ip2asn, Clock, DatasetFetcher, DatasetInfo, OwnerRepository, SystemClock};
use syntrac_store::SqliteStore;

fn file_bytes(path: &std::path::Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let source = PathBuf::from(args.next().expect("source database"));
    let work = PathBuf::from(args.next().expect("work directory"));
    std::fs::create_dir_all(&work).expect("create work dir");
    let copy = work.join("owner-rehearsal.db");
    assert!(!copy.exists(), "{} already exists; remove it first", copy.display());

    let started = Instant::now();
    {
        let src =
            Connection::open_with_flags(&source, OpenFlags::SQLITE_OPEN_READ_ONLY).expect("open source read-only");
        src.execute("VACUUM INTO ?1", [copy.to_string_lossy().as_ref()]).expect("copy");
    }
    println!("copied in {:.1}s ({} MB)", started.elapsed().as_secs_f64(), file_bytes(&copy) / 1_000_000);
    let mut store = SqliteStore::open(&copy).expect("open and upgrade the copy");
    println!("schema v{}", store.schema_version().unwrap());
    let before = file_bytes(&copy);

    let started = Instant::now();
    let file = match WinHttpFetcher::for_owner_data().fetch() {
        Ok(file) => file,
        Err(error) => {
            println!("DOWNLOAD FAILED: {error}");
            return;
        }
    };
    println!(
        "downloaded and unpacked in {:.1}s: {} MB of text from {}, sha256 {}",
        started.elapsed().as_secs_f64(),
        file.text.len() / 1_000_000,
        file.source,
        &file.sha256[..16]
    );

    let started = Instant::now();
    let parsed = parse_ip2asn(&file.text);
    println!(
        "parsed in {:.1}s: {} ranges kept, {} unrouted, {} bad lines",
        started.elapsed().as_secs_f64(),
        parsed.ranges.len(),
        parsed.unrouted,
        parsed.bad_lines
    );

    let info = DatasetInfo {
        source: file.source,
        downloaded_at: SystemClock.now(),
        rows: u32::try_from(parsed.ranges.len()).unwrap_or(u32::MAX),
        bad_lines: parsed.bad_lines,
        sha256: file.sha256,
    };
    let started = Instant::now();
    store.replace_dataset(&parsed.ranges, &info).expect("store the dataset");
    println!("stored in {:.1}s", started.elapsed().as_secs_f64());
    drop(store);
    println!("database: {} MB before, {} MB after", before / 1_000_000, file_bytes(&copy) / 1_000_000);

    let store = SqliteStore::open(&copy).expect("reopen");
    let conn = Connection::open_with_flags(&copy, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let addrs: Vec<String> = conn
        .prepare("SELECT addr FROM destinations")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    let started = Instant::now();
    let mut found = 0usize;
    for addr in &addrs {
        if let Ok(ip) = addr.parse() {
            if store.lookup_owner(ip).expect("lookup").is_some() {
                found += 1;
            }
        }
    }
    println!(
        "looked up {} destinations in {:.0} ms: {} have an owner",
        addrs.len(),
        started.elapsed().as_secs_f64() * 1000.0,
        found
    );
    let mut owners: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for addr in addrs.iter().filter_map(|a| a.parse().ok()) {
        if let Some(owner) = store.lookup_owner(addr).unwrap() {
            *owners.entry(format!("AS{} {} ({})", owner.asn, owner.org, owner.country)).or_default() += 1;
        }
    }
    let mut top: Vec<_> = owners.into_iter().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1));
    for (owner, count) in top.into_iter().take(8) {
        println!("  {count:5}  {owner}");
    }
    println!("copy left at {}", copy.display());
}
