//! `IdentityRepository` for SQLite: one row per executable in `executable_identity`, keyed by the same id as the rollups.
//! The rows are permanent: retention and the purge of detail never touch them.

use rusqlite::{params, OptionalExtension, Row};
use syntrac_core::rollup::executable_key;
use syntrac_core::{
    ChangeFacts, Executable, FileFacts, FileStamp, IdentityChange, IdentityChangeRepository, IdentityEntry,
    IdentityFailure, IdentityRecord, IdentityRepository, InspectErrorKind, LedgerEntry, RepoError, Signature,
    SignatureKind, SignatureStatus, Timestamp, VersionInfo,
};

use crate::codec::{to_i64, to_u64};
use crate::connection::{backend, SqliteStore};

/// Every executable but the unattributed row, with its ledger and (if it has one) its identity. The identity columns start
/// at index 5.
const ENTRIES: &str = "
    SELECT e.path_key, e.path, e.first_seen, e.last_seen, i.exe_id,
           i.last_attempt, i.inspected_at, i.failure_kind, i.failure_message, i.failure_attempts,
           i.size, i.modified, i.sha256, i.signature_status, i.signer, i.thumbprint, i.signature_kind,
           i.company, i.product, i.description, i.original_filename, i.file_version, i.zone_id
    FROM executables e LEFT JOIN executable_identity i ON i.exe_id = e.id
    WHERE e.id <> 1";

fn timestamp(value: Option<i64>) -> Option<Timestamp> {
    value.map(Timestamp::from_micros)
}

fn decode_error(what: &str, text: &str) -> RepoError {
    RepoError::Backend(format!("unknown {what} '{text}' in executable_identity"))
}

fn facts_from(row: &Row<'_>) -> Result<Option<FileFacts>, RepoError> {
    let size: Option<i64> = row.get(10).map_err(backend)?;
    let Some(size) = size else { return Ok(None) };
    let status: String = row.get(13).map_err(backend)?;
    let kind: Option<String> = row.get(16).map_err(backend)?;
    let kind = kind
        .map(|text| SignatureKind::parse(&text).ok_or_else(|| decode_error("signature kind", &text)))
        .transpose()?;
    let text = |index| row.get::<_, Option<String>>(index).map_err(backend);
    let zone: Option<i64> = row.get(22).map_err(backend)?;
    Ok(Some(FileFacts {
        stamp: FileStamp { size: to_u64(size), modified: timestamp(row.get(11).map_err(backend)?) },
        sha256: text(12)?,
        signature: Signature {
            status: SignatureStatus::parse(&status).ok_or_else(|| decode_error("signature status", &status))?,
            signer: text(14)?,
            thumbprint: text(15)?,
            kind,
        },
        version: VersionInfo {
            company: text(17)?,
            product: text(18)?,
            description: text(19)?,
            original_filename: text(20)?,
            file_version: text(21)?,
        },
        zone_id: zone.map(|z| u32::try_from(z).unwrap_or(u32::MAX)),
    }))
}

fn failure_from(row: &Row<'_>) -> Result<Option<IdentityFailure>, RepoError> {
    let kind: Option<String> = row.get(7).map_err(backend)?;
    let Some(kind) = kind else { return Ok(None) };
    let attempts: i64 = row.get(9).map_err(backend)?;
    Ok(Some(IdentityFailure {
        kind: InspectErrorKind::parse(&kind).ok_or_else(|| decode_error("failure kind", &kind))?,
        message: row.get(8).map_err(backend)?,
        attempts: u32::try_from(attempts).unwrap_or(u32::MAX),
    }))
}

fn entry_from(row: &Row<'_>) -> Result<IdentityEntry, RepoError> {
    let exe = Executable { key: row.get(0).map_err(backend)?, path: row.get(1).map_err(backend)? };
    let first: Option<i64> = row.get(2).map_err(backend)?;
    let last: Option<i64> = row.get(3).map_err(backend)?;
    let seen = first.zip(last).map(|(first, last)| LedgerEntry {
        first_seen: Timestamp::from_micros(first),
        last_seen: Timestamp::from_micros(last),
    });
    let has_identity: Option<i64> = row.get(4).map_err(backend)?;
    let record = if has_identity.is_some() {
        Some(IdentityRecord {
            exe: exe.clone(),
            facts: facts_from(row)?,
            inspected_at: timestamp(row.get(6).map_err(backend)?),
            last_attempt: Timestamp::from_micros(row.get(5).map_err(backend)?),
            failure: failure_from(row)?,
        })
    } else {
        None
    };
    Ok(IdentityEntry { exe, seen, record })
}

impl IdentityRepository for SqliteStore {
    fn identity_entries(&self) -> Result<Vec<IdentityEntry>, RepoError> {
        let mut statement = self.conn.prepare(&format!("{ENTRIES} ORDER BY e.path_key")).map_err(backend)?;
        let mut rows = statement.query([]).map_err(backend)?;
        let mut entries = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            entries.push(entry_from(row)?);
        }
        Ok(entries)
    }

    fn save_identity(&mut self, record: &IdentityRecord) -> Result<(), RepoError> {
        let exe_id: Option<i64> = self
            .conn
            .query_row("SELECT id FROM executables WHERE path_key = ?1 AND id <> 1", [&record.exe.key], |row| {
                row.get(0)
            })
            .optional()
            .map_err(backend)?;
        let exe_id = exe_id.ok_or(RepoError::NotFound)?;

        let facts = record.facts.as_ref();
        let failure = record.failure.as_ref();
        // New CONTENT is a change worth remembering: both readings must have been hashed (docs/ALERTS-PLAN.md, F2).
        let old: Option<(Option<String>, Option<String>, Option<String>)> = self
            .conn
            .query_row(
                "SELECT sha256, signer, signature_status FROM executable_identity WHERE exe_id = ?1",
                [exe_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(backend)?;
        if let (Some((Some(old_sha), old_signer, old_status)), Some(new)) = (old, facts) {
            if new.sha256.as_deref().is_some_and(|new_sha| new_sha != old_sha) {
                self.conn
                    .execute(
                        "INSERT INTO identity_changes
                            (exe_id, old_sha256, old_signer, old_status, new_sha256, new_signer, new_status, changed_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                        params![
                            exe_id,
                            old_sha,
                            old_signer,
                            old_status,
                            new.sha256,
                            new.signature.signer,
                            new.signature.status.as_str(),
                            record.inspected_at.unwrap_or(record.last_attempt).as_micros()
                        ],
                    )
                    .map_err(backend)?;
            }
        }
        self.conn
            .execute(
                "INSERT OR REPLACE INTO executable_identity
                    (exe_id, last_attempt, inspected_at, failure_kind, failure_message, failure_attempts,
                     size, modified, sha256, signature_status, signer, thumbprint, signature_kind,
                     company, product, description, original_filename, file_version, zone_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
                params![
                    exe_id,
                    record.last_attempt.as_micros(),
                    record.inspected_at.map(Timestamp::as_micros),
                    failure.map(|f| f.kind.as_str()),
                    failure.map(|f| f.message.as_str()),
                    failure.map_or(0, |f| i64::from(f.attempts)),
                    facts.map(|f| to_i64(f.stamp.size)),
                    facts.and_then(|f| f.stamp.modified).map(Timestamp::as_micros),
                    facts.and_then(|f| f.sha256.as_deref()),
                    facts.map(|f| f.signature.status.as_str()),
                    facts.and_then(|f| f.signature.signer.as_deref()),
                    facts.and_then(|f| f.signature.thumbprint.as_deref()),
                    facts.and_then(|f| f.signature.kind).map(SignatureKind::as_str),
                    facts.and_then(|f| f.version.company.as_deref()),
                    facts.and_then(|f| f.version.product.as_deref()),
                    facts.and_then(|f| f.version.description.as_deref()),
                    facts.and_then(|f| f.version.original_filename.as_deref()),
                    facts.and_then(|f| f.version.file_version.as_deref()),
                    facts.and_then(|f| f.zone_id).map(i64::from),
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn identity(&self, path: &str) -> Result<Option<IdentityRecord>, RepoError> {
        let Some(key) = executable_key(path) else { return Ok(None) };
        let mut statement = self.conn.prepare(&format!("{ENTRIES} AND e.path_key = ?1")).map_err(backend)?;
        let mut rows = statement.query([key]).map_err(backend)?;
        match rows.next().map_err(backend)? {
            Some(row) => Ok(entry_from(row)?.record),
            None => Ok(None),
        }
    }
}

fn change_side(row: &Row<'_>, at: usize) -> Result<ChangeFacts, RepoError> {
    let status: Option<String> = row.get(at + 2).map_err(backend)?;
    Ok(ChangeFacts {
        sha256: row.get(at).map_err(backend)?,
        signer: row.get(at + 1).map_err(backend)?,
        status: match status {
            None => None,
            Some(text) => Some(SignatureStatus::parse(&text).ok_or_else(|| decode_error("signature status", &text))?),
        },
    })
}

impl IdentityChangeRepository for SqliteStore {
    fn identity_changes_since(&self, since: Timestamp) -> Result<Vec<IdentityChange>, RepoError> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT e.path_key, e.path, c.old_sha256, c.old_signer, c.old_status,
                        c.new_sha256, c.new_signer, c.new_status, c.changed_at
                 FROM identity_changes c JOIN executables e ON e.id = c.exe_id
                 WHERE c.changed_at >= ?1 ORDER BY c.changed_at, c.id",
            )
            .map_err(backend)?;
        let mut rows = statement.query([since.as_micros()]).map_err(backend)?;
        let mut changes = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            changes.push(IdentityChange {
                exe: Executable { key: row.get(0).map_err(backend)?, path: row.get(1).map_err(backend)? },
                old: change_side(row, 2)?,
                new: change_side(row, 5)?,
                at: Timestamp::from_micros(row.get(8).map_err(backend)?),
            });
        }
        Ok(changes)
    }
}
