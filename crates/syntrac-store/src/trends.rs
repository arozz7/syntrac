//! `TrendRepository` for SQLite: one group-by per table, so a year of hourly rows is summed inside the database and only one row
//! per program comes back. Merging the sums and ranking them is `syntrac_core::trend`.

use rusqlite::{params, Row};
use syntrac_core::{Executable, ProcessTotals, RepoError, Timestamp, TrafficTotals, TrendRepository};

use crate::codec::to_u64;
use crate::connection::{backend, SqliteStore};

/// The `executables` row that stands for "owner or path unknown".
const UNATTRIBUTED: i64 = 1;

fn executable(row: &Row<'_>) -> Result<Option<Executable>, RepoError> {
    let id: i64 = row.get(0).map_err(backend)?;
    if id == UNATTRIBUTED {
        return Ok(None);
    }
    Ok(Some(Executable { key: row.get(1).map_err(backend)?, path: row.get(2).map_err(backend)? }))
}

impl TrendRepository for SqliteStore {
    fn system_sample_total(&self, from: Timestamp, to: Timestamp) -> Result<u64, RepoError> {
        let total: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(SUM(samples), 0) FROM rollup_system_hour WHERE hour_start >= ?1 AND hour_start <= ?2",
                params![from.as_micros(), to.as_micros()],
                |row| row.get(0),
            )
            .map_err(backend)?;
        Ok(to_u64(total))
    }

    fn process_totals(&self, from: Timestamp, to: Timestamp) -> Result<Vec<ProcessTotals>, RepoError> {
        // The join keeps only hours that have a system row: its samples are the weights of the average.
        let mut statement = self
            .conn
            .prepare(
                "SELECT e.id, e.path_key, e.path, COUNT(*), SUM(p.samples_present), SUM(p.cpu_avg * s.samples),
                        MAX(p.cpu_peak), MAX(p.memory_peak), SUM(p.starts), MAX(p.gpu_peak), MAX(p.gpu_memory_peak)
                 FROM rollup_process_hour p
                 JOIN rollup_system_hour s ON s.hour_start = p.hour_start
                 JOIN executables e ON e.id = p.exe_id
                 WHERE p.hour_start >= ?1 AND p.hour_start <= ?2
                 GROUP BY e.id",
            )
            .map_err(backend)?;
        let mut rows = statement.query(params![from.as_micros(), to.as_micros()]).map_err(backend)?;
        let mut totals = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            totals.push(ProcessTotals {
                exe: executable(row)?,
                hours_present: row.get(3).map_err(backend)?,
                samples_present: to_u64(row.get(4).map_err(backend)?),
                cpu_weighted: row.get(5).map_err(backend)?,
                cpu_peak: row.get(6).map_err(backend)?,
                memory_peak: to_u64(row.get(7).map_err(backend)?),
                starts: to_u64(row.get(8).map_err(backend)?),
                gpu_peak: row.get(9).map_err(backend)?,
                gpu_memory_peak: to_u64(row.get(10).map_err(backend)?),
            });
        }
        Ok(totals)
    }

    fn traffic_totals(&self, from: Timestamp, to: Timestamp) -> Result<Vec<TrafficTotals>, RepoError> {
        // Loopback is the whole 127.0.0.0/8 block and ::1, the same set `IpAddr::is_loopback` recognises.
        let mut statement = self
            .conn
            .prepare(
                "SELECT e.id, e.path_key, e.path, SUM(t.bytes_sent), SUM(t.bytes_received), SUM(t.flows),
                        COUNT(DISTINCT t.addr)
                 FROM rollup_traffic_hour t
                 JOIN executables e ON e.id = t.exe_id
                 WHERE t.hour_start >= ?1 AND t.hour_start <= ?2 AND t.addr NOT LIKE '127.%' AND t.addr <> '::1'
                 GROUP BY e.id",
            )
            .map_err(backend)?;
        let mut rows = statement.query(params![from.as_micros(), to.as_micros()]).map_err(backend)?;
        let mut totals = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            totals.push(TrafficTotals {
                exe: executable(row)?,
                bytes_sent: to_u64(row.get(3).map_err(backend)?),
                bytes_received: to_u64(row.get(4).map_err(backend)?),
                flows: to_u64(row.get(5).map_err(backend)?),
                destinations: row.get(6).map_err(backend)?,
            });
        }
        Ok(totals)
    }
}
