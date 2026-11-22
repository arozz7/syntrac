//! `syntrac-service --check-db <file>`: a plain-text report a script can compare between an original and its copy.

use std::path::Path;

use syntrac_service::{database_check_passed, format_database_report};
use syntrac_store::DatabaseReport;

fn report(integrity: &str, tables: &[(&str, u64)]) -> DatabaseReport {
    DatabaseReport {
        schema_version: 3,
        integrity_ok: integrity == "ok",
        integrity_message: integrity.to_owned(),
        tables: tables.iter().map(|(name, rows)| ((*name).to_owned(), *rows)).collect(),
    }
}

#[test]
fn the_report_has_one_stable_line_per_fact_so_two_reports_can_be_compared_as_text() {
    let text = format_database_report(
        Path::new(r"D:\Syntrac\data\syntrac.db"),
        &report("ok", &[("events", 83259), ("flows", 0)]),
    );
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], r"database:  D:\Syntrac\data\syntrac.db");
    assert!(lines.contains(&"schema:    v3"), "{text}");
    assert!(lines.contains(&"integrity: ok"), "{text}");
    assert!(lines.contains(&"table events 83259"), "{text}");
    assert!(lines.contains(&"table flows 0"), "{text}");
}

#[test]
fn the_path_line_is_the_only_line_that_differs_between_a_database_and_its_copy() {
    let tables = [("events", 5)];
    let original = format_database_report(Path::new("a.db"), &report("ok", &tables));
    let copy = format_database_report(Path::new("b.db"), &report("ok", &tables));
    assert_eq!(original.lines().skip(1).collect::<Vec<_>>(), copy.lines().skip(1).collect::<Vec<_>>());
}

#[test]
fn a_database_that_fails_its_integrity_check_does_not_pass_and_says_why() {
    let bad = report("*** in database main *** Page 7: btree page is damaged", &[("events", 5)]);
    assert!(!database_check_passed(&bad));
    let text = format_database_report(Path::new("a.db"), &bad);
    assert!(text.contains("integrity: FAILED"), "{text}");
    assert!(text.contains("Page 7"), "{text}");
}

#[test]
fn a_healthy_database_passes() {
    assert!(database_check_passed(&report("ok", &[])));
}

#[test]
fn the_version_report_names_the_program_version_and_the_newest_database_schema_it_knows() {
    let text = syntrac_service::format_version_report("1.2.3", 15);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines,
        ["syntrac-service 1.2.3", "schema:    v15"],
        "a script reads the second line, so the format is fixed"
    );
}
