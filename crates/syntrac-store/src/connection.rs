//! Connection lifecycle: open (read-write or read-only), migrate, shared helpers.

use std::fmt::Display;
use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};
use syntrac_core::RepoError;

use crate::migrations::{MIGRATIONS, REWRITING_MIGRATION};

/// How long a connection waits for another to release a lock before failing.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

pub struct SqliteStore {
    pub(crate) conn: Connection,
    /// Things `open` did or could not do, for the host to log (see `take_notices`).
    pub(crate) notices: Vec<String>,
}

pub(crate) fn backend(error: impl Display) -> RepoError {
    RepoError::Backend(error.to_string())
}

fn read_version(conn: &Connection) -> Result<u32, RepoError> {
    conn.pragma_query_value(None, "user_version", |row| row.get(0)).map_err(backend)
}

/// A consistent copy of the database next to it (`<name>.v<version>-backup`, then `-2`, `-3` ... if that exists), made
/// with `VACUUM INTO`, which is safe while the file is open. An existing file is never overwritten. If the copy cannot be
/// written, the upgrade does not happen: nothing is rewritten without a way back.
fn write_backup(conn: &Connection, database: &Path, version: u32) -> Result<(), RepoError> {
    let file_name = database.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut attempt = 1;
    let target = loop {
        let suffix = if attempt == 1 { String::new() } else { format!("-{attempt}") };
        let candidate = database.with_file_name(format!("{file_name}.v{version}-backup{suffix}"));
        if !candidate.exists() {
            break candidate;
        }
        attempt += 1;
    };
    conn.execute("VACUUM INTO ?1", [target.to_string_lossy().as_ref()]).map_err(|e| {
        RepoError::Backend(format!("could not write the backup {} before upgrading: {e}", target.display()))
    })?;
    Ok(())
}

impl SqliteStore {
    pub const LATEST_SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;

    /// Opens (creating if needed) a database file in WAL mode and applies pending migrations.
    /// Only the collector service should use this; everything else uses [`SqliteStore::open_read_only`].
    pub fn open(path: &Path) -> Result<Self, RepoError> {
        let conn = Connection::open(path).map_err(backend)?;
        conn.busy_timeout(BUSY_TIMEOUT).map_err(backend)?;
        conn.pragma_update(None, "journal_mode", "WAL").map_err(backend)?;
        conn.pragma_update(None, "synchronous", "NORMAL").map_err(backend)?;
        let existing_version = read_version(&conn)?;
        // An existing (version 1 or later) database that has not yet had the table rewrite applied: back it up first,
        // compact it afterwards. A brand-new or already-current database needs neither.
        let will_be_rewritten = (1..REWRITING_MIGRATION).contains(&existing_version);
        if will_be_rewritten {
            write_backup(&conn, path, existing_version)?;
        }
        let mut store = Self::migrated(conn)?;
        if will_be_rewritten {
            // The same rewrite also switches the file to incremental auto-vacuum (see `ensure_incremental_auto_vacuum`).
            let _ = store.conn.pragma_update(None, "auto_vacuum", crate::storage::AUTO_VACUUM_INCREMENTAL);
            store.compact()?;
        }
        store.ensure_incremental_auto_vacuum();
        Ok(store)
    }

    /// Gives the pages freed by a table rewrite back to the file system, and empties the write-ahead log.
    /// Needs about the database's size in temporary space; runs once, after the layout upgrade.
    fn compact(&self) -> Result<(), RepoError> {
        self.conn.execute_batch("VACUUM").map_err(backend)?;
        self.conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(())).map_err(backend)
    }

    pub fn open_in_memory() -> Result<Self, RepoError> {
        Self::migrated(Connection::open_in_memory().map_err(backend)?)
    }

    /// Opens an existing database for reading only, for example from the UI while the service writes.
    ///
    /// SQLite needs to create the `-shm` file for a WAL database, so the containing directory must
    /// let this user create files. Fails if the database is missing or not at the current schema
    /// (the service migrates it; the reader never does).
    pub fn open_read_only(path: &Path) -> Result<Self, RepoError> {
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let conn = Connection::open_with_flags(path, flags).map_err(backend)?;
        conn.busy_timeout(BUSY_TIMEOUT).map_err(backend)?;
        let store = Self { conn, notices: Vec::new() };

        let version = store.schema_version()?;
        if version != Self::LATEST_SCHEMA_VERSION {
            return Err(RepoError::SchemaMismatch { found: version, expected: Self::LATEST_SCHEMA_VERSION });
        }
        Ok(store)
    }

    pub fn schema_version(&self) -> Result<u32, RepoError> {
        read_version(&self.conn)
    }

    fn migrated(conn: Connection) -> Result<Self, RepoError> {
        let mut store = Self { conn, notices: Vec::new() };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&mut self) -> Result<(), RepoError> {
        let current = self.schema_version()?;
        if current > Self::LATEST_SCHEMA_VERSION {
            return Err(RepoError::SchemaMismatch { found: current, expected: Self::LATEST_SCHEMA_VERSION });
        }
        for (index, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
            let tx = self.conn.transaction().map_err(backend)?;
            tx.execute_batch(sql).map_err(backend)?;
            tx.pragma_update(None, "user_version", index as u32 + 1).map_err(backend)?;
            tx.commit().map_err(backend)?;
        }
        Ok(())
    }
}
