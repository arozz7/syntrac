//! The permanent address-to-name table (schema v8): what `RollupRepository` stores and reads for DNS names. The choice of
//! which pairs to keep is `syntrac_core::dns_names`; this file only moves rows.

use std::net::IpAddr;

use rusqlite::{params, Connection, OptionalExtension, Transaction};
use syntrac_core::dns_names::MAX_NAMES_PER_ADDRESS;
use syntrac_core::{AddressName, DnsAnswerRow, RepoError, Timestamp};

use crate::connection::backend;

/// Whether the one-time backfill has been done.
pub(crate) fn backfilled(conn: &Connection) -> Result<bool, RepoError> {
    let found: Option<i64> = conn
        .query_row("SELECT id FROM names_progress WHERE id = 1", [], |row| row.get(0))
        .optional()
        .map_err(backend)?;
    Ok(found.is_some())
}

/// Records that the backfill is done.
pub(crate) fn mark_backfilled(tx: &Transaction<'_>) -> Result<(), RepoError> {
    tx.execute("INSERT OR IGNORE INTO names_progress (id) VALUES (1)", []).map_err(backend)?;
    Ok(())
}

/// Widens the stored pairs (MIN and MAX, so this is idempotent) and keeps at most [`MAX_NAMES_PER_ADDRESS`] names per touched
/// address: the most recently seen, a tie keeping the name that sorts first (the rule of `dns_names::cap_newest`).
pub(crate) fn store(tx: &Transaction<'_>, names: &[AddressName]) -> Result<(), RepoError> {
    let mut touched: Vec<String> = Vec::new();
    for row in names {
        let addr = row.addr.to_canonical().to_string();
        tx.execute(
            "INSERT INTO address_names (addr, name, first_seen, last_seen) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(addr, name) DO UPDATE SET first_seen = MIN(first_seen, excluded.first_seen),
                                                   last_seen = MAX(last_seen, excluded.last_seen)",
            params![addr, row.name, row.first_seen.as_micros(), row.last_seen.as_micros()],
        )
        .map_err(backend)?;
        if !touched.contains(&addr) {
            touched.push(addr);
        }
    }
    for addr in touched {
        tx.execute(
            "DELETE FROM address_names WHERE addr = ?1 AND name NOT IN
                 (SELECT name FROM address_names WHERE addr = ?1 ORDER BY last_seen DESC, name ASC LIMIT ?2)",
            params![addr, MAX_NAMES_PER_ADDRESS as i64],
        )
        .map_err(backend)?;
    }
    Ok(())
}

/// The names of one address, most recently seen first (a tie: by name).
pub(crate) fn read(conn: &Connection, addr: IpAddr) -> Result<Vec<AddressName>, RepoError> {
    let canonical = addr.to_canonical();
    let mut statement = conn
        .prepare(
            "SELECT name, first_seen, last_seen FROM address_names WHERE addr = ?1 ORDER BY last_seen DESC, name ASC",
        )
        .map_err(backend)?;
    let mut rows = statement.query([canonical.to_string()]).map_err(backend)?;
    let mut names = Vec::new();
    while let Some(row) = rows.next().map_err(backend)? {
        names.push(AddressName {
            addr: canonical,
            name: row.get(0).map_err(backend)?,
            first_seen: Timestamp::from_micros(row.get(1).map_err(backend)?),
            last_seen: Timestamp::from_micros(row.get(2).map_err(backend)?),
        });
    }
    Ok(names)
}

/// The DNS lookups with `from <= ts < to`, oldest first. An address that does not parse is skipped (nothing to link).
pub(crate) fn load_dns(conn: &Connection, from: Timestamp, to: Timestamp) -> Result<Vec<DnsAnswerRow>, RepoError> {
    let mut statement = conn
        .prepare(
            "SELECT ts, json_extract(payload, '$.query'), json_extract(payload, '$.addresses') FROM events
             WHERE category = 'dns' AND ts >= ?1 AND ts < ?2 ORDER BY ts",
        )
        .map_err(backend)?;
    let mut rows = statement.query(params![from.as_micros(), to.as_micros()]).map_err(backend)?;
    let mut answers = Vec::new();
    while let Some(row) = rows.next().map_err(backend)? {
        let query: Option<String> = row.get(1).map_err(backend)?;
        let addresses: Option<String> = row.get(2).map_err(backend)?;
        let Some(query) = query else { continue };
        let listed: Vec<String> = addresses.and_then(|json| serde_json::from_str(&json).ok()).unwrap_or_default();
        answers.push(DnsAnswerRow {
            ts: Timestamp::from_micros(row.get(0).map_err(backend)?),
            query,
            addresses: listed.iter().filter_map(|text| text.parse::<IpAddr>().ok()).collect(),
        });
    }
    Ok(answers)
}
