//! `DestinationTrendRepository` for SQLite: one group-by per question, so a year of hourly rows is summed inside the database
//! and only one row per address comes back. Ranking and the "new" rule are `syntrac_core::destination_trend`.

use std::net::IpAddr;

use rusqlite::{params, Row};
use syntrac_core::destination_trend::{DestinationProgram, DestinationTotals, NewDestination};
use syntrac_core::{DestinationTrendRepository, Executable, RepoError, Timestamp};

use crate::codec::to_u64;
use crate::connection::{backend, SqliteStore};

/// The `executables` row that stands for "owner or path unknown".
const UNATTRIBUTED: i64 = 1;

/// Loopback is the whole 127.0.0.0/8 block and ::1, the same set `IpAddr::is_loopback` recognises.
const NOT_LOOPBACK: &str = "addr NOT LIKE '127.%' AND addr <> '::1'";

fn parse_addr(text: &str) -> Result<IpAddr, RepoError> {
    text.parse::<IpAddr>().map_err(backend)
}

fn ts(row: &Row<'_>, index: usize) -> Result<Timestamp, RepoError> {
    Ok(Timestamp::from_micros(row.get(index).map_err(backend)?))
}

fn destination_totals_sql() -> String {
    format!(
        "SELECT addr, SUM(bytes_sent), SUM(bytes_received), SUM(flows), COUNT(DISTINCT exe_id)
         FROM rollup_traffic_hour
         WHERE hour_start >= ?1 AND hour_start <= ?2 AND {NOT_LOOPBACK}
         GROUP BY addr"
    )
}

/// The first program is the pair with the earliest first sighting (a tie: the path that sorts first), found through the
/// `(addr, first_seen)` index of `exe_destinations`. Only attributed programs have pairs.
fn new_destinations_sql() -> String {
    format!(
        "SELECT d.addr, d.first_seen, d.last_seen, e.path_key, e.path
         FROM destinations d
         LEFT JOIN executables e ON e.id = (
             SELECT x.exe_id FROM exe_destinations x JOIN executables y ON y.id = x.exe_id
             WHERE x.addr = d.addr ORDER BY x.first_seen, y.path_key LIMIT 1)
         WHERE d.first_seen >= ?1 AND d.{NOT_LOOPBACK}"
    )
}

impl DestinationTrendRepository for SqliteStore {
    fn destination_totals(&self, from: Timestamp, to: Timestamp) -> Result<Vec<DestinationTotals>, RepoError> {
        let mut statement = self.conn.prepare(&destination_totals_sql()).map_err(backend)?;
        let mut rows = statement.query(params![from.as_micros(), to.as_micros()]).map_err(backend)?;
        let mut totals = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let addr: String = row.get(0).map_err(backend)?;
            totals.push(DestinationTotals {
                addr: parse_addr(&addr)?,
                bytes_sent: to_u64(row.get(1).map_err(backend)?),
                bytes_received: to_u64(row.get(2).map_err(backend)?),
                flows: to_u64(row.get(3).map_err(backend)?),
                programs: row.get(4).map_err(backend)?,
            });
        }
        Ok(totals)
    }

    fn new_destinations(&self, since: Timestamp) -> Result<Vec<NewDestination>, RepoError> {
        let mut statement = self.conn.prepare(&new_destinations_sql()).map_err(backend)?;
        let mut rows = statement.query(params![since.as_micros()]).map_err(backend)?;
        let mut found = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let addr: String = row.get(0).map_err(backend)?;
            let key: Option<String> = row.get(3).map_err(backend)?;
            let path: Option<String> = row.get(4).map_err(backend)?;
            found.push(NewDestination {
                addr: parse_addr(&addr)?,
                first_seen: ts(row, 1)?,
                last_seen: ts(row, 2)?,
                first_program: key.zip(path).map(|(key, path)| Executable { key, path }),
            });
        }
        Ok(found)
    }

    fn destination_programs(
        &self,
        addr: IpAddr,
        from: Timestamp,
        to: Timestamp,
    ) -> Result<Vec<DestinationProgram>, RepoError> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT e.id, e.path_key, e.path, SUM(t.bytes_sent), SUM(t.bytes_received), SUM(t.flows),
                        MIN(t.first_seen), MAX(t.last_seen)
                 FROM rollup_traffic_hour t JOIN executables e ON e.id = t.exe_id
                 WHERE t.addr = ?1 AND t.hour_start >= ?2 AND t.hour_start <= ?3
                 GROUP BY e.id",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![addr.to_canonical().to_string(), from.as_micros(), to.as_micros()])
            .map_err(backend)?;
        let mut programs = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let id: i64 = row.get(0).map_err(backend)?;
            let exe = if id == UNATTRIBUTED {
                None
            } else {
                Some(Executable { key: row.get(1).map_err(backend)?, path: row.get(2).map_err(backend)? })
            };
            programs.push(DestinationProgram {
                exe,
                bytes_sent: to_u64(row.get(3).map_err(backend)?),
                bytes_received: to_u64(row.get(4).map_err(backend)?),
                flows: to_u64(row.get(5).map_err(backend)?),
                first_seen: ts(row, 6)?,
                last_seen: ts(row, 7)?,
            });
        }
        Ok(programs)
    }
}

#[cfg(test)]
mod plan_tests {
    use super::*;

    fn plan(store: &SqliteStore, sql: &str, parameters: usize) -> String {
        let mut statement = store.conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        let bound: Vec<i64> = vec![0; parameters];
        let rows = statement
            .query_map(rusqlite::params_from_iter(bound), |row| row.get::<_, String>(3))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        rows.join(" | ")
    }

    #[test]
    fn the_first_program_of_a_destination_is_found_through_the_addr_index_not_by_scanning_pairs() {
        let store = SqliteStore::open_in_memory().unwrap();

        let plan = plan(&store, &new_destinations_sql(), 1);

        assert!(plan.contains("idx_exe_destinations_addr"), "{plan}");
    }

    #[test]
    fn the_totals_of_a_range_read_only_that_range_of_hours() {
        let store = SqliteStore::open_in_memory().unwrap();

        let plan = plan(&store, &destination_totals_sql(), 2);

        assert!(plan.contains("SEARCH rollup_traffic_hour USING PRIMARY KEY"), "{plan}");
    }
}
