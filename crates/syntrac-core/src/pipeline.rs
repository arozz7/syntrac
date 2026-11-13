//! One ingest step: drain a source, correlate in time order, persist.

use crate::correlator::correlate;
use crate::ports::{EventRepository, ProcessRepository, RepoError, SourceError, TelemetrySource};

#[derive(Debug, thiserror::Error)]
pub enum IngestError {
    #[error(transparent)]
    Source(#[from] SourceError),
    #[error(transparent)]
    Repo(#[from] RepoError),
}

/// Returns how many events were stored.
pub fn ingest<S, R>(source: &mut S, repo: &mut R) -> Result<usize, IngestError>
where
    S: TelemetrySource + ?Sized,
    R: ProcessRepository + EventRepository + ?Sized,
{
    let mut events = source.drain()?;
    events.sort_by_key(|event| event.ts);

    let mut correlated = Vec::with_capacity(events.len());
    for event in events {
        correlated.push(correlate(repo, event)?);
    }

    repo.append(&correlated)?;
    Ok(correlated.len())
}
