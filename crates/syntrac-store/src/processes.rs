//! `ProcessRepository` and `ProcessQueries` for SQLite.

use rusqlite::{params, params_from_iter, OptionalExtension, Row};
use syntrac_core::{
    ProcessFilter, ProcessInstanceId, ProcessQueries, ProcessRecord, ProcessRepository, RepoError, Timestamp,
};

use crate::codec::{instance_from_blob, instance_to_blob, IdBytes};
use crate::connection::{backend, SqliteStore};

fn process_from_row(row: &Row<'_>) -> Result<ProcessRecord, rusqlite::Error> {
    let parent_id: Option<IdBytes> = row.get("parent_id")?;
    Ok(ProcessRecord {
        id: instance_from_blob(row.get("id")?),
        pid: row.get("pid")?,
        parent_pid: row.get("parent_pid")?,
        parent_id: parent_id.map(instance_from_blob),
        image_path: row.get("image_path")?,
        command_line: row.get("command_line")?,
        started: Timestamp::from_micros(row.get("started")?),
        ended: row.get::<_, Option<i64>>("ended")?.map(Timestamp::from_micros),
        ended_approximate: row.get::<_, i64>("ended_approximate")? != 0,
        exit_code: row.get("exit_code")?,
    })
}

impl ProcessRepository for SqliteStore {
    fn record_start(&mut self, record: ProcessRecord) -> Result<(), RepoError> {
        // COALESCE keeps already-known values, so a late snapshot never clears a recorded exit.
        self.conn
            .execute(
                "INSERT INTO processes
                 (id, pid, parent_pid, parent_id, image_path, command_line, started, ended, ended_approximate, exit_code)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(id) DO UPDATE SET
                   pid = excluded.pid,
                   parent_pid = excluded.parent_pid,
                   parent_id = COALESCE(excluded.parent_id, parent_id),
                   image_path = excluded.image_path,
                   command_line = COALESCE(excluded.command_line, command_line),
                   started = excluded.started,
                   ended = COALESCE(excluded.ended, ended),
                   ended_approximate = CASE WHEN excluded.ended IS NULL THEN ended_approximate
                                            ELSE excluded.ended_approximate END,
                   exit_code = COALESCE(excluded.exit_code, exit_code)",
                params![
                    instance_to_blob(record.id),
                    record.pid,
                    record.parent_pid,
                    record.parent_id.map(instance_to_blob),
                    record.image_path,
                    record.command_line,
                    record.started.as_micros(),
                    record.ended.map(Timestamp::as_micros),
                    record.ended_approximate,
                    record.exit_code,
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn record_exit(
        &mut self,
        id: ProcessInstanceId,
        ended: Timestamp,
        exit_code: Option<u32>,
    ) -> Result<(), RepoError> {
        // A real exit report always replaces an approximate bound.
        let changed = self
            .conn
            .execute(
                "UPDATE processes SET ended = ?2, exit_code = ?3, ended_approximate = 0 WHERE id = ?1",
                params![instance_to_blob(id), ended.as_micros(), exit_code],
            )
            .map_err(backend)?;
        if changed == 0 {
            return Err(RepoError::NotFound);
        }
        Ok(())
    }

    fn get(&self, id: ProcessInstanceId) -> Result<Option<ProcessRecord>, RepoError> {
        self.conn
            .query_row("SELECT * FROM processes WHERE id = ?1", [instance_to_blob(id)], process_from_row)
            .optional()
            .map_err(backend)
    }

    fn reconcile_running(&mut self, alive: &[ProcessInstanceId], observed_at: Timestamp) -> Result<u64, RepoError> {
        // The alive list is passed as one JSON array of hex ids (decoded to the stored 16 bytes by unhex) so a few hundred ids need no per-id parameters.
        let alive_json =
            serde_json::to_string(&alive.iter().map(|id| id.as_uuid().simple().to_string()).collect::<Vec<_>>())
                .map_err(backend)?;
        let closed = self
            .conn
            .execute(
                "UPDATE processes SET ended = ?1, ended_approximate = 1
                 WHERE ended IS NULL AND started <= ?1
                   AND id NOT IN (SELECT unhex(value) FROM json_each(?2))",
                params![observed_at.as_micros(), alive_json],
            )
            .map_err(backend)?;
        Ok(closed as u64)
    }

    fn resolve(&self, pid: u32, at: Timestamp) -> Result<Option<ProcessInstanceId>, RepoError> {
        let found: Option<IdBytes> = self
            .conn
            .query_row(
                "SELECT id FROM processes
                 WHERE pid = ?1 AND started <= ?2 AND (ended IS NULL OR ?2 <= ended)
                 ORDER BY started DESC LIMIT 1",
                params![pid, at.as_micros()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        Ok(found.map(instance_from_blob))
    }
}

/// Escapes `%`, `_` and the escape character itself so user text matches literally in a LIKE pattern.
fn like_pattern(text: &str) -> String {
    let mut pattern = String::from("%");
    for c in text.to_lowercase().chars() {
        if matches!(c, '%' | '_' | '\\') {
            pattern.push('\\');
        }
        pattern.push(c);
    }
    pattern.push('%');
    pattern
}

impl ProcessQueries for SqliteStore {
    fn list_processes(&self, filter: &ProcessFilter) -> Result<Vec<ProcessRecord>, RepoError> {
        let mut clauses: Vec<&str> = Vec::new();
        let mut args: Vec<rusqlite::types::Value> = Vec::new();
        if filter.running_only {
            clauses.push("ended IS NULL");
        }
        if let Some(text) = filter.name_contains.as_deref().filter(|t| !t.is_empty()) {
            clauses.push("lower(image_path) LIKE ? ESCAPE '\\'");
            args.push(like_pattern(text).into());
        }

        let mut sql = String::from("SELECT * FROM processes");
        if !clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&clauses.join(" AND "));
        }
        sql.push_str(" ORDER BY started DESC, rowid DESC LIMIT ?");
        args.push(i64::try_from(filter.limit).unwrap_or(i64::MAX).into());

        let mut statement = self.conn.prepare(&sql).map_err(backend)?;
        let rows = statement.query_map(params_from_iter(args), process_from_row).map_err(backend)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(backend)
    }
}
