//! Looks inside a database file without changing it: schema version, SQLite's own integrity check, and a row count
//! per table. Used to prove that a copy of the database (for example after moving it to another drive) is a faithful
//! copy before the collector is pointed at it. Works on any schema version, because a copy made before a migration
//! must be inspectable too.

use std::path::Path;

use rusqlite::{Connection, OpenFlags};
use syntrac_core::RepoError;

use crate::connection::backend;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseReport {
    pub schema_version: u32,
    /// SQLite's `PRAGMA integrity_check` returned exactly `ok`.
    pub integrity_ok: bool,
    /// `ok`, or SQLite's first complaints.
    pub integrity_message: String,
    /// `(table, rows)` for every user table, sorted by name so two reports compare directly.
    pub tables: Vec<(String, u64)>,
}

/// Read-only. A missing file is an error (it is never created), and so is a file SQLite cannot read as a database.
pub fn inspect_database(path: &Path) -> Result<DatabaseReport, RepoError> {
    if !path.exists() {
        return Err(RepoError::Backend(format!("there is no database file at {}", path.display())));
    }
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = Connection::open_with_flags(path, flags).map_err(backend)?;

    let schema_version: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0)).map_err(backend)?;
    let integrity_message = integrity_summary(&conn)?;
    let integrity_ok = integrity_message == "ok";

    let mut names_statement = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .map_err(backend)?;
    let names: Vec<String> =
        names_statement.query_map([], |row| row.get(0)).map_err(backend)?.collect::<Result<_, _>>().map_err(backend)?;

    let mut tables = Vec::with_capacity(names.len());
    for name in names {
        // The name comes from the database's own schema, and is quoted so an odd name cannot alter the statement.
        let quoted = name.replace('"', "\"\"");
        let rows: i64 =
            conn.query_row(&format!("SELECT COUNT(*) FROM \"{quoted}\""), [], |row| row.get(0)).map_err(backend)?;
        tables.push((name, u64::try_from(rows).unwrap_or(0)));
    }
    Ok(DatabaseReport { schema_version, integrity_ok, integrity_message, tables })
}

/// `ok`, or the first few problems joined together (a damaged database can report thousands).
fn integrity_summary(conn: &Connection) -> Result<String, RepoError> {
    let mut statement = conn.prepare("PRAGMA integrity_check(5)").map_err(backend)?;
    let messages: Vec<String> =
        statement.query_map([], |row| row.get(0)).map_err(backend)?.collect::<Result<_, _>>().map_err(backend)?;
    Ok(messages.join("; "))
}
