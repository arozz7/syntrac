//! Findings, expectations and the detection state for SQLite (docs/ALERTS-PLAN.md, F1). Permanent: the purge of detail never touches
//! them. A write is one transaction, so a failed batch changes nothing.

use rusqlite::{params, OptionalExtension, Row};
use syntrac_core::findings::{Expectation, Finding, FindingStatus, FindingWrite, NewExpectation, Tier};
use syntrac_core::{
    DetectionState, DetectionStateRepository, ExpectationRepository, FindingRepository, RepoError, Timestamp,
};

use crate::codec::to_u64;
use crate::connection::{backend, SqliteStore};

const FINDING_COLUMNS: &str =
    "id, kind, subject, subject_label, tier, title, why, evidence, dedupe_key, status, first_at, last_at, \
                               hits, decided_at, snoozed_until, level";

fn stored(what: &str, text: &str) -> RepoError {
    RepoError::Backend(format!("a stored finding has an unknown {what}: {text}"))
}

fn ts(row: &Row<'_>, index: usize) -> Result<Timestamp, RepoError> {
    Ok(Timestamp::from_micros(row.get(index).map_err(backend)?))
}

fn optional_ts(row: &Row<'_>, index: usize) -> Result<Option<Timestamp>, RepoError> {
    Ok(row.get::<_, Option<i64>>(index).map_err(backend)?.map(Timestamp::from_micros))
}

fn finding_from(row: &Row<'_>) -> Result<Finding, RepoError> {
    let tier: String = row.get(4).map_err(backend)?;
    let status: String = row.get(9).map_err(backend)?;
    let evidence: String = row.get(7).map_err(backend)?;
    Ok(Finding {
        id: row.get(0).map_err(backend)?,
        kind: row.get(1).map_err(backend)?,
        subject: row.get(2).map_err(backend)?,
        subject_label: row.get(3).map_err(backend)?,
        tier: Tier::parse(&tier).ok_or_else(|| stored("tier", &tier))?,
        title: row.get(5).map_err(backend)?,
        why: row.get(6).map_err(backend)?,
        evidence: evidence.lines().map(str::to_owned).collect(),
        dedupe_key: row.get(8).map_err(backend)?,
        level: row.get(15).map_err(backend)?,
        status: FindingStatus::parse(&status).ok_or_else(|| stored("status", &status))?,
        first_at: ts(row, 10)?,
        last_at: ts(row, 11)?,
        hits: u32::try_from(to_u64(row.get(12).map_err(backend)?)).unwrap_or(u32::MAX),
        decided_at: optional_ts(row, 13)?,
        snoozed_until: optional_ts(row, 14)?,
    })
}

fn collect(statement: &mut rusqlite::Statement<'_>, values: impl rusqlite::Params) -> Result<Vec<Finding>, RepoError> {
    let mut rows = statement.query(values).map_err(backend)?;
    let mut found = Vec::new();
    while let Some(row) = rows.next().map_err(backend)? {
        found.push(finding_from(row)?);
    }
    Ok(found)
}

/// SQLite allows 999 variables per statement (older builds).
const MAX_KEYS_PER_READ: usize = 500;

impl FindingRepository for SqliteStore {
    fn findings(&self, status: Option<FindingStatus>, limit: usize) -> Result<Vec<Finding>, RepoError> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        match status {
            Some(status) => {
                let mut statement = self
                    .conn
                    .prepare(&format!(
                        "SELECT {FINDING_COLUMNS} FROM findings WHERE status = ?1 ORDER BY last_at DESC, id DESC LIMIT ?2"
                    ))
                    .map_err(backend)?;
                collect(&mut statement, params![status.as_str(), limit])
            }
            None => {
                let mut statement = self
                    .conn
                    .prepare(&format!("SELECT {FINDING_COLUMNS} FROM findings ORDER BY last_at DESC, id DESC LIMIT ?1"))
                    .map_err(backend)?;
                collect(&mut statement, params![limit])
            }
        }
    }

    fn finding(&self, id: i64) -> Result<Option<Finding>, RepoError> {
        let mut statement =
            self.conn.prepare(&format!("SELECT {FINDING_COLUMNS} FROM findings WHERE id = ?1")).map_err(backend)?;
        Ok(collect(&mut statement, params![id])?.into_iter().next())
    }

    fn findings_for_keys(&self, keys: &[String]) -> Result<Vec<Finding>, RepoError> {
        let mut found = Vec::new();
        for chunk in keys.chunks(MAX_KEYS_PER_READ) {
            let marks = vec!["?"; chunk.len()].join(",");
            let mut statement = self
                .conn
                .prepare(&format!("SELECT {FINDING_COLUMNS} FROM findings WHERE dedupe_key IN ({marks})"))
                .map_err(backend)?;
            found.extend(collect(&mut statement, rusqlite::params_from_iter(chunk.iter()))?);
        }
        Ok(found)
    }

    fn apply_writes(&mut self, writes: &[FindingWrite]) -> Result<(), RepoError> {
        let tx = self.conn.transaction().map_err(backend)?;
        for write in writes {
            match write {
                FindingWrite::Insert(candidate) => {
                    tx.execute(
                        "INSERT INTO findings (kind, subject, subject_label, tier, title, why, evidence, dedupe_key, status,
                                               first_at, last_at, hits, level)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'open', ?9, ?9, 1, ?10)",
                        params![
                            candidate.kind,
                            candidate.subject,
                            candidate.subject_label,
                            candidate.tier.as_str(),
                            candidate.title,
                            candidate.why,
                            candidate.evidence.join("\n"),
                            candidate.dedupe_key,
                            candidate.at.as_micros(),
                            candidate.level
                        ],
                    )
                    .map_err(backend)?;
                }
                FindingWrite::Touch { id, at } => {
                    tx.execute(
                        "UPDATE findings SET hits = hits + 1, last_at = MAX(last_at, ?2) WHERE id = ?1",
                        params![id, at.as_micros()],
                    )
                    .map_err(backend)?;
                }
                FindingWrite::Reopen { id, at } => {
                    tx.execute(
                        "UPDATE findings SET hits = hits + 1, last_at = MAX(last_at, ?2), status = 'open', snoozed_until = NULL
                         WHERE id = ?1",
                        params![id, at.as_micros()],
                    )
                    .map_err(backend)?;
                }
            }
        }
        tx.commit().map_err(backend)
    }

    fn set_status(
        &mut self,
        id: i64,
        status: FindingStatus,
        decided_at: Timestamp,
        snoozed_until: Option<Timestamp>,
    ) -> Result<bool, RepoError> {
        let changed = self
            .conn
            .execute(
                "UPDATE findings SET status = ?2, decided_at = ?3, snoozed_until = ?4 WHERE id = ?1",
                params![id, status.as_str(), decided_at.as_micros(), snoozed_until.map(Timestamp::as_micros)],
            )
            .map_err(backend)?;
        Ok(changed > 0)
    }

    fn status_counts(&self) -> Result<Vec<(FindingStatus, u32)>, RepoError> {
        let mut statement =
            self.conn.prepare("SELECT status, COUNT(*) FROM findings GROUP BY status").map_err(backend)?;
        let mut rows = statement.query([]).map_err(backend)?;
        let mut counts = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let status: String = row.get(0).map_err(backend)?;
            let count: i64 = row.get(1).map_err(backend)?;
            counts.push((
                FindingStatus::parse(&status).ok_or_else(|| stored("status", &status))?,
                u32::try_from(count).unwrap_or(u32::MAX),
            ));
        }
        Ok(counts)
    }
}

impl ExpectationRepository for SqliteStore {
    fn expectations(&self) -> Result<Vec<Expectation>, RepoError> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT id, kind, subject, note, created_at, source_finding, ceiling FROM expectations ORDER BY id",
            )
            .map_err(backend)?;
        let mut rows = statement.query([]).map_err(backend)?;
        let mut list = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            list.push(Expectation {
                id: row.get(0).map_err(backend)?,
                kind: row.get(1).map_err(backend)?,
                subject: row.get(2).map_err(backend)?,
                note: row.get(3).map_err(backend)?,
                created_at: ts(row, 4)?,
                source_finding: row.get(5).map_err(backend)?,
                ceiling: row.get(6).map_err(backend)?,
            });
        }
        Ok(list)
    }

    fn add_expectation(&mut self, expectation: &NewExpectation, at: Timestamp) -> Result<i64, RepoError> {
        self.conn
            .execute(
                "INSERT INTO expectations (kind, subject, note, created_at, source_finding, ceiling)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (kind, subject) DO UPDATE SET note = excluded.note, ceiling = excluded.ceiling",
                params![
                    expectation.kind,
                    expectation.subject,
                    expectation.note,
                    at.as_micros(),
                    expectation.source_finding,
                    expectation.ceiling
                ],
            )
            .map_err(backend)?;
        self.conn
            .query_row(
                "SELECT id FROM expectations WHERE kind = ?1 AND subject = ?2",
                params![expectation.kind, expectation.subject],
                |row| row.get(0),
            )
            .map_err(backend)
    }

    fn remove_expectation(&mut self, id: i64) -> Result<bool, RepoError> {
        let removed = self.conn.execute("DELETE FROM expectations WHERE id = ?1", [id]).map_err(backend)?;
        Ok(removed > 0)
    }
}

impl DetectionStateRepository for SqliteStore {
    fn detection_state(&self) -> Result<Option<DetectionState>, RepoError> {
        self.conn
            .query_row("SELECT switched_on_at, last_run_at, last_error FROM detect_state WHERE id = 1", [], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?, row.get::<_, Option<String>>(2)?))
            })
            .optional()
            .map_err(backend)
            .map(|found| {
                found.map(|(on, run, error)| DetectionState {
                    switched_on_at: Timestamp::from_micros(on),
                    last_run_at: run.map(Timestamp::from_micros),
                    last_error: error,
                })
            })
    }

    fn switch_on(&mut self, now: Timestamp) -> Result<Timestamp, RepoError> {
        self.conn
            .execute("INSERT OR IGNORE INTO detect_state (id, switched_on_at) VALUES (1, ?1)", [now.as_micros()])
            .map_err(backend)?;
        let on: i64 = self
            .conn
            .query_row("SELECT switched_on_at FROM detect_state WHERE id = 1", [], |row| row.get(0))
            .map_err(backend)?;
        Ok(Timestamp::from_micros(on))
    }

    fn record_run(&mut self, at: Timestamp, error: Option<&str>) -> Result<(), RepoError> {
        self.conn
            .execute("INSERT OR IGNORE INTO detect_state (id, switched_on_at) VALUES (1, ?1)", [at.as_micros()])
            .map_err(backend)?;
        self.conn
            .execute(
                "UPDATE detect_state SET last_run_at = ?1, last_error = ?2 WHERE id = 1",
                params![at.as_micros(), error],
            )
            .map_err(backend)?;
        Ok(())
    }
}
