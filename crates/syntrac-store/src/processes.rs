//! `ProcessRepository` for SQLite.

use rusqlite::{params, OptionalExtension, Row};
use syntrac_core::{ProcessInstanceId, ProcessRecord, ProcessRepository, RepoError, Timestamp};

use crate::codec::{instance_from_column, instance_from_text, instance_to_text};
use crate::connection::{backend, SqliteStore};

fn process_from_row(row: &Row<'_>) -> Result<ProcessRecord, rusqlite::Error> {
    let parent_id: Option<String> = row.get("parent_id")?;
    Ok(ProcessRecord {
        id: instance_from_column(row.get("id")?)?,
        pid: row.get("pid")?,
        parent_pid: row.get("parent_pid")?,
        parent_id: parent_id.map(instance_from_column).transpose()?,
        image_path: row.get("image_path")?,
        command_line: row.get("command_line")?,
        started: Timestamp::from_micros(row.get("started")?),
        ended: row.get::<_, Option<i64>>("ended")?.map(Timestamp::from_micros),
        exit_code: row.get("exit_code")?,
    })
}

impl ProcessRepository for SqliteStore {
    fn record_start(&mut self, record: ProcessRecord) -> Result<(), RepoError> {
        // COALESCE keeps already-known values, so a late snapshot never clears a recorded exit.
        self.conn
            .execute(
                "INSERT INTO processes
                 (id, pid, parent_pid, parent_id, image_path, command_line, started, ended, exit_code)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(id) DO UPDATE SET
                   pid = excluded.pid,
                   parent_pid = excluded.parent_pid,
                   parent_id = COALESCE(excluded.parent_id, parent_id),
                   image_path = excluded.image_path,
                   command_line = COALESCE(excluded.command_line, command_line),
                   started = excluded.started,
                   ended = COALESCE(excluded.ended, ended),
                   exit_code = COALESCE(excluded.exit_code, exit_code)",
                params![
                    instance_to_text(record.id),
                    record.pid,
                    record.parent_pid,
                    record.parent_id.map(instance_to_text),
                    record.image_path,
                    record.command_line,
                    record.started.as_micros(),
                    record.ended.map(Timestamp::as_micros),
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
        let changed = self
            .conn
            .execute(
                "UPDATE processes SET ended = ?2, exit_code = ?3 WHERE id = ?1",
                params![instance_to_text(id), ended.as_micros(), exit_code],
            )
            .map_err(backend)?;
        if changed == 0 {
            return Err(RepoError::NotFound);
        }
        Ok(())
    }

    fn get(&self, id: ProcessInstanceId) -> Result<Option<ProcessRecord>, RepoError> {
        self.conn
            .query_row("SELECT * FROM processes WHERE id = ?1", [instance_to_text(id)], process_from_row)
            .optional()
            .map_err(backend)
    }

    fn resolve(&self, pid: u32, at: Timestamp) -> Result<Option<ProcessInstanceId>, RepoError> {
        let found: Option<String> = self
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
        found.map(|text| instance_from_text(&text)).transpose()
    }
}
