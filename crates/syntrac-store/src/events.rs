//! `EventRepository` and `FlowQueries` for SQLite.
//!
//! Every event has a row in `events` (the timeline). A network flow is stored ONCE, in the indexed `flows` table, under
//! the same number (`seq`) as its event; the event row carries no payload (NULL) and the payload is rebuilt from `flows`
//! when read. Questions like "top destinations" never scan JSON.

use std::net::IpAddr;

use rusqlite::{params, params_from_iter, Row, Transaction};
use syntrac_core::{
    Destination, EventEnvelope, EventPayload, EventQuery, EventRepository, FlowQueries, FlowRow, NetProtocol,
    RepoError, Timestamp,
};

use crate::codec::{
    instance_from_blob, instance_to_blob, severity_from_text, severity_to_text, to_i64, to_u64, uuid_from_blob,
    uuid_to_blob, IdBytes,
};
use crate::connection::{backend, SqliteStore};

fn protocol_text(protocol: NetProtocol) -> &'static str {
    match protocol {
        NetProtocol::Tcp => "tcp",
        NetProtocol::Udp => "udp",
    }
}

fn protocol_from_text(text: &str) -> NetProtocol {
    if text == "udp" {
        NetProtocol::Udp
    } else {
        NetProtocol::Tcp
    }
}

/// The columns of an event row followed by the (possibly absent) columns of its flow, in this order.
const EVENT_COLUMNS: &str = "e.id, e.ts, e.process_instance_id, e.severity, e.payload,
        f.window_start, f.window_end, f.pid, f.protocol, f.remote_addr, f.remote_port,
        f.bytes_sent, f.bytes_received, f.opened, f.closed, f.failed";

/// A payload stored on the event itself (every kind except a flow), or the flow rebuilt from its `flows` row.
fn payload_from_row(row: &Row<'_>, event_id: &uuid::Uuid) -> Result<EventPayload, RepoError> {
    let stored: Option<String> = row.get(4).map_err(backend)?;
    if let Some(json) = stored {
        return serde_json::from_str::<EventPayload>(&json).map_err(backend);
    }
    let window_start: Option<i64> = row.get(5).map_err(backend)?;
    let Some(window_start) = window_start else {
        return Err(RepoError::Backend(format!("event {event_id} has no payload and no flow row")));
    };
    let remote_addr: String = row.get(9).map_err(backend)?;
    let protocol: String = row.get(8).map_err(backend)?;
    Ok(EventPayload::NetworkFlow {
        pid: row.get(7).map_err(backend)?,
        protocol: protocol_from_text(&protocol),
        remote_addr: remote_addr.parse::<IpAddr>().map_err(backend)?,
        remote_port: row.get(10).map_err(backend)?,
        bytes_sent: to_u64(row.get(11).map_err(backend)?),
        bytes_received: to_u64(row.get(12).map_err(backend)?),
        opened: row.get(13).map_err(backend)?,
        closed: row.get(14).map_err(backend)?,
        failed: row.get(15).map_err(backend)?,
        window_start: Timestamp::from_micros(window_start),
        window_end: Timestamp::from_micros(row.get(6).map_err(backend)?),
    })
}

fn event_from_row(row: &Row<'_>) -> Result<EventEnvelope, RepoError> {
    let id = uuid_from_blob(row.get::<_, IdBytes>(0).map_err(backend)?);
    let owner: Option<IdBytes> = row.get(2).map_err(backend)?;
    let severity: Option<String> = row.get(3).map_err(backend)?;
    Ok(EventEnvelope {
        id,
        ts: Timestamp::from_micros(row.get(1).map_err(backend)?),
        process_instance_id: owner.map(instance_from_blob),
        severity: severity.as_deref().map(severity_from_text).transpose()?,
        payload: payload_from_row(row, &id)?,
    })
}

/// Inserts the event row and returns its `seq`. A flow's payload is NOT stored here (see the module docs).
fn insert_event(tx: &Transaction<'_>, event: &EventEnvelope) -> Result<i64, RepoError> {
    let payload = match &event.payload {
        EventPayload::NetworkFlow { .. } => None,
        other => Some(serde_json::to_string(other).map_err(backend)?),
    };
    tx.execute(
        "INSERT INTO events (id, ts, category, kind, process_instance_id, severity, payload)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            uuid_to_blob(event.id),
            event.ts.as_micros(),
            event.payload.category().as_str(),
            event.payload.kind(),
            event.process_instance_id.map(instance_to_blob),
            event.severity.map(severity_to_text).transpose()?,
            payload,
        ],
    )
    .map_err(backend)?;
    Ok(tx.last_insert_rowid())
}

fn insert_flow(tx: &Transaction<'_>, seq: i64, event: &EventEnvelope) -> Result<(), RepoError> {
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
        "INSERT INTO flows (seq, window_start, window_end, process_instance_id, pid, protocol,
                            remote_addr, remote_port, bytes_sent, bytes_received, opened, closed, failed)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            seq,
            window_start.as_micros(),
            window_end.as_micros(),
            event.process_instance_id.map(instance_to_blob),
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
            let seq = insert_event(&tx, event)?;
            insert_flow(&tx, seq, event)?;
        }
        tx.commit().map_err(backend)
    }

    fn query(&self, query: &EventQuery) -> Result<Vec<EventEnvelope>, RepoError> {
        let mut clauses: Vec<&str> = Vec::new();
        let mut args: Vec<rusqlite::types::Value> = Vec::new();
        if let Some(from) = query.from {
            clauses.push("e.ts >= ?");
            args.push(from.as_micros().into());
        }
        if let Some(to) = query.to {
            clauses.push("e.ts <= ?");
            args.push(to.as_micros().into());
        }
        if let Some(category) = query.category {
            clauses.push("e.category = ?");
            args.push(category.as_str().to_owned().into());
        }
        if let Some(process) = query.process {
            clauses.push("e.process_instance_id = ?");
            args.push(instance_to_blob(process).to_vec().into());
        }

        let mut sql = format!("SELECT {EVENT_COLUMNS} FROM events e LEFT JOIN flows f ON f.seq = e.seq");
        if !clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&clauses.join(" AND "));
        }
        sql.push_str(" ORDER BY e.ts ASC, e.seq ASC");
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
    fn recent_flows(&self, from: Timestamp, to: Timestamp, limit: usize) -> Result<Vec<FlowRow>, RepoError> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT f.window_start, f.window_end, f.process_instance_id, f.pid, p.image_path, f.protocol,
                        f.remote_addr, f.remote_port, f.bytes_sent, f.bytes_received, f.opened, f.closed, f.failed
                 FROM flows f LEFT JOIN processes p ON p.id = f.process_instance_id
                 WHERE f.window_start >= ?1 AND f.window_start <= ?2
                 ORDER BY f.window_start DESC, f.rowid DESC
                 LIMIT ?3",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![from.as_micros(), to.as_micros(), i64::try_from(limit).unwrap_or(i64::MAX)])
            .map_err(backend)?;

        let mut flows = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let owner: Option<IdBytes> = row.get(2).map_err(backend)?;
            let protocol: String = row.get(5).map_err(backend)?;
            let addr: String = row.get(6).map_err(backend)?;
            flows.push(FlowRow {
                window_start: Timestamp::from_micros(row.get(0).map_err(backend)?),
                window_end: Timestamp::from_micros(row.get(1).map_err(backend)?),
                process: owner.map(instance_from_blob),
                pid: row.get(3).map_err(backend)?,
                image_path: row.get(4).map_err(backend)?,
                protocol: protocol_from_text(&protocol),
                remote_addr: addr.parse::<IpAddr>().map_err(backend)?,
                remote_port: row.get(7).map_err(backend)?,
                bytes_sent: to_u64(row.get(8).map_err(backend)?),
                bytes_received: to_u64(row.get(9).map_err(backend)?),
                opened: row.get(10).map_err(backend)?,
                closed: row.get(11).map_err(backend)?,
                failed: row.get(12).map_err(backend)?,
            });
        }
        Ok(flows)
    }
}
