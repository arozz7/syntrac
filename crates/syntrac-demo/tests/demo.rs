//! The demo database (docs/PUBLIC-PLAN.md, P1): it is repeatable, it holds nothing real, and the real rules find something of every kind the
//! screens show on it.

use std::path::Path;

use rusqlite::types::ValueRef;
use rusqlite::Connection;
use syntrac_core::{ExpectationRepository, FindingRepository, KnownProductRepository};
use syntrac_demo::generate;
use syntrac_demo::world::{DESTINATIONS, PROGRAMS};
use syntrac_store::SqliteStore;

/// 2026-11-21 10:37 UTC: not on an hour, so the "last completed hour" logic is exercised.
const NOW: i64 = 1_795_257_420;

fn make(dir: &Path, name: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    generate(&path, NOW).expect("the demo database is written");
    path
}

fn tables(connection: &Connection) -> Vec<String> {
    let mut statement =
        connection.prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'").unwrap();
    statement.query_map([], |row| row.get::<_, String>(0)).unwrap().map(Result::unwrap).collect()
}

/// Every text value of every table.
fn all_text(path: &Path) -> Vec<String> {
    let connection = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let mut found = Vec::new();
    for table in tables(&connection) {
        let mut statement = connection.prepare(&format!("SELECT * FROM \"{table}\"")).unwrap();
        let columns = statement.column_count();
        let mut rows = statement.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            for column in 0..columns {
                if let ValueRef::Text(bytes) = row.get_ref(column).unwrap() {
                    found.push(String::from_utf8_lossy(bytes).into_owned());
                }
            }
        }
    }
    found
}

fn row_counts(path: &Path) -> Vec<(String, i64)> {
    let connection = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let mut counts: Vec<(String, i64)> = tables(&connection)
        .into_iter()
        .map(|table| {
            let count =
                connection.query_row(&format!("SELECT COUNT(*) FROM \"{table}\""), [], |row| row.get(0)).unwrap();
            (table, count)
        })
        .collect();
    counts.sort();
    counts
}

#[test]
fn the_same_moment_gives_the_same_database() {
    let dir = tempfile::tempdir().unwrap();
    let (first, second) = (make(dir.path(), "a.db"), make(dir.path(), "b.db"));

    assert_eq!(row_counts(&first), row_counts(&second));
    let describe = |path: &Path| -> Vec<(String, String, String)> {
        let store = SqliteStore::open(path).unwrap();
        let mut found: Vec<_> = store
            .findings(None, 500)
            .unwrap()
            .into_iter()
            .map(|finding| (finding.kind, finding.subject, finding.title))
            .collect();
        found.sort();
        found
    };
    assert_eq!(describe(&first), describe(&second));
}

#[test]
fn it_refuses_to_overwrite_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = make(dir.path(), "demo.db");
    assert!(generate(&path, NOW).is_err());
}

#[test]
fn nothing_real_is_in_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = make(dir.path(), "demo.db");
    let texts = all_text(&path);
    assert!(texts.len() > 1_000, "the scan looked at {} values", texts.len());
    for text in &texts {
        let lower = text.to_lowercase();
        // The fictional machine has one drive and no email address in anything it stores. (Nothing here names a person or a product:
        // the other test limits every program, maker, folder and address to the invented set, so what can appear is what is listed.)
        assert!(!lower.contains('@'), "an email-like text: {text}");
        let bytes = lower.as_bytes();
        for index in 1..bytes.len().saturating_sub(1) {
            if bytes[index] == b':' && bytes[index + 1] == b'\\' && bytes[index - 1].is_ascii_lowercase() {
                assert_eq!(bytes[index - 1], b'c', "a drive other than C: in: {text}");
            }
        }
    }
}

#[test]
fn every_address_is_a_documentation_address_and_every_name_is_invented() {
    for destination in DESTINATIONS {
        let [a, b, c, _] = destination.addr;
        assert!(matches!((a, b, c), (203, 0, 113) | (198, 51, 100) | (192, 0, 2)), "{:?}", destination.addr);
        assert!(destination.name.contains(".example"), "{}", destination.name);
    }
    for program in PROGRAMS {
        let lower = program.path.to_lowercase();
        let invented = [
            r"c:\program files\fabrikam",
            r"c:\program files\contoso",
            r"c:\program files\northwind",
            r"c:\program files\acme",
            r"c:\program files\tailspin",
            r"c:\windows",
            r"c:\users\demo",
        ];
        assert!(invented.iter().any(|prefix| lower.starts_with(prefix)), "{}", program.path);
    }
}

#[test]
fn the_real_rules_find_something_of_every_kind_the_screens_show() {
    let dir = tempfile::tempdir().unwrap();
    let path = make(dir.path(), "demo.db");
    let store = SqliteStore::open(&path).unwrap();
    let findings = store.findings(None, 500).unwrap();
    let kinds: Vec<&str> = findings.iter().map(|finding| finding.kind.as_str()).collect();

    assert!(kinds.contains(&"new_program"), "{kinds:?}");
    assert!(kinds.contains(&"pattern_unsigned_program_keeps_gpu_busy"), "{kinds:?}");
    let usage: Vec<&str> = findings.iter().filter(|f| f.kind == "usage").map(|f| f.subject.as_str()).collect();
    assert!(usage.iter().any(|subject| subject.ends_with("|cpu")), "a CPU rise: {usage:?}");
    assert!(usage.iter().any(|subject| subject.ends_with("|sent")), "an upload rise: {usage:?}");
    let tiers: Vec<&str> = findings.iter().map(|finding| finding.tier.as_str()).collect();
    assert!(tiers.contains(&"unusual") && tiers.contains(&"suspicious"), "{tiers:?}");

    // The local AI tool was marked expected before the rules ran: it is on the list of expectations and raised nothing.
    assert_eq!(store.expectations().unwrap().len(), 1);
    assert!(!findings.iter().any(|finding| finding.subject.contains("localai")), "the expected program is not raised");
    assert_eq!(store.known_products().unwrap().len(), 2);
}
