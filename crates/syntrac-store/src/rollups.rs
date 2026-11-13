//! `RollupRepository` for SQLite. Loads the raw rows of one hour, stores a computed rollup (rows, ledgers and the watermark in
//! ONE transaction) and reads history back. It does no aggregation: that is `syntrac_core::rollup::roll_up_hour`.

use std::collections::HashMap;
use std::net::IpAddr;

use rusqlite::{params, Connection, OptionalExtension, Row, Transaction};
use syntrac_core::rollup::{executable_key, next_hour, FlowInputRow, ProcessSampleRow, StartRow};
use syntrac_core::{
    Executable, HourInput, HourRollup, LedgerEntry, OtherProcesses, ProcessHour, RepoError, RollupRepository,
    SystemHour, SystemMetrics, SystemPoint, Timestamp, TrafficHour,
};

use crate::codec::{to_i64, to_u64};
use crate::connection::{backend, SqliteStore};

/// The `executables` row that stands for "owner or path unknown".
const UNATTRIBUTED: i64 = 1;

fn ts(row: &Row<'_>, index: usize) -> Result<Timestamp, RepoError> {
    Ok(Timestamp::from_micros(row.get(index).map_err(backend)?))
}

fn ledger_from(first: Option<i64>, last: Option<i64>) -> Option<LedgerEntry> {
    Some(LedgerEntry { first_seen: Timestamp::from_micros(first?), last_seen: Timestamp::from_micros(last?) })
}

fn parse_addr(text: &str) -> Result<IpAddr, RepoError> {
    text.parse::<IpAddr>().map_err(backend)
}

/// The executable named by an `executables` row: `None` for the unattributed row.
fn executable_from(id: i64, key: String, path: String) -> Option<Executable> {
    if id == UNATTRIBUTED {
        None
    } else {
        Some(Executable { key, path })
    }
}

/// Looks the executable up (adding it on first sight) and returns its id. The path stored is the spelling first seen.
fn executable_id(
    tx: &Transaction<'_>,
    cache: &mut HashMap<String, i64>,
    exe: Option<&Executable>,
) -> Result<i64, RepoError> {
    let Some(exe) = exe else { return Ok(UNATTRIBUTED) };
    if let Some(id) = cache.get(&exe.key) {
        return Ok(*id);
    }
    tx.execute(
        "INSERT INTO executables (path_key, path) VALUES (?1, ?2) ON CONFLICT(path_key) DO NOTHING",
        params![exe.key, exe.path],
    )
    .map_err(backend)?;
    let id: i64 = tx
        .query_row("SELECT id FROM executables WHERE path_key = ?1", [&exe.key], |row| row.get(0))
        .map_err(backend)?;
    cache.insert(exe.key.clone(), id);
    Ok(id)
}

fn insert_rows(tx: &Transaction<'_>, rollup: &HourRollup, ids: &mut HashMap<String, i64>) -> Result<(), RepoError> {
    let hour = rollup.hour.as_micros();
    // Replace, never add: whatever an earlier attempt left for this hour goes first.
    tx.execute("DELETE FROM rollup_system_hour WHERE hour_start = ?1", [hour]).map_err(backend)?;
    tx.execute("DELETE FROM rollup_process_hour WHERE hour_start = ?1", [hour]).map_err(backend)?;
    tx.execute("DELETE FROM rollup_traffic_hour WHERE hour_start = ?1", [hour]).map_err(backend)?;

    if let Some(s) = &rollup.system {
        tx.execute(
            "INSERT INTO rollup_system_hour (hour_start, samples, cpu_avg, cpu_max, memory_used_avg, memory_used_max,
                 memory_total, disk_read, disk_write, net_received, net_sent, others_cpu_avg)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                hour,
                s.samples,
                s.cpu_avg,
                s.cpu_max,
                to_i64(s.memory_used_avg),
                to_i64(s.memory_used_max),
                to_i64(s.memory_total),
                to_i64(s.disk_read),
                to_i64(s.disk_write),
                to_i64(s.net_received),
                to_i64(s.net_sent),
                s.others_cpu_avg
            ],
        )
        .map_err(backend)?;
    }
    for p in &rollup.processes {
        let exe_id = executable_id(tx, ids, p.exe.as_ref())?;
        tx.execute(
            "INSERT INTO rollup_process_hour (hour_start, exe_id, samples_present, cpu_avg, cpu_peak, memory_peak, starts,
                 first_seen, last_seen)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                hour,
                exe_id,
                p.samples_present,
                p.cpu_avg,
                p.cpu_peak,
                to_i64(p.memory_peak),
                p.starts,
                p.first_seen.as_micros(),
                p.last_seen.as_micros()
            ],
        )
        .map_err(backend)?;
    }
    for t in &rollup.traffic {
        let exe_id = executable_id(tx, ids, t.exe.as_ref())?;
        tx.execute(
            "INSERT INTO rollup_traffic_hour (hour_start, exe_id, addr, bytes_sent, bytes_received, flows, first_seen,
                 last_seen, distinct_ports, top_port)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                hour,
                exe_id,
                t.addr.to_string(),
                to_i64(t.bytes_sent),
                to_i64(t.bytes_received),
                t.flows,
                t.first_seen.as_micros(),
                t.last_seen.as_micros(),
                t.distinct_ports,
                t.top_port
            ],
        )
        .map_err(backend)?;
    }
    Ok(())
}

/// Widens the permanent ledgers. MIN and MAX make this idempotent and independent of the order hours arrive in.
fn widen_ledgers(tx: &Transaction<'_>, rollup: &HourRollup, ids: &mut HashMap<String, i64>) -> Result<(), RepoError> {
    let observed = rollup.ledger_observations();
    for (exe, entry) in &observed.executables {
        let id = executable_id(tx, ids, Some(exe))?;
        tx.execute(
            "UPDATE executables SET first_seen = MIN(COALESCE(first_seen, ?2), ?2), last_seen = MAX(COALESCE(last_seen, ?3), ?3)
             WHERE id = ?1",
            params![id, entry.first_seen.as_micros(), entry.last_seen.as_micros()],
        )
        .map_err(backend)?;
    }
    for (addr, entry) in &observed.destinations {
        tx.execute(
            "INSERT INTO destinations (addr, first_seen, last_seen) VALUES (?1, ?2, ?3)
             ON CONFLICT(addr) DO UPDATE SET first_seen = MIN(first_seen, excluded.first_seen),
                                             last_seen = MAX(last_seen, excluded.last_seen)",
            params![addr.to_string(), entry.first_seen.as_micros(), entry.last_seen.as_micros()],
        )
        .map_err(backend)?;
    }
    for (exe, addr, entry) in &observed.pairs {
        let id = executable_id(tx, ids, Some(exe))?;
        tx.execute(
            "INSERT INTO exe_destinations (exe_id, addr, first_seen, last_seen) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(exe_id, addr) DO UPDATE SET first_seen = MIN(first_seen, excluded.first_seen),
                                                     last_seen = MAX(last_seen, excluded.last_seen)",
            params![id, addr.to_string(), entry.first_seen.as_micros(), entry.last_seen.as_micros()],
        )
        .map_err(backend)?;
    }
    Ok(())
}

fn read_progress(conn: &Connection) -> Result<Option<Timestamp>, RepoError> {
    let micros: Option<i64> = conn
        .query_row("SELECT hours_done_before FROM rollup_progress WHERE id = 1", [], |row| row.get(0))
        .optional()
        .map_err(backend)?;
    Ok(micros.map(Timestamp::from_micros))
}

impl RollupRepository for SqliteStore {
    fn rolled_up_before(&self) -> Result<Option<Timestamp>, RepoError> {
        read_progress(&self.conn)
    }

    fn earliest_detail(&self) -> Result<Option<Timestamp>, RepoError> {
        let oldest: Option<i64> = self
            .conn
            .query_row(
                "SELECT MIN(oldest) FROM (SELECT MIN(ts) AS oldest FROM events
                                          UNION ALL SELECT MIN(ts) FROM system_metrics
                                          UNION ALL SELECT MIN(ts) FROM process_metrics)",
                [],
                |row| row.get(0),
            )
            .map_err(backend)?;
        Ok(oldest.map(Timestamp::from_micros))
    }

    fn load_hour(&self, hour: Timestamp) -> Result<HourInput, RepoError> {
        let (from, to) = (hour.as_micros(), next_hour(hour).as_micros());
        let mut input = HourInput::empty(hour);

        let mut statement = self
            .conn
            .prepare(
                "SELECT ts, cpu_percent, memory_used, memory_total, disk_read, disk_write, net_received, net_sent,
                        others_count, others_cpu, others_memory
                 FROM system_metrics WHERE ts >= ?1 AND ts < ?2 ORDER BY ts",
            )
            .map_err(backend)?;
        let mut rows = statement.query(params![from, to]).map_err(backend)?;
        while let Some(row) = rows.next().map_err(backend)? {
            input.system.push(SystemPoint {
                ts: ts(row, 0)?,
                system: SystemMetrics {
                    cpu_percent: row.get(1).map_err(backend)?,
                    memory_used_bytes: to_u64(row.get(2).map_err(backend)?),
                    memory_total_bytes: to_u64(row.get(3).map_err(backend)?),
                    disk_read_bytes: to_u64(row.get(4).map_err(backend)?),
                    disk_write_bytes: to_u64(row.get(5).map_err(backend)?),
                    net_received_bytes: to_u64(row.get(6).map_err(backend)?),
                    net_sent_bytes: to_u64(row.get(7).map_err(backend)?),
                },
                others: OtherProcesses {
                    process_count: row.get(8).map_err(backend)?,
                    cpu_percent: row.get(9).map_err(backend)?,
                    memory_bytes: to_u64(row.get(10).map_err(backend)?),
                },
            });
        }

        // Owners are resolved from the process INSTANCE (the id), never from the PID.
        let mut statement = self
            .conn
            .prepare(
                "SELECT m.ts, p.image_path, m.cpu_percent, m.memory_bytes
                 FROM process_metrics m LEFT JOIN processes p ON p.id = m.instance
                 WHERE m.ts >= ?1 AND m.ts < ?2",
            )
            .map_err(backend)?;
        let mut rows = statement.query(params![from, to]).map_err(backend)?;
        while let Some(row) = rows.next().map_err(backend)? {
            input.process_samples.push(ProcessSampleRow {
                ts: ts(row, 0)?,
                exe_path: row.get(1).map_err(backend)?,
                cpu_percent: row.get(2).map_err(backend)?,
                memory_bytes: to_u64(row.get(3).map_err(backend)?),
            });
        }

        let mut statement = self
            .conn
            .prepare(
                "SELECT ts, json_extract(payload, '$.image_path') FROM events
                 WHERE category = 'process' AND kind = 'process_started' AND ts >= ?1 AND ts < ?2",
            )
            .map_err(backend)?;
        let mut rows = statement.query(params![from, to]).map_err(backend)?;
        while let Some(row) = rows.next().map_err(backend)? {
            input.starts.push(StartRow { ts: ts(row, 0)?, exe_path: row.get(1).map_err(backend)? });
        }

        let mut statement = self
            .conn
            .prepare(
                "SELECT f.window_start, p.image_path, f.remote_addr, f.remote_port, f.bytes_sent, f.bytes_received
                 FROM flows f LEFT JOIN processes p ON p.id = f.process_instance_id
                 WHERE f.window_start >= ?1 AND f.window_start < ?2",
            )
            .map_err(backend)?;
        let mut rows = statement.query(params![from, to]).map_err(backend)?;
        while let Some(row) = rows.next().map_err(backend)? {
            let addr: String = row.get(2).map_err(backend)?;
            input.flows.push(FlowInputRow {
                window_start: ts(row, 0)?,
                exe_path: row.get(1).map_err(backend)?,
                remote_addr: parse_addr(&addr)?,
                remote_port: row.get(3).map_err(backend)?,
                bytes_sent: to_u64(row.get(4).map_err(backend)?),
                bytes_received: to_u64(row.get(5).map_err(backend)?),
            });
        }
        Ok(input)
    }

    fn store_hour(&mut self, rollup: &HourRollup) -> Result<(), RepoError> {
        let end = next_hour(rollup.hour);
        let tx = self.conn.transaction().map_err(backend)?;
        if read_progress(&tx)?.is_some_and(|done| end <= done) {
            return Err(RepoError::Backend(format!(
                "the hour starting at {} is already rolled up",
                rollup.hour.as_micros()
            )));
        }
        let mut ids = HashMap::new();
        insert_rows(&tx, rollup, &mut ids)?;
        widen_ledgers(&tx, rollup, &mut ids)?;
        tx.execute(
            "INSERT INTO rollup_progress (id, hours_done_before) VALUES (1, ?1)
             ON CONFLICT(id) DO UPDATE SET hours_done_before = MAX(hours_done_before, excluded.hours_done_before)",
            [end.as_micros()],
        )
        .map_err(backend)?;
        tx.commit().map_err(backend)
    }

    fn system_hours(&self, from: Timestamp, to: Timestamp) -> Result<Vec<SystemHour>, RepoError> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT hour_start, samples, cpu_avg, cpu_max, memory_used_avg, memory_used_max, memory_total, disk_read,
                        disk_write, net_received, net_sent, others_cpu_avg
                 FROM rollup_system_hour WHERE hour_start >= ?1 AND hour_start <= ?2 ORDER BY hour_start",
            )
            .map_err(backend)?;
        let mut rows = statement.query(params![from.as_micros(), to.as_micros()]).map_err(backend)?;
        let mut hours = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            hours.push(SystemHour {
                hour: ts(row, 0)?,
                samples: row.get(1).map_err(backend)?,
                cpu_avg: row.get(2).map_err(backend)?,
                cpu_max: row.get(3).map_err(backend)?,
                memory_used_avg: to_u64(row.get(4).map_err(backend)?),
                memory_used_max: to_u64(row.get(5).map_err(backend)?),
                memory_total: to_u64(row.get(6).map_err(backend)?),
                disk_read: to_u64(row.get(7).map_err(backend)?),
                disk_write: to_u64(row.get(8).map_err(backend)?),
                net_received: to_u64(row.get(9).map_err(backend)?),
                net_sent: to_u64(row.get(10).map_err(backend)?),
                others_cpu_avg: row.get(11).map_err(backend)?,
            });
        }
        Ok(hours)
    }

    fn executable_hours(&self, path: &str, from: Timestamp, to: Timestamp) -> Result<Vec<ProcessHour>, RepoError> {
        let Some(key) = executable_key(path) else { return Ok(Vec::new()) };
        let mut statement = self
            .conn
            .prepare(
                "SELECT r.hour_start, e.id, e.path_key, e.path, r.samples_present, r.cpu_avg, r.cpu_peak, r.memory_peak,
                        r.starts, r.first_seen, r.last_seen
                 FROM rollup_process_hour r JOIN executables e ON e.id = r.exe_id
                 WHERE e.path_key = ?1 AND r.hour_start >= ?2 AND r.hour_start <= ?3
                 ORDER BY r.hour_start",
            )
            .map_err(backend)?;
        let mut rows = statement.query(params![key, from.as_micros(), to.as_micros()]).map_err(backend)?;
        let mut hours = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            hours.push(ProcessHour {
                hour: ts(row, 0)?,
                exe: executable_from(
                    row.get(1).map_err(backend)?,
                    row.get(2).map_err(backend)?,
                    row.get(3).map_err(backend)?,
                ),
                samples_present: row.get(4).map_err(backend)?,
                cpu_avg: row.get(5).map_err(backend)?,
                cpu_peak: row.get(6).map_err(backend)?,
                memory_peak: to_u64(row.get(7).map_err(backend)?),
                starts: row.get(8).map_err(backend)?,
                first_seen: ts(row, 9)?,
                last_seen: ts(row, 10)?,
            });
        }
        Ok(hours)
    }

    fn destination_hours(&self, addr: IpAddr, from: Timestamp, to: Timestamp) -> Result<Vec<TrafficHour>, RepoError> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT t.hour_start, e.id, e.path_key, e.path, t.bytes_sent, t.bytes_received, t.flows, t.first_seen,
                        t.last_seen, t.distinct_ports, t.top_port
                 FROM rollup_traffic_hour t JOIN executables e ON e.id = t.exe_id
                 WHERE t.addr = ?1 AND t.hour_start >= ?2 AND t.hour_start <= ?3
                 ORDER BY t.hour_start, e.path_key",
            )
            .map_err(backend)?;
        let mut rows = statement.query(params![addr.to_string(), from.as_micros(), to.as_micros()]).map_err(backend)?;
        let mut hours = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            hours.push(TrafficHour {
                hour: ts(row, 0)?,
                exe: executable_from(
                    row.get(1).map_err(backend)?,
                    row.get(2).map_err(backend)?,
                    row.get(3).map_err(backend)?,
                ),
                addr,
                bytes_sent: to_u64(row.get(4).map_err(backend)?),
                bytes_received: to_u64(row.get(5).map_err(backend)?),
                flows: row.get(6).map_err(backend)?,
                first_seen: ts(row, 7)?,
                last_seen: ts(row, 8)?,
                distinct_ports: row.get(9).map_err(backend)?,
                top_port: row.get(10).map_err(backend)?,
            });
        }
        Ok(hours)
    }

    fn executable_ledger(&self, path: &str) -> Result<Option<LedgerEntry>, RepoError> {
        let Some(key) = executable_key(path) else { return Ok(None) };
        let found: Option<(Option<i64>, Option<i64>)> = self
            .conn
            .query_row("SELECT first_seen, last_seen FROM executables WHERE path_key = ?1", [key], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .optional()
            .map_err(backend)?;
        Ok(found.and_then(|(first, last)| ledger_from(first, last)))
    }

    fn destination_ledger(&self, addr: IpAddr) -> Result<Option<LedgerEntry>, RepoError> {
        let found: Option<(i64, i64)> = self
            .conn
            .query_row("SELECT first_seen, last_seen FROM destinations WHERE addr = ?1", [addr.to_string()], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .optional()
            .map_err(backend)?;
        Ok(found.and_then(|(first, last)| ledger_from(Some(first), Some(last))))
    }

    fn pair_ledger(&self, path: &str, addr: IpAddr) -> Result<Option<LedgerEntry>, RepoError> {
        let Some(key) = executable_key(path) else { return Ok(None) };
        let found: Option<(i64, i64)> = self
            .conn
            .query_row(
                "SELECT d.first_seen, d.last_seen FROM exe_destinations d JOIN executables e ON e.id = d.exe_id
                 WHERE e.path_key = ?1 AND d.addr = ?2",
                params![key, addr.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        Ok(found.and_then(|(first, last)| ledger_from(Some(first), Some(last))))
    }
}
