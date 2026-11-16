//! `OwnerRepository` for SQLite: the downloaded IP-to-owner dataset in `asn_ranges` (one row per range, 16-byte big-endian keys,
//! primary key on the start so a lookup is one seek) and its description in `owner_dataset`.
//!
//! The ranges are replaced as a whole. The new ones are written into a staging table in short transactions (so the collector's
//! writes are never held up for long), then swapped in with one quick transaction: a reader sees the old dataset or the new one,
//! never a mixture, and a failure at any point leaves the old one in place.

use std::net::IpAddr;

use rusqlite::{params, OptionalExtension};
use syntrac_core::owner_dataset::{addr_key, key_bytes, key_from_bytes};
use syntrac_core::{DatasetInfo, Owner, OwnerRange, OwnerRepository, RepoError, Timestamp};

use crate::codec::to_u64;
use crate::connection::{backend, SqliteStore};

/// Rows written per transaction while the new ranges are staged.
const STAGING_CHUNK: usize = 20_000;

const CREATE_STAGING: &str = "
    DROP TABLE IF EXISTS asn_ranges_staging;
    CREATE TABLE asn_ranges_staging (
        range_start BLOB NOT NULL PRIMARY KEY,
        range_end   BLOB NOT NULL,
        asn         INTEGER NOT NULL,
        country     TEXT NOT NULL,
        org         TEXT NOT NULL
    ) WITHOUT ROWID;";

fn lookup_sql() -> &'static str {
    "SELECT range_end, asn, country, org FROM asn_ranges WHERE range_start <= ?1 ORDER BY range_start DESC LIMIT 1"
}

impl SqliteStore {
    fn stage_ranges(&mut self, ranges: &[OwnerRange]) -> Result<(), RepoError> {
        self.conn.execute_batch(CREATE_STAGING).map_err(backend)?;
        for chunk in ranges.chunks(STAGING_CHUNK) {
            let tx = self.conn.transaction().map_err(backend)?;
            {
                let mut insert = tx
                    .prepare(
                        "INSERT INTO asn_ranges_staging (range_start, range_end, asn, country, org)
                         VALUES (?1, ?2, ?3, ?4, ?5)",
                    )
                    .map_err(backend)?;
                for range in chunk {
                    insert
                        .execute(params![
                            key_bytes(range.start).as_slice(),
                            key_bytes(range.end).as_slice(),
                            range.asn,
                            range.country,
                            range.org
                        ])
                        .map_err(backend)?;
                }
            }
            tx.commit().map_err(backend)?;
        }
        Ok(())
    }

    fn swap_in_staged_ranges(&mut self, info: &DatasetInfo) -> Result<(), RepoError> {
        let tx = self.conn.transaction().map_err(backend)?;
        tx.execute_batch("DROP TABLE asn_ranges; ALTER TABLE asn_ranges_staging RENAME TO asn_ranges;")
            .map_err(backend)?;
        tx.execute(
            "INSERT INTO owner_dataset (id, source, downloaded_at, rows, bad_lines, sha256) VALUES (1, ?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET source = ?1, downloaded_at = ?2, rows = ?3, bad_lines = ?4, sha256 = ?5",
            params![info.source, info.downloaded_at.as_micros(), info.rows, info.bad_lines, info.sha256],
        )
        .map_err(backend)?;
        tx.commit().map_err(backend)
    }
}

impl OwnerRepository for SqliteStore {
    fn dataset_info(&self) -> Result<Option<DatasetInfo>, RepoError> {
        self.conn
            .query_row(
                "SELECT source, downloaded_at, rows, bad_lines, sha256 FROM owner_dataset WHERE id = 1",
                [],
                |row| {
                    Ok(DatasetInfo {
                        source: row.get(0)?,
                        downloaded_at: Timestamp::from_micros(row.get(1)?),
                        rows: u32::try_from(to_u64(row.get(2)?)).unwrap_or(u32::MAX),
                        bad_lines: u32::try_from(to_u64(row.get(3)?)).unwrap_or(u32::MAX),
                        sha256: row.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(backend)
    }

    fn replace_dataset(&mut self, ranges: &[OwnerRange], info: &DatasetInfo) -> Result<(), RepoError> {
        let result = self.stage_ranges(ranges).and_then(|()| self.swap_in_staged_ranges(info));
        if result.is_err() {
            // The old dataset is untouched; do not leave a half-written staging table behind.
            let _ = self.conn.execute_batch("DROP TABLE IF EXISTS asn_ranges_staging;");
        }
        result
    }

    fn lookup_owner(&self, addr: IpAddr) -> Result<Option<Owner>, RepoError> {
        let key = addr_key(addr);
        let found = self
            .conn
            .query_row(lookup_sql(), params![key_bytes(key).as_slice()], |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, u32>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .optional()
            .map_err(backend)?;
        Ok(found.and_then(|(end, asn, country, org)| {
            let end = key_from_bytes(&end)?;
            (key <= end).then_some(Owner { asn, country, org })
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(rows: u32) -> DatasetInfo {
        DatasetInfo {
            source: "test".into(),
            downloaded_at: Timestamp::from_micros(1),
            rows,
            bad_lines: 0,
            sha256: "00".into(),
        }
    }

    #[test]
    fn a_lookup_is_one_seek_on_the_primary_key_not_a_scan() {
        let store = SqliteStore::open_in_memory().unwrap();
        let mut statement = store.conn.prepare(&format!("EXPLAIN QUERY PLAN {}", lookup_sql())).unwrap();
        let plan = statement
            .query_map([vec![0u8; 16]], |row| row.get::<_, String>(3))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .join(" | ");

        assert!(plan.contains("SEARCH asn_ranges USING PRIMARY KEY"), "{plan}");
    }

    #[test]
    fn a_failed_swap_leaves_the_old_dataset_and_no_staging_table() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        let old = syntrac_core::parse_ip2asn("1.0.0.0\t1.0.0.255\t1\tUS\tOld\n").ranges;
        store.replace_dataset(&old, &info(1)).unwrap();
        // Make the swap fail: the description row can no longer be written.
        store.conn.execute_batch("DROP TABLE owner_dataset; CREATE TABLE owner_dataset (id INTEGER);").unwrap();

        let new = syntrac_core::parse_ip2asn("8.8.8.0\t8.8.8.255\t2\tUS\tNew\n").ranges;
        assert!(store.replace_dataset(&new, &info(1)).is_err());

        assert_eq!(store.lookup_owner("1.0.0.7".parse().unwrap()).unwrap().map(|o| o.asn), Some(1));
        assert_eq!(store.lookup_owner("8.8.8.8".parse().unwrap()).unwrap(), None);
        let staging: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM sqlite_master WHERE name = 'asn_ranges_staging'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(staging, 0);
    }
}
