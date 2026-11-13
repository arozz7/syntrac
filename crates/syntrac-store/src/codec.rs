//! Conversions between domain values and SQLite column values.

use syntrac_core::{ProcessInstanceId, RepoError, Severity};
use uuid::Uuid;

use crate::connection::backend;

pub(crate) fn instance_to_text(id: ProcessInstanceId) -> String {
    id.as_uuid().to_string()
}

pub(crate) fn instance_from_text(text: &str) -> Result<ProcessInstanceId, RepoError> {
    Uuid::parse_str(text).map(ProcessInstanceId::from_uuid).map_err(backend)
}

/// Same as [`instance_from_text`] but shaped for use inside a rusqlite row closure.
pub(crate) fn instance_from_column(text: String) -> Result<ProcessInstanceId, rusqlite::Error> {
    Uuid::parse_str(&text)
        .map(ProcessInstanceId::from_uuid)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))
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
