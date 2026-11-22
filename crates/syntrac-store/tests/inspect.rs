//! Inspecting a database file (used to prove a copy is a faithful copy before the service is pointed at it).

use std::path::Path;

use syntrac_core::EventRepository;
use syntrac_store::{inspect_database, read_schema_version, SqliteStore};
use syntrac_testkit::fixtures::{at_secs, process_started, EventFactory};

fn database_with_some_data(path: &Path) {
    let mut store = SqliteStore::open(path).unwrap();
    let mut factory = EventFactory::default();
    let events = vec![
        factory.make(at_secs(100), process_started(10, 1, 90, r"C:\a\one.exe")),
        factory.make(at_secs(101), process_started(11, 1, 91, r"C:\a\two.exe")),
    ];
    store.append(&events).unwrap();
}

#[test]
fn a_healthy_database_reports_its_version_integrity_and_a_row_count_per_table() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.sqlite");
    database_with_some_data(&path);

    let report = inspect_database(&path).unwrap();

    assert!(report.integrity_ok, "{}", report.integrity_message);
    assert_eq!(report.schema_version, SqliteStore::LATEST_SCHEMA_VERSION);
    let count = |table: &str| report.tables.iter().find(|(name, _)| name == table).map(|(_, rows)| *rows);
    assert_eq!(count("events"), Some(2));
    assert_eq!(count("flows"), Some(0));
    assert!(report.tables.iter().all(|(name, _)| !name.starts_with("sqlite_")), "internal tables are not listed");
}

#[test]
fn tables_are_listed_in_name_order_so_two_reports_can_be_compared_directly() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.sqlite");
    database_with_some_data(&path);
    let report = inspect_database(&path).unwrap();
    let names: Vec<&String> = report.tables.iter().map(|(name, _)| name).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);
}

#[test]
fn a_copy_of_the_files_reports_exactly_the_same_as_the_original() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("a.sqlite");
    database_with_some_data(&original);
    let copy = dir.path().join("b.sqlite");
    std::fs::copy(&original, &copy).unwrap();

    assert_eq!(inspect_database(&original).unwrap(), inspect_database(&copy).unwrap());
}

#[test]
fn inspecting_never_changes_the_database() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.sqlite");
    database_with_some_data(&path);
    let before = inspect_database(&path).unwrap();
    for _ in 0..3 {
        inspect_database(&path).unwrap();
    }
    assert_eq!(inspect_database(&path).unwrap(), before);
}

#[test]
fn a_missing_file_is_a_clear_error_and_does_not_create_one() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nope.sqlite");
    let error = inspect_database(&path).unwrap_err();
    assert!(
        error.to_string().contains("nope.sqlite") || error.to_string().to_lowercase().contains("unable"),
        "{error}"
    );
    assert!(!path.exists(), "inspecting must not create the database");
}

#[test]
fn a_file_that_is_not_a_database_is_an_error_not_a_pass() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("junk.sqlite");
    std::fs::write(&path, vec![0x41u8; 8192]).unwrap();
    assert!(inspect_database(&path).is_err());
}

#[test]
fn a_database_that_is_damaged_in_the_middle_is_reported_as_failing_the_integrity_check_or_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.sqlite");
    {
        let mut store = SqliteStore::open(&path).unwrap();
        let mut factory = EventFactory::default();
        // enough rows to span several pages
        let events: Vec<_> = (0..400)
            .map(|i| factory.make(at_secs(100 + i), process_started(1000 + i as u32, 1, 90 + i, r"C:\a\program.exe")))
            .collect();
        store.append(&events).unwrap();
    }
    // Overwrite a stretch inside the file (past the header) with garbage.
    let mut bytes = std::fs::read(&path).unwrap();
    assert!(bytes.len() > 20_000, "the fixture should span several pages");
    for byte in &mut bytes[8192..12288] {
        *byte = 0xFF;
    }
    std::fs::write(&path, bytes).unwrap();

    // If SQLite refuses to read it at all, that is also a failure to verify; only a successful read must not look healthy.
    if let Ok(report) = inspect_database(&path) {
        assert!(!report.integrity_ok, "damage must not look healthy");
    }
}

// --- reading only the schema version (what the installer needs; the full inspection reads the whole file)

#[test]
fn the_schema_version_alone_is_read_from_a_database() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.sqlite");
    database_with_some_data(&path);

    assert_eq!(read_schema_version(&path).unwrap(), SqliteStore::LATEST_SCHEMA_VERSION);
}

#[test]
fn reading_the_schema_version_of_a_missing_file_is_an_error_and_creates_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("absent.sqlite");

    assert!(read_schema_version(&path).is_err());
    assert!(!path.exists());
}

#[test]
fn reading_the_schema_version_of_something_that_is_not_a_database_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("not-a-database.sqlite");
    std::fs::write(&path, vec![0x41u8; 8192]).unwrap();

    assert!(read_schema_version(&path).is_err());
}

#[test]
fn reading_the_schema_version_leaves_the_database_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.sqlite");
    database_with_some_data(&path);
    let before = inspect_database(&path).unwrap();

    read_schema_version(&path).unwrap();

    assert_eq!(inspect_database(&path).unwrap(), before);
}
