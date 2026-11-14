//! Rehearsal of the identity worker on a COPY of a database: `cargo run --release -p syntrac-service --example identity_probe -- <copy.db>`
//!
//! Opens the copy (which migrates it to the latest schema), runs the same pass the service runs until nothing is left to look
//! at, and prints what it found: counts by signature status and kind, failures by kind, the time per file, the CPU the pass
//! used and how much the database grew. It never touches the live database: give it a copy.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use syntrac_collector::file_inspector::WindowsFileInspector;
use syntrac_collector::system::{own_cpu_time, SystemClock};
use syntrac_core::{
    classify_location, inspect_due, Clock, IdentityPass, IdentityRepository, IdentitySettings, SignatureStatus,
};
use syntrac_store::SqliteStore;

fn main() {
    let path: PathBuf = std::env::args().nth(1).expect("usage: identity_probe <copy of the database>").into();
    let size_before = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let mut store = SqliteStore::open(&path).expect("could not open the copy");
    println!("opened {} (schema v{})", path.display(), store.schema_version().unwrap());
    let entries = store.identity_entries().unwrap();
    println!(
        "{} executables known, {} with an identity already",
        entries.len(),
        entries.iter().filter(|e| e.record.is_some()).count()
    );

    let inspector = WindowsFileInspector::default();
    let settings = IdentitySettings { max_per_pass: 25, ..IdentitySettings::default() };
    let (cpu_before, started) = (own_cpu_time(), Instant::now());
    let mut totals = IdentityPass::default();
    let mut slowest: Vec<(Duration, String)> = Vec::new();
    loop {
        let pass_started = Instant::now();
        let pass = inspect_due(&mut store, &inspector, SystemClock.now(), &settings).unwrap();
        let took = pass_started.elapsed();
        totals.inspected += pass.inspected;
        totals.failed += pass.failed;
        totals.unchanged += pass.unchanged;
        if pass.inspected + pass.failed + pass.unchanged > 0 {
            slowest.push((took, format!("pass of {} files", pass.inspected + pass.failed + pass.unchanged)));
        }
        if !pass.backlog {
            break;
        }
    }
    let wall = started.elapsed();
    let cpu = own_cpu_time() - cpu_before;

    println!("\n== Pass summary");
    println!("read in full {}, failed {}, unchanged {}", totals.inspected, totals.failed, totals.unchanged);
    println!(
        "wall time {:.1} s, CPU {:.1} s, {:.0} ms per file on average",
        wall.as_secs_f64(),
        cpu.as_secs_f64(),
        wall.as_secs_f64() * 1000.0 / (totals.inspected + totals.failed).max(1) as f64
    );
    slowest.sort_by(|a, b| b.0.cmp(&a.0));
    if let Some((took, what)) = slowest.first() {
        println!("slowest pass: {:.1} s ({what})", took.as_secs_f64());
    }

    let entries = store.identity_entries().unwrap();
    let mut by_status: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_failure: BTreeMap<String, usize> = BTreeMap::new();
    let mut flagged: Vec<String> = Vec::new();
    let mut without_hash = 0;
    for entry in &entries {
        let Some(record) = &entry.record else { continue };
        if let Some(failure) = &record.failure {
            *by_failure.entry(format!("{:?}", failure.kind)).or_default() += 1;
        }
        let Some(facts) = &record.facts else { continue };
        let kind = facts.signature.kind.map_or("-", |k| k.as_str());
        *by_status.entry(format!("{} ({kind})", facts.signature.status.as_str())).or_default() += 1;
        if facts.sha256.is_none() {
            without_hash += 1;
        }
        let location = classify_location(&entry.exe.path);
        if facts.signature.status != SignatureStatus::Valid {
            flagged.push(format!(
                "{:<10} {:<14} {}",
                facts.signature.status.as_str(),
                location.as_str(),
                entry.exe.path
            ));
        }
    }
    println!("\n== Signatures");
    for (status, count) in &by_status {
        println!("{count:>5}  {status}");
    }
    println!("{without_hash:>5}  not hashed (too large)");
    println!("\n== Failures (the files could not be read)");
    if by_failure.is_empty() {
        println!("none");
    }
    for (kind, count) in &by_failure {
        println!("{count:>5}  {kind}");
    }

    println!("\n== Not validly signed, by location (first 40)");
    flagged.sort();
    for line in flagged.iter().take(40) {
        println!("{line}");
    }
    println!("({} in all)", flagged.len());

    println!("\n== Spot checks");
    for needle in
        ["cmd.exe", "acmeserv.exe", "acmetransmit64.exe", "syntrac-service", "syntrac-app", "chrome.exe", "msmpeng.exe"]
    {
        if let Some(entry) = entries.iter().find(|e| e.exe.key.ends_with(needle) || e.exe.key.contains(needle)) {
            match entry.record.as_ref().and_then(|r| r.facts.as_ref()) {
                Some(f) => println!(
                    "{}\n    {} | signer {:?} | {} | company {:?} | product {:?} | original {:?} | hash {}",
                    entry.exe.path,
                    f.signature.status.as_str(),
                    f.signature.signer,
                    f.signature.kind.map_or("-", |k| k.as_str()),
                    f.version.company,
                    f.version.product,
                    f.version.original_filename,
                    f.sha256.as_deref().map_or("-", |h| &h[..12])
                ),
                None => println!(
                    "{}\n    no facts: {:?}",
                    entry.exe.path,
                    entry.record.as_ref().and_then(|r| r.failure.as_ref()).map(|f| (f.kind, &f.message))
                ),
            }
        } else {
            println!("{needle}: not among the executables seen");
        }
    }

    drop(store);
    let size_after = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    println!(
        "\n== Database\nfile size {size_before} -> {size_after} bytes (+{})",
        size_after.saturating_sub(size_before)
    );
}
