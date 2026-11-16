//! `LauncherRepository` for SQLite: how each program has been seen to get started (`executable_launchers`), keyed by the same
//! executable id as the rollups, and when each kind of place was last read completely (`launcher_sweeps`).
//!
//! Permanent (retention never touches it), and only first and last seen are kept (MIN and MAX), so recording the same sighting
//! twice changes nothing but the text of the entry.

use std::collections::BTreeMap;

use rusqlite::{params, Transaction};
use syntrac_core::rollup::executable_key;
use syntrac_core::{
    Executable, LauncherKind, LauncherObservation, LauncherRepository, LauncherSeen, LauncherWrite, RepoError,
    Timestamp,
};

use crate::connection::{backend, SqliteStore};
use crate::rollups::executable_id;

fn record_one(
    tx: &Transaction<'_>,
    exe_id: i64,
    row: &LauncherObservation,
    at: Timestamp,
    write: &mut LauncherWrite,
) -> Result<(), RepoError> {
    let known: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM executable_launchers WHERE exe_id = ?1 AND kind = ?2 AND location = ?3",
            params![exe_id, row.kind.as_str(), row.location],
            |r| r.get(0),
        )
        .map_err(backend)?;
    if known == 0 {
        write.new += 1;
    }
    write.written += 1;
    tx.execute(
        "INSERT INTO executable_launchers (exe_id, kind, location, detail, arguments, first_seen, last_seen)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
         ON CONFLICT(exe_id, kind, location) DO UPDATE SET
             first_seen = MIN(first_seen, excluded.first_seen),
             last_seen = MAX(last_seen, excluded.last_seen),
             detail = excluded.detail,
             arguments = excluded.arguments",
        params![exe_id, row.kind.as_str(), row.location, row.detail, row.arguments, at.as_micros()],
    )
    .map_err(backend)?;
    Ok(())
}

/// Every entry, optionally of one program, with the program's key and path; `current` is whether the entry was still there when
/// its kind was last read completely.
const ENTRIES: &str = "
    SELECT e.path_key, e.path, l.kind, l.location, l.detail, l.arguments, l.first_seen, l.last_seen,
           COALESCE(l.last_seen >= s.completed_at, 0)
    FROM executable_launchers l
    JOIN executables e ON e.id = l.exe_id
    LEFT JOIN launcher_sweeps s ON s.kind = l.kind";

type Launchers = BTreeMap<String, (Executable, Vec<LauncherSeen>)>;

fn read_launchers(store: &SqliteStore, only_key: Option<&str>) -> Result<Launchers, RepoError> {
    let filter = if only_key.is_some() { " WHERE e.path_key = ?1" } else { "" };
    let key_parameter: Vec<&str> = only_key.into_iter().collect();
    let mut statement = store
        .conn
        .prepare(&format!("{ENTRIES}{filter} ORDER BY l.last_seen DESC, l.kind, l.location"))
        .map_err(backend)?;
    let mut rows = statement.query(rusqlite::params_from_iter(key_parameter.iter())).map_err(backend)?;
    let mut found: Launchers = BTreeMap::new();
    while let Some(row) = rows.next().map_err(backend)? {
        let (key, path): (String, String) = (row.get(0).map_err(backend)?, row.get(1).map_err(backend)?);
        let kind: String = row.get(2).map_err(backend)?;
        let Some(kind) = LauncherKind::parse(&kind) else { continue };
        let current: i64 = row.get(8).map_err(backend)?;
        found.entry(key.clone()).or_insert_with(|| (Executable { key, path }, Vec::new())).1.push(LauncherSeen {
            kind,
            location: row.get(3).map_err(backend)?,
            detail: row.get(4).map_err(backend)?,
            arguments: row.get(5).map_err(backend)?,
            first_seen: Timestamp::from_micros(row.get(6).map_err(backend)?),
            last_seen: Timestamp::from_micros(row.get(7).map_err(backend)?),
            current: current != 0,
        });
    }
    Ok(found)
}

impl LauncherRepository for SqliteStore {
    fn record_launchers(
        &mut self,
        rows: &[LauncherObservation],
        completed: &[LauncherKind],
        at: Timestamp,
    ) -> Result<LauncherWrite, RepoError> {
        let tx = self.conn.transaction().map_err(backend)?;
        let mut ids = std::collections::HashMap::new();
        let mut write = LauncherWrite::default();
        for row in rows {
            let exe_id = executable_id(&tx, &mut ids, Some(&row.exe))?;
            record_one(&tx, exe_id, row, at, &mut write)?;
        }
        for kind in completed {
            tx.execute(
                "INSERT INTO launcher_sweeps (kind, completed_at) VALUES (?1, ?2)
                 ON CONFLICT(kind) DO UPDATE SET completed_at = MAX(completed_at, excluded.completed_at)",
                params![kind.as_str(), at.as_micros()],
            )
            .map_err(backend)?;
        }
        tx.commit().map_err(backend)?;
        Ok(write)
    }

    fn launchers_of(&self, path: &str) -> Result<Vec<LauncherSeen>, RepoError> {
        let Some(key) = executable_key(path) else { return Ok(Vec::new()) };
        Ok(read_launchers(self, Some(&key))?.into_values().next().map(|(_, list)| list).unwrap_or_default())
    }

    fn all_launchers(&self) -> Result<Vec<(Executable, Vec<LauncherSeen>)>, RepoError> {
        Ok(read_launchers(self, None)?.into_values().collect())
    }
}
