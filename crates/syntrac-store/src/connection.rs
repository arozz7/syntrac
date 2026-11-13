//! Connection lifecycle: open (read-write or read-only), migrate, shared helpers.

use std::fmt::Display;
use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};
use syntrac_core::RepoError;

use crate::migrations::MIGRATIONS;

/// How long a connection waits for another to release a lock before failing.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

pub struct SqliteStore {
    pub(crate) conn: Connection,
}

pub(crate) fn backend(error: impl Display) -> RepoError {
    RepoError::Backend(error.to_string())
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
        Self::migrated(conn)
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
        let store = Self { conn };

        let version = store.schema_version()?;
        if version != Self::LATEST_SCHEMA_VERSION {
            return Err(RepoError::SchemaMismatch { found: version, expected: Self::LATEST_SCHEMA_VERSION });
        }
        Ok(store)
    }

    pub fn schema_version(&self) -> Result<u32, RepoError> {
        self.conn.pragma_query_value(None, "user_version", |row| row.get(0)).map_err(backend)
    }

    fn migrated(conn: Connection) -> Result<Self, RepoError> {
        let mut store = Self { conn };
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
