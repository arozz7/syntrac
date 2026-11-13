//! Conversions between domain values and SQLite column values.

use syntrac_core::{ProcessInstanceId, RepoError, Severity};
use uuid::Uuid;

use crate::connection::backend;

/// Ids are stored as their 16 raw bytes (a 36-character text id is more than twice the size, in every row and index).
pub(crate) type IdBytes = [u8; 16];

pub(crate) fn instance_to_blob(id: ProcessInstanceId) -> IdBytes {
    *id.as_uuid().as_bytes()
}

pub(crate) fn instance_from_blob(bytes: IdBytes) -> ProcessInstanceId {
    ProcessInstanceId::from_uuid(Uuid::from_bytes(bytes))
}

pub(crate) fn uuid_to_blob(id: Uuid) -> IdBytes {
    *id.as_bytes()
}

pub(crate) fn uuid_from_blob(bytes: IdBytes) -> Uuid {
    Uuid::from_bytes(bytes)
}

pub(crate) fn severity_to_text(severity: Severity) -> Result<String, RepoError> {
    match serde_json::to_value(severity).map_err(backend)? {
        serde_json::Value::String(text) => Ok(text),
        other => Err(RepoError::Backend(format!("unexpected severity encoding: {other}"))),
    }
}

pub(crate) fn severity_from_text(text: &str) -> Result<Severity, RepoError> {
    serde_json::from_value(serde_json::Value::String(text.to_owned())).map_err(backend)
}

/// SQLite integers are signed 64-bit; byte counters beyond that saturate rather than wrap.
pub(crate) fn to_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

pub(crate) fn to_u64(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}
