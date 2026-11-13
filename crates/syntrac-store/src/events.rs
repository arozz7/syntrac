//! `EventRepository` and `FlowQueries` for SQLite.
//!
//! Every event goes into `events` (the timeline). Network flows are additionally written to the
//! indexed `flows` table so questions like "top destinations" never scan JSON.

use std::net::IpAddr;

use rusqlite::{params, params_from_iter, Row, Transaction};
use syntrac_core::{
    Destination, EventEnvelope, EventPayload, EventQuery, EventRepository, FlowQueries, NetProtocol, RepoError,
    Timestamp,
};
use uuid::Uuid;

use crate::codec::{instance_from_text, instance_to_text, severity_from_text, severity_to_text, to_i64, to_u64};
use crate::connection::{backend, SqliteStore};

fn protocol_text(protocol: NetProtocol) -> &'static str {
    match protocol {
        NetProtocol::Tcp => "tcp",
        NetProtocol::Udp => "udp",
    }
}

fn event_from_row(row: &Row<'_>) -> Result<EventEnvelope, RepoError> {
    let id: String = row.get("id").map_err(backend)?;
    let owner: Option<String> = row.get("process_instance_id").map_err(backend)?;
    let severity: Option<String> = row.get("severity").map_err(backend)?;
    let payload: String = row.get("payload").map_err(backend)?;
    Ok(EventEnvelope {
        id: Uuid::parse_str(&id).map_err(backend)?,
        ts: Timestamp::from_micros(row.get("ts").map_err(backend)?),
        process_instance_id: owner.as_deref().map(instance_from_text).transpose()?,
        severity: severity.as_deref().map(severity_from_text).transpose()?,
        payload: serde_json::from_str::<EventPayload>(&payload).map_err(backend)?,
    })
}

fn insert_event(tx: &Transaction<'_>, event: &EventEnvelope) -> Result<(), RepoError> {
    tx.execute(
        "INSERT INTO events (id, ts, category, kind, process_instance_id, severity, payload)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            event.id.to_string(),
            event.ts.as_micros(),
            event.payload.category().as_str(),
            event.payload.kind(),
            event.process_instance_id.map(instance_to_text),
            event.severity.map(severity_to_text).transpose()?,
            serde_json::to_string(&event.payload).map_err(backend)?,
        ],
    )
    .map_err(backend)?;
    Ok(())
}

fn insert_flow(tx: &Transaction<'_>, event: &EventEnvelope) -> Result<(), RepoError> {
    let EventPayload::NetworkFlow {
        pid,
        protocol,
        remote_addr,
        remote_port,
        bytes_sent,
        bytes_received,
        opened,
        closed,
        failed,
        window_start,
        window_end,
    } = &event.payload
    else {
        return Ok(());
    };
    tx.execute(
        "INSERT INTO flows (event_id, window_start, window_end, process_instance_id, pid, protocol,
                            remote_addr, remote_port, bytes_sent, bytes_received, opened, closed, failed)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            event.id.to_string(),
            window_start.as_micros(),
            window_end.as_micros(),
            event.process_instance_id.map(instance_to_text),
            pid,
            protocol_text(*protocol),
            remote_addr.to_string(),
            remote_port,
            to_i64(*bytes_sent),
            to_i64(*bytes_received),
            opened,
            closed,
            failed,
        ],
    )
    .map_err(backend)?;
    Ok(())
}

impl EventRepository for SqliteStore {
    fn append(&mut self, events: &[EventEnvelope]) -> Result<(), RepoError> {
        let tx = self.conn.transaction().map_err(backend)?;
        for event in events {
            insert_event(&tx, event)?;
            insert_flow(&tx, event)?;
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
            args.push(instance_to_text(process).into());
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

impl SqliteStore {
    /// How many events of each kind are stored, most common first. A diagnostic for operators.
    pub fn event_counts_by_kind(&self) -> Result<Vec<(String, u64)>, RepoError> {
        let mut statement = self
            .conn
            .prepare("SELECT kind, COUNT(*) FROM events GROUP BY kind ORDER BY COUNT(*) DESC, kind ASC")
            .map_err(backend)?;
        let counts = statement
            .query_map([], |row| Ok((row.get::<_, String>(0)?, to_u64(row.get(1)?))))
            .map_err(backend)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(backend)?;
        Ok(counts)
    }
}

impl FlowQueries for SqliteStore {
    fn top_destinations(&self, from: Timestamp, to: Timestamp, limit: usize) -> Result<Vec<Destination>, RepoError> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT remote_addr, remote_port,
                        SUM(bytes_sent), SUM(bytes_received), COUNT(*),
                        COUNT(DISTINCT COALESCE(process_instance_id, 'pid:' || pid))
                 FROM flows
                 WHERE window_start >= ?1 AND window_start <= ?2
                 GROUP BY remote_addr, remote_port
                 ORDER BY SUM(bytes_sent) + SUM(bytes_received) DESC
                 LIMIT ?3",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![from.as_micros(), to.as_micros(), i64::try_from(limit).unwrap_or(i64::MAX)])
            .map_err(backend)?;

        let mut destinations = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let addr: String = row.get(0).map_err(backend)?;
            destinations.push(Destination {
                remote_addr: addr.parse::<IpAddr>().map_err(backend)?,
                remote_port: row.get(1).map_err(backend)?,
                bytes_sent: to_u64(row.get(2).map_err(backend)?),
                bytes_received: to_u64(row.get(3).map_err(backend)?),
                flow_count: row.get(4).map_err(backend)?,
                process_count: row.get(5).map_err(backend)?,
            });
        }
        Ok(destinations)
    }
}
