//! The v14 -> v15 migration (the level a usage rule measured) is purely additive: a nullable column on findings; nothing existing is
//! touched, and no backup is needed.

use rusqlite::Connection;
use syntrac_core::findings::{Candidate, FindingWrite, Tier};
use syntrac_core::{FindingRepository, Timestamp};

use crate::migrations::MIGRATIONS;
use crate::SqliteStore;

const V14: usize = 14;

#[test]
fn upgrading_keeps_the_findings_gives_them_no_level_and_makes_no_backup() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    {
        let conn = Connection::open(&path).unwrap();
        conn.pragma_update(None, "journal_mode", "WAL").unwrap();
        for sql in &MIGRATIONS[..V14] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", V14 as u32).unwrap();
        conn.execute(
            "INSERT INTO findings (kind, subject, subject_label, tier, title, why, evidence, dedupe_key, status, first_at, last_at, hits)
             VALUES ('new_program', 'a', 'a', 'unusual', 't', 'w', '', 'new_program:a', 'open', 1, 2, 3)",
            [],
        )
        .unwrap();
    }

    let mut store = SqliteStore::open(&path).unwrap();

    assert_eq!(store.schema_version().unwrap(), SqliteStore::LATEST_SCHEMA_VERSION);
    let old = store.findings(None, 10).unwrap();
    assert_eq!((old.len(), old[0].hits, old[0].level), (1, 3, None));
    store
        .apply_writes(&[FindingWrite::Insert(Candidate {
            kind: "usage".into(),
            subject: "p|cpu".into(),
            subject_label: "p".into(),
            tier: Tier::Unusual,
            title: "t".into(),
            why: "w".into(),
            evidence: vec![],
            dedupe_key: "usage:p|cpu:1".into(),
            level: Some(61.5),
            at: Timestamp::from_micros(9),
        })])
        .unwrap();
    assert_eq!(store.findings_for_keys(&["usage:p|cpu:1".into()]).unwrap()[0].level, Some(61.5));
    let backups = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().contains("backup"))
        .count();
    assert_eq!(backups, 0, "an additive migration needs no backup copy");
}
