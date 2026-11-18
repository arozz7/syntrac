//! Backtest of the folder-scope change (docs/ALERTS-PLAN.md, F6) on a COPY of a real database. The source is only read.
//!
//!   cargo run --release -p syntrac-store --example rehearse_folders -- <source.db> <work-dir>
//!
//! For every program SYNTRAC knows it asks whether the user's expectations cover it under the OLD matching (a folder expectation
//! covers anything below the folder) and under the NEW one (a protected folder covers only what is protected inside it), and prints
//! how many programs stop being covered, so a flood is known before the build ships. It also counts how many programs sit in
//! writable folders that already have a whole-folder expectation (kept and flagged, not switched off). The output names folders on
//! this machine: keep it local. Nothing is deleted; the work directory is left for you to remove.

use std::collections::BTreeMap;
use std::path::PathBuf;

use rusqlite::{Connection, OpenFlags};
use syntrac_core::findings::{expectation_covers, Expectation, FOLDER_SCOPE_PREFIX};
use syntrac_core::{is_protected_folder_on_system_drive, system_drive, ExpectationRepository, IdentityRepository};
use syntrac_store::SqliteStore;

/// The matching before F6: an item by equality, a folder by prefix, whatever the folder is.
fn covered_before(expectations: &[Expectation], subject: &str) -> bool {
    expectations.iter().any(|expectation| match expectation.subject.strip_prefix(FOLDER_SCOPE_PREFIX) {
        Some(root) => subject.strip_prefix(root).is_some_and(|rest| rest.starts_with('\\')),
        None => expectation.subject == subject,
    })
}

fn main() {
    let mut args = std::env::args().skip(1);
    let source = PathBuf::from(args.next().expect("source database"));
    let work = PathBuf::from(args.next().expect("work directory"));
    std::fs::create_dir_all(&work).expect("create work dir");
    let copy = work.join("folders-rehearsal.db");
    assert!(!copy.exists(), "{} already exists; remove it first", copy.display());
    {
        let src =
            Connection::open_with_flags(&source, OpenFlags::SQLITE_OPEN_READ_ONLY).expect("open source read-only");
        src.execute("VACUUM INTO ?1", [copy.to_string_lossy().as_ref()]).expect("copy");
    }
    let store = SqliteStore::open(&copy).expect("open and upgrade the copy");
    let expectations = store.expectations().expect("expectations");
    let entries = store.identity_entries().expect("identity entries");
    let drive = system_drive();

    let folders: Vec<&Expectation> =
        expectations.iter().filter(|e| e.subject.starts_with(FOLDER_SCOPE_PREFIX)).collect();
    let protected: Vec<&&Expectation> = folders
        .iter()
        .filter(|e| {
            e.subject
                .strip_prefix(FOLDER_SCOPE_PREFIX)
                .is_some_and(|root| is_protected_folder_on_system_drive(root, &drive))
        })
        .collect();
    println!(
        "{} programs known; {} expectations, {} of them whole-folder ({} protected on {drive}, {} writable and kept)",
        entries.len(),
        expectations.len(),
        folders.len(),
        protected.len(),
        folders.len() - protected.len()
    );

    let mut covered_old = 0usize;
    let mut covered_new = 0usize;
    let mut stop: BTreeMap<String, usize> = BTreeMap::new();
    let mut under_writable: BTreeMap<String, usize> = BTreeMap::new();
    for entry in &entries {
        let key = entry.exe.key.as_str();
        let before = covered_before(&expectations, key);
        let after = expectation_covers(&expectations, "new_program", key);
        covered_old += usize::from(before);
        covered_new += usize::from(after);
        if before && !after {
            let root = folders
                .iter()
                .filter_map(|e| e.subject.strip_prefix(FOLDER_SCOPE_PREFIX))
                .find(|root| key.strip_prefix(root).is_some_and(|rest| rest.starts_with('\\')))
                .unwrap_or("(item)");
            *stop.entry(root.to_owned()).or_default() += 1;
        }
        if after {
            if let Some(root) = folders
                .iter()
                .filter_map(|e| e.subject.strip_prefix(FOLDER_SCOPE_PREFIX))
                .find(|root| key.strip_prefix(root).is_some_and(|rest| rest.starts_with('\\')))
            {
                if !is_protected_folder_on_system_drive(root, &drive) {
                    *under_writable.entry(root.to_owned()).or_default() += 1;
                }
            }
        }
    }
    println!("covered by an expectation: {covered_old} before, {covered_new} after");
    println!(
        "programs that stop being covered (they would be raised again if they were new): {}",
        covered_old - covered_new
    );
    for (root, count) in &stop {
        println!("  {count:5}  under {root}");
    }
    println!(
        "programs still covered inside a writable folder (honoured, flagged on the Findings tab): {}",
        under_writable.values().sum::<usize>()
    );
    for (root, count) in &under_writable {
        println!("  {count:5}  under {root}");
    }
    println!("\nthe copy is at {}; remove it when done", copy.display());
}
