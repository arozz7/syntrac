//! `InstallRepository` for SQLite: the products the Uninstall registry keys named (`install_records`), a clue about where a
//! program came from. One row per (registry view, key name); only first and last seen are kept (MIN and MAX), the other fields are
//! those of the newest sighting. When the keys were last read COMPLETELY is the `launcher_sweeps` row of kind `install_records`.
//!
//! Permanent (retention never touches it).

use rusqlite::params;
use syntrac_core::{InstallRecord, InstallRepository, InstallSeen, InstallWrite, RepoError, Timestamp};

use crate::connection::{backend, SqliteStore};

/// The `launcher_sweeps` kind that stands for "the Uninstall keys".
const SWEEP_KIND: &str = "install_records";

impl InstallRepository for SqliteStore {
    fn record_installs(&mut self, rows: &[InstallRecord], at: Timestamp) -> Result<InstallWrite, RepoError> {
        let tx = self.conn.transaction().map_err(backend)?;
        let mut write = InstallWrite::default();
        for row in rows {
            let known: i64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM install_records WHERE hive = ?1 AND key_name = ?2",
                    params![row.hive, row.key_name],
                    |r| r.get(0),
                )
                .map_err(backend)?;
            if known == 0 {
                write.new += 1;
            }
            write.written += 1;
            tx.execute(
                "INSERT INTO install_records
                     (hive, key_name, display_name, publisher, version, install_date, location, location_derived,
                      system_component, first_seen, last_seen)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)
                 ON CONFLICT(hive, key_name) DO UPDATE SET
                     first_seen = MIN(first_seen, excluded.first_seen),
                     display_name = CASE WHEN excluded.last_seen >= last_seen THEN excluded.display_name ELSE display_name END,
                     publisher = CASE WHEN excluded.last_seen >= last_seen THEN excluded.publisher ELSE publisher END,
                     version = CASE WHEN excluded.last_seen >= last_seen THEN excluded.version ELSE version END,
                     install_date = CASE WHEN excluded.last_seen >= last_seen THEN excluded.install_date ELSE install_date END,
                     location = CASE WHEN excluded.last_seen >= last_seen THEN excluded.location ELSE location END,
                     location_derived = CASE WHEN excluded.last_seen >= last_seen THEN excluded.location_derived ELSE location_derived END,
                     system_component = CASE WHEN excluded.last_seen >= last_seen THEN excluded.system_component ELSE system_component END,
                     last_seen = MAX(last_seen, excluded.last_seen)",
                params![
                    row.hive,
                    row.key_name,
                    row.display_name,
                    row.publisher,
                    row.version,
                    row.install_date,
                    row.location,
                    row.derived,
                    row.system_component,
                    at.as_micros()
                ],
            )
            .map_err(backend)?;
        }
        tx.execute(
            "INSERT INTO launcher_sweeps (kind, completed_at) VALUES (?1, ?2)
             ON CONFLICT(kind) DO UPDATE SET completed_at = MAX(completed_at, excluded.completed_at)",
            params![SWEEP_KIND, at.as_micros()],
        )
        .map_err(backend)?;
        tx.commit().map_err(backend)?;
        Ok(write)
    }

    fn installs(&self) -> Result<Vec<InstallSeen>, RepoError> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT r.hive, r.key_name, r.display_name, r.publisher, r.version, r.install_date, r.location,
                        r.location_derived, r.system_component, r.first_seen, r.last_seen,
                        COALESCE(r.last_seen >= s.completed_at, 0)
                 FROM install_records r LEFT JOIN launcher_sweeps s ON s.kind = ?1
                 ORDER BY r.hive, r.key_name",
            )
            .map_err(backend)?;
        let mut rows = statement.query([SWEEP_KIND]).map_err(backend)?;
        let mut found = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let flag =
                |index: usize| -> Result<bool, RepoError> { Ok(row.get::<_, i64>(index).map_err(backend)? != 0) };
            found.push(InstallSeen {
                record: InstallRecord {
                    hive: row.get(0).map_err(backend)?,
                    key_name: row.get(1).map_err(backend)?,
                    display_name: row.get(2).map_err(backend)?,
                    publisher: row.get(3).map_err(backend)?,
                    version: row.get(4).map_err(backend)?,
                    install_date: row.get(5).map_err(backend)?,
                    location: row.get(6).map_err(backend)?,
                    derived: flag(7)?,
                    system_component: flag(8)?,
                },
                first_seen: Timestamp::from_micros(row.get(9).map_err(backend)?),
                last_seen: Timestamp::from_micros(row.get(10).map_err(backend)?),
                current: flag(11)?,
            });
        }
        Ok(found)
    }
}
