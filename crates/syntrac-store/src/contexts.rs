//! `ContextRepository` for SQLite: how each program has been seen running (the accounts and the Windows services), in
//! `executable_accounts` and `executable_services`, keyed by the same executable id as the rollups.
//!
//! The rows are permanent (retention never touches them) and only first and last seen are kept, with MIN and MAX, so recording
//! the same sighting twice changes nothing.

use std::collections::BTreeMap;

use rusqlite::{params, Transaction};
use syntrac_core::rollup::executable_key;
use syntrac_core::{
    AccountKind, AccountSeen, ContextObservation, ContextRepository, ContextWrite, Executable, ExecutableContext,
    RepoError, ServiceSeen, Timestamp,
};

use crate::connection::{backend, SqliteStore};
use crate::rollups::executable_id;

fn count(tx: &Transaction<'_>, sql: &str, parameters: impl rusqlite::Params) -> Result<i64, RepoError> {
    tx.query_row(sql, parameters, |row| row.get(0)).map_err(backend)
}

fn record_account(
    tx: &Transaction<'_>,
    exe_id: i64,
    observation: &ContextObservation,
    at: Timestamp,
    write: &mut ContextWrite,
) -> Result<(), RepoError> {
    let known = count(
        tx,
        "SELECT COUNT(*) FROM executable_accounts WHERE exe_id = ?1 AND sid = ?2",
        params![exe_id, observation.sid],
    )?;
    if known == 0 {
        write.new_accounts += 1;
    }
    tx.execute(
        "INSERT INTO executable_accounts (exe_id, sid, account_name, account_kind, first_seen, last_seen)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)
         ON CONFLICT(exe_id, sid) DO UPDATE SET
             first_seen = MIN(first_seen, excluded.first_seen),
             last_seen = MAX(last_seen, excluded.last_seen),
             account_name = COALESCE(excluded.account_name, account_name),
             account_kind = excluded.account_kind",
        params![exe_id, observation.sid, observation.account_name, observation.kind.as_str(), at.as_micros()],
    )
    .map_err(backend)?;
    Ok(())
}

fn record_services(
    tx: &Transaction<'_>,
    exe_id: i64,
    observation: &ContextObservation,
    at: Timestamp,
    write: &mut ContextWrite,
) -> Result<(), RepoError> {
    for service in &observation.services {
        let known = count(
            tx,
            "SELECT COUNT(*) FROM executable_services WHERE exe_id = ?1 AND service_name = ?2",
            params![exe_id, service.name],
        )?;
        if known == 0 {
            write.new_services += 1;
        }
        tx.execute(
            "INSERT INTO executable_services (exe_id, service_name, display_name, first_seen, last_seen)
             VALUES (?1, ?2, ?3, ?4, ?4)
             ON CONFLICT(exe_id, service_name) DO UPDATE SET
                 first_seen = MIN(first_seen, excluded.first_seen),
                 last_seen = MAX(last_seen, excluded.last_seen),
                 display_name = COALESCE(excluded.display_name, display_name)",
            params![exe_id, service.name, service.display_name, at.as_micros()],
        )
        .map_err(backend)?;
    }
    Ok(())
}

/// Every account row, optionally of one program, with the program's key and path.
const ACCOUNTS: &str = "
    SELECT e.path_key, e.path, a.sid, a.account_name, a.account_kind, a.first_seen, a.last_seen
    FROM executable_accounts a JOIN executables e ON e.id = a.exe_id";
const SERVICES: &str = "
    SELECT e.path_key, e.path, s.service_name, s.display_name, s.first_seen, s.last_seen
    FROM executable_services s JOIN executables e ON e.id = s.exe_id";

type Contexts = BTreeMap<String, (Executable, ExecutableContext)>;

fn read_contexts(store: &SqliteStore, only_key: Option<&str>) -> Result<Contexts, RepoError> {
    let filter = if only_key.is_some() { " WHERE e.path_key = ?1" } else { "" };
    let key_parameter: Vec<&str> = only_key.into_iter().collect();
    let mut contexts: Contexts = BTreeMap::new();
    let mut statement =
        store.conn.prepare(&format!("{ACCOUNTS}{filter} ORDER BY a.last_seen DESC, a.sid")).map_err(backend)?;
    let mut rows = statement.query(rusqlite::params_from_iter(key_parameter.iter())).map_err(backend)?;
    while let Some(row) = rows.next().map_err(backend)? {
        let (key, path): (String, String) = (row.get(0).map_err(backend)?, row.get(1).map_err(backend)?);
        let kind: String = row.get(4).map_err(backend)?;
        contexts
            .entry(key.clone())
            .or_insert_with(|| (Executable { key, path }, ExecutableContext::default()))
            .1
            .accounts
            .push(AccountSeen {
                sid: row.get(2).map_err(backend)?,
                name: row.get(3).map_err(backend)?,
                kind: AccountKind::parse(&kind),
                first_seen: Timestamp::from_micros(row.get(5).map_err(backend)?),
                last_seen: Timestamp::from_micros(row.get(6).map_err(backend)?),
            });
    }

    let mut statement = store
        .conn
        .prepare(&format!("{SERVICES}{filter} ORDER BY s.last_seen DESC, s.service_name"))
        .map_err(backend)?;
    let mut rows = statement.query(rusqlite::params_from_iter(key_parameter.iter())).map_err(backend)?;
    while let Some(row) = rows.next().map_err(backend)? {
        let (key, path): (String, String) = (row.get(0).map_err(backend)?, row.get(1).map_err(backend)?);
        contexts
            .entry(key.clone())
            .or_insert_with(|| (Executable { key, path }, ExecutableContext::default()))
            .1
            .services
            .push(ServiceSeen {
                name: row.get(2).map_err(backend)?,
                display_name: row.get(3).map_err(backend)?,
                first_seen: Timestamp::from_micros(row.get(4).map_err(backend)?),
                last_seen: Timestamp::from_micros(row.get(5).map_err(backend)?),
            });
    }
    Ok(contexts)
}

impl ContextRepository for SqliteStore {
    fn record_context(
        &mut self,
        observations: &[ContextObservation],
        at: Timestamp,
    ) -> Result<ContextWrite, RepoError> {
        let tx = self.conn.transaction().map_err(backend)?;
        let mut ids = std::collections::HashMap::new();
        let mut write = ContextWrite::default();
        for observation in observations {
            let exe_id = executable_id(&tx, &mut ids, Some(&observation.exe))?;
            record_account(&tx, exe_id, observation, at, &mut write)?;
            record_services(&tx, exe_id, observation, at, &mut write)?;
        }
        tx.commit().map_err(backend)?;
        Ok(write)
    }

    fn context_of(&self, path: &str) -> Result<ExecutableContext, RepoError> {
        let Some(key) = executable_key(path) else { return Ok(ExecutableContext::default()) };
        Ok(read_contexts(self, Some(&key))?.into_values().next().map(|(_, context)| context).unwrap_or_default())
    }

    fn all_contexts(&self) -> Result<Vec<(Executable, ExecutableContext)>, RepoError> {
        Ok(read_contexts(self, None)?.into_values().collect())
    }
}
