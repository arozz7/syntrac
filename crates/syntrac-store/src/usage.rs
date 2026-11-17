//! `UsageRepository` for SQLite: the hourly per-program rows the usage rules read (docs/ALERTS-PLAN.md, F3). Two reads over a bounded
//! range of hours (about two weeks), so only a few thousand small rows come back.

use syntrac_core::{Executable, ProcessHourRow, RepoError, Timestamp, TrafficHourRow, UsageRepository};

use crate::codec::to_u64;
use crate::connection::{backend, SqliteStore};

/// Loopback is the whole 127.0.0.0/8 block and ::1, as everywhere else.
const NOT_LOOPBACK: &str = "t.addr NOT LIKE '127.%' AND t.addr <> '::1'";

impl UsageRepository for SqliteStore {
    fn process_hour_rows(&self, from: Timestamp, to: Timestamp) -> Result<Vec<ProcessHourRow>, RepoError> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT r.hour_start, e.path_key, e.path, r.cpu_avg, r.memory_peak
                 FROM rollup_process_hour r JOIN executables e ON e.id = r.exe_id
                 WHERE e.id <> 1 AND r.hour_start >= ?1 AND r.hour_start <= ?2",
            )
            .map_err(backend)?;
        let mut rows = statement.query([from.as_micros(), to.as_micros()]).map_err(backend)?;
        let mut found = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            found.push(ProcessHourRow {
                hour: Timestamp::from_micros(row.get(0).map_err(backend)?),
                exe: Executable { key: row.get(1).map_err(backend)?, path: row.get(2).map_err(backend)? },
                cpu_avg: row.get(3).map_err(backend)?,
                memory_peak: to_u64(row.get(4).map_err(backend)?),
            });
        }
        Ok(found)
    }

    fn traffic_hour_rows(&self, from: Timestamp, to: Timestamp) -> Result<Vec<TrafficHourRow>, RepoError> {
        let mut statement = self
            .conn
            .prepare(&format!(
                "SELECT t.hour_start, e.path_key, e.path, SUM(t.bytes_sent), SUM(t.bytes_received)
                 FROM rollup_traffic_hour t JOIN executables e ON e.id = t.exe_id
                 WHERE e.id <> 1 AND t.hour_start >= ?1 AND t.hour_start <= ?2 AND {NOT_LOOPBACK}
                 GROUP BY t.hour_start, t.exe_id"
            ))
            .map_err(backend)?;
        let mut rows = statement.query([from.as_micros(), to.as_micros()]).map_err(backend)?;
        let mut found = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            found.push(TrafficHourRow {
                hour: Timestamp::from_micros(row.get(0).map_err(backend)?),
                exe: Executable { key: row.get(1).map_err(backend)?, path: row.get(2).map_err(backend)? },
                bytes_sent: to_u64(row.get(3).map_err(backend)?),
                bytes_received: to_u64(row.get(4).map_err(backend)?),
            });
        }
        Ok(found)
    }
}
