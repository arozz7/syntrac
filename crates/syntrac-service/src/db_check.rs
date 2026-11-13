//! The text printed by `syntrac-service --check-db <file>`. One line per fact, in a fixed order, so a script can
//! compare the report of a database with the report of its copy: apart from the first line (the path) they must be
//! identical.

use std::fmt::Write as _;
use std::path::Path;

use syntrac_store::DatabaseReport;

pub fn format_database_report(path: &Path, report: &DatabaseReport) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "database:  {}", path.display());
    let _ = writeln!(out, "schema:    v{}", report.schema_version);
    if report.integrity_ok {
        let _ = writeln!(out, "integrity: ok");
    } else {
        let _ = writeln!(out, "integrity: FAILED ({})", report.integrity_message);
    }
    for (table, rows) in &report.tables {
        let _ = writeln!(out, "table {table} {rows}");
    }
    out
}

/// A database passes when SQLite's own integrity check says so.
pub fn database_check_passed(report: &DatabaseReport) -> bool {
    report.integrity_ok
}
