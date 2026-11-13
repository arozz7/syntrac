//! SQLite implementation of the repository interfaces.

use std::fmt::Display;
use std::path::Path;

use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row};
use syntrac_core::{
    EventEnvelope, EventPayload, EventQuery, EventRepository, ProcessInstanceId, ProcessRecord, ProcessRepository,
    RepoError, Severity, Timestamp,
};
use uuid::Uuid;

use crate::migrations::MIGRATIONS;

pub struct SqliteStore {
    conn: Connection,
}

fn backend(error: impl Display) -> RepoError {
    RepoError::Backend(error.to_string())
}

impl SqliteStore {
    pub const LATEST_SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;

    /// Opens (creating if needed) a database file in WAL mode and applies pending migrations.
    pub fn open(path: &Path) -> Result<Self, RepoError> {
        let conn = Connection::open(path).map_err(backend)?;
        conn.pragma_update(None, "journal_mode", "WAL").map_err(backend)?;
        conn.pragma_update(None, "synchronous", "NORMAL").map_err(backend)?;
        Self::from_connection(conn)
    }

    pub fn open_in_memory() -> Result<Self, RepoError> {
        Self::from_connection(Connection::open_in_memory().map_err(backend)?)
    }

    fn from_connection(conn: Connection) -> Result<Self, RepoError> {
        let mut store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    pub fn schema_version(&self) -> Result<u32, RepoError> {
        self.conn.pragma_query_value(None, "user_version", |row| row.get(0)).map_err(backend)
    }

    fn migrate(&mut self) -> Result<(), RepoError> {
        let current = self.schema_version()?;
        if current > Self::LATEST_SCHEMA_VERSION {
            return Err(RepoError::Backend(format!(
                "database schema v{current} is newer than supported v{}",
                Self::LATEST_SCHEMA_VERSION
            )));
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

fn parse_id(text: String) -> Result<ProcessInstanceId, rusqlite::Error> {
    Uuid::parse_str(&text)
        .map(ProcessInstanceId::from_uuid)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))
}

fn process_from_row(row: &Row<'_>) -> Result<ProcessRecord, rusqlite::Error> {
    let parent_id: Option<String> = row.get("parent_id")?;
    Ok(ProcessRecord {
        id: parse_id(row.get("id")?)?,
        pid: row.get("pid")?,
        parent_pid: row.get("parent_pid")?,
        parent_id: parent_id.map(parse_id).transpose()?,
        image_path: row.get("image_path")?,
        command_line: row.get("command_line")?,
        started: Timestamp::from_micros(row.get("started")?),
        ended: row.get::<_, Option<i64>>("ended")?.map(Timestamp::from_micros),
        exit_code: row.get("exit_code")?,
    })
}

impl ProcessRepository for SqliteStore {
    fn record_start(&mut self, record: ProcessRecord) -> Result<(), RepoError> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO processes
                 (id, pid, parent_pid, parent_id, image_path, command_line, started, ended, exit_code)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    record.id.as_uuid().to_string(),
                    record.pid,
                    record.parent_pid,
                    record.parent_id.map(|id| id.as_uuid().to_string()),
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
                params![id.as_uuid().to_string(), ended.as_micros(), exit_code],
            )
            .map_err(backend)?;
        if changed == 0 {
            return Err(RepoError::NotFound);
        }
        Ok(())
    }

    fn get(&self, id: ProcessInstanceId) -> Result<Option<ProcessRecord>, RepoError> {
        self.conn
            .query_row("SELECT * FROM processes WHERE id = ?1", [id.as_uuid().to_string()], process_from_row)
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
        found.map(|text| parse_id(text).map_err(backend)).transpose()
    }
}

fn severity_to_text(severity: Severity) -> Result<String, RepoError> {
    match serde_json::to_value(severity).map_err(backend)? {
        serde_json::Value::String(text) => Ok(text),
        other => Err(RepoError::Backend(format!("unexpected severity encoding: {other}"))),
    }
}

fn severity_from_text(text: &str) -> Result<Severity, RepoError> {
    serde_json::from_value(serde_json::Value::String(text.to_owned())).map_err(backend)
}

fn event_from_row(row: &Row<'_>) -> Result<EventEnvelope, RepoError> {
    let id: String = row.get("id").map_err(backend)?;
    let owner: Option<String> = row.get("process_instance_id").map_err(backend)?;
    let severity: Option<String> = row.get("severity").map_err(backend)?;
    let payload: String = row.get("payload").map_err(backend)?;
    Ok(EventEnvelope {
        id: Uuid::parse_str(&id).map_err(backend)?,
        ts: Timestamp::from_micros(row.get("ts").map_err(backend)?),
        process_instance_id: owner.map(parse_id).transpose().map_err(backend)?,
        severity: severity.as_deref().map(severity_from_text).transpose()?,
        payload: serde_json::from_str::<EventPayload>(&payload).map_err(backend)?,
    })
}

impl EventRepository for SqliteStore {
    fn append(&mut self, events: &[EventEnvelope]) -> Result<(), RepoError> {
        let tx = self.conn.transaction().map_err(backend)?;
        {
            let mut insert = tx
                .prepare(
                    "INSERT INTO events (id, ts, category, kind, process_instance_id, severity, payload)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                )
                .map_err(backend)?;
            for event in events {
                insert
                    .execute(params![
                        event.id.to_string(),
                        event.ts.as_micros(),
                        event.payload.category().as_str(),
                        event.payload.kind(),
                        event.process_instance_id.map(|id| id.as_uuid().to_string()),
                        event.severity.map(severity_to_text).transpose()?,
                        serde_json::to_string(&event.payload).map_err(backend)?,
                    ])
                    .map_err(backend)?;
            }
        }
        tx.commit().map_err(backend)
    }

    fn query(&self, query: &EventQuery) -> Result<Vec<EventEnvelope>, RepoError> {
        let mut clauses: Vec<&str> = Vec::new();
        let mut args: Vec<rusqlite::types::Value> = Vec::new();
        if let Some(from) = query.from {
            clauses.push("ts >= ?");
            args.push(from.as_micros().into());
        }
        if let Some(to) = query.to {
            clauses.push("ts <= ?");
            args.push(to.as_micros().into());
        }
        if let Some(category) = query.category {
            clauses.push("category = ?");
            args.push(category.as_str().to_owned().into());
        }
        if let Some(process) = query.process {
            clauses.push("process_instance_id = ?");
            args.push(process.as_uuid().to_string().into());
        }

        let mut sql = String::from("SELECT * FROM events");
        if !clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&clauses.join(" AND "));
        }
        sql.push_str(" ORDER BY ts ASC, rowid ASC");
        if let Some(limit) = query.limit {
            sql.push_str(&format!(" LIMIT {limit}"));
        }

        let mut statement = self.conn.prepare(&sql).map_err(backend)?;
        let mut rows = statement.query(params_from_iter(args)).map_err(backend)?;
        let mut events = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            events.push(event_from_row(row)?);
        }
        Ok(events)
    }
}
