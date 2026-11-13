//! Size accounting and space recovery for the SQLite store.
//!
//! Deleting rows only turns pages into free pages inside the file. With incremental auto-vacuum (switched on by `open`), freed
//! pages can be handed back to the drive in bounded steps, and a WAL checkpoint then truncates the file. Everything here is safe
//! to call at any time; a failure is reported to the caller, never fatal to the store.

use syntrac_core::storage_guard::{DatabaseUsage, StorageStats};
use syntrac_core::{RepoError, RollupRepository, Timestamp};

use crate::connection::{backend, SqliteStore};

/// `PRAGMA auto_vacuum` value for incremental mode.
pub(crate) const AUTO_VACUUM_INCREMENTAL: i64 = 2;

impl SqliteStore {
    pub(crate) fn auto_vacuum_mode(&self) -> Result<i64, RepoError> {
        self.conn.pragma_query_value(None, "auto_vacuum", |row| row.get(0)).map_err(backend)
    }

    /// Switches an existing database to incremental auto-vacuum (a one-time `VACUUM` rewrites the file; the data does not
    /// change). Not fatal: if it cannot be done now (another connection holds a lock), a notice is recorded and it is tried
    /// again at the next start. The collector keeps working either way; only space recovery is limited until it succeeds.
    pub(crate) fn ensure_incremental_auto_vacuum(&mut self) {
        match self.auto_vacuum_mode() {
            Ok(AUTO_VACUUM_INCREMENTAL) => return,
            Ok(_) => {}
            Err(error) => {
                self.notices.push(format!("could not read the auto-vacuum mode: {error}"));
                return;
            }
        }
        let converted = self
            .conn
            .pragma_update(None, "auto_vacuum", AUTO_VACUUM_INCREMENTAL)
            .and_then(|()| self.conn.execute_batch("VACUUM"));
        match (converted, self.auto_vacuum_mode()) {
            (Ok(()), Ok(AUTO_VACUUM_INCREMENTAL)) => {
                self.notices.push("switched the database to incremental auto-vacuum".to_owned());
            }
            (Ok(()), _) => self.notices.push(
                "the database could not be switched to incremental auto-vacuum; freed space will not be returned to the drive"
                    .to_owned(),
            ),
            (Err(error), _) => self.notices.push(format!(
                "could not switch the database to incremental auto-vacuum now ({error}); will try again at the next start"
            )),
        }
    }

    /// Messages about things `open` did or could not do (for the host to log). Taking them clears them.
    pub fn take_notices(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notices)
    }
}

fn pragma_u64(store: &SqliteStore, name: &str) -> Result<u64, RepoError> {
    let value: i64 = store.conn.pragma_query_value(None, name, |row| row.get(0)).map_err(backend)?;
    Ok(u64::try_from(value).unwrap_or(0))
}

impl StorageStats for SqliteStore {
    fn usage(&self) -> Result<DatabaseUsage, RepoError> {
        let page_size = pragma_u64(self, "page_size")?;
        let page_count = pragma_u64(self, "page_count")?;
        let free_pages = pragma_u64(self, "freelist_count")?;
        let oldest_rollup: Option<i64> = self
            .conn
            .query_row("SELECT MIN(hour_start) FROM rollup_system_hour", [], |row| row.get(0))
            .map_err(backend)?;
        Ok(DatabaseUsage {
            file_bytes: page_count.saturating_mul(page_size),
            live_bytes: page_count.saturating_sub(free_pages).saturating_mul(page_size),
            reclaimable_bytes: free_pages.saturating_mul(page_size),
            oldest_detail: self.earliest_detail()?,
            oldest_rollup: oldest_rollup.map(Timestamp::from_micros),
        })
    }

    fn reclaim_space(&mut self) -> Result<(), RepoError> {
        // A no-op unless incremental auto-vacuum is on. Frees every free page, then truncates the log so the file can shrink
        // (a reader that is mid-read can delay the truncation; the next call finishes it).
        // Each step of this statement frees ONE page, so it is stepped until it is exhausted.
        {
            let mut statement = self.conn.prepare("PRAGMA incremental_vacuum").map_err(backend)?;
            let mut rows = statement.query([]).map_err(backend)?;
            while rows.next().map_err(backend)?.is_some() {}
        }
        self.conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(())).map_err(backend)
    }
}
