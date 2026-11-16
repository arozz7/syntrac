//! Getting the owner dataset onto the machine and replacing the old one, without ever leaving a half dataset behind. The download
//! is a port (`DatasetFetcher`: the Windows adapter and a test fake), the store is a port (`OwnerRepository`), the checks are
//! pure. `docs/OWNER-PLAN.md`.

use std::net::IpAddr;

use crate::owner_dataset::{parse_ip2asn, Owner, OwnerRange};
use crate::ports::RepoError;
use crate::time::Timestamp;

/// What is known about the dataset that is stored now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatasetInfo {
    /// Where it came from, for the screen ("downloaded <date> from <source>").
    pub source: String,
    pub downloaded_at: Timestamp,
    /// Ranges kept.
    pub rows: u32,
    pub bad_lines: u32,
    /// SHA-256 of the file as downloaded.
    pub sha256: String,
}

pub trait OwnerRepository {
    /// `None` until a dataset was stored.
    fn dataset_info(&self) -> Result<Option<DatasetInfo>, RepoError>;

    /// Replaces the whole dataset at once: a reader sees the old one or the new one, never a mixture, and a failure leaves the
    /// old one in place. `ranges` are sorted by start and do not overlap (as `parse_ip2asn` returns them).
    fn replace_dataset(&mut self, ranges: &[OwnerRange], info: &DatasetInfo) -> Result<(), RepoError>;

    /// Who owns `addr`, from the stored dataset. `None`: no dataset, or the address is in no range.
    fn lookup_owner(&self, addr: IpAddr) -> Result<Option<Owner>, RepoError>;
}

/// A downloaded file, already decompressed, with where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedFile {
    pub text: String,
    pub source: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct FetchError(pub String);

/// The one thing that leaves the machine: fetching the dataset file. Implementations enforce the transfer limits.
pub trait DatasetFetcher {
    fn fetch(&self) -> Result<FetchedFile, FetchError>;
}

impl<T: DatasetFetcher + ?Sized> DatasetFetcher for std::sync::Arc<T> {
    fn fetch(&self) -> Result<FetchedFile, FetchError> {
        (**self).fetch()
    }
}

/// How a downloaded file is judged before it may replace the stored one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefreshLimits {
    /// Fewer usable ranges than this is not the dataset (an error page, a truncated file).
    pub min_ranges: usize,
    /// More unusable lines than this share (percent of all lines read) is not the dataset either.
    pub max_bad_percent: u32,
}

impl Default for RefreshLimits {
    fn default() -> Self {
        Self { min_ranges: 1_000, max_bad_percent: 5 }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum OwnerRefreshError {
    #[error("the download failed: {0}")]
    Fetch(String),
    #[error("the downloaded file was not used: {0}")]
    Rejected(String),
    #[error("could not store the owner data: {0}")]
    Store(#[from] RepoError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefreshOutcome {
    pub rows: u32,
    pub bad_lines: u32,
    pub unrouted: u32,
}

/// Downloads, checks and stores the dataset. Any failure leaves the stored dataset as it was.
pub fn refresh_owner_data<F: DatasetFetcher, R: OwnerRepository>(
    fetcher: &F,
    repo: &mut R,
    now: Timestamp,
    limits: &RefreshLimits,
) -> Result<RefreshOutcome, OwnerRefreshError> {
    let file = fetcher.fetch().map_err(|error| OwnerRefreshError::Fetch(error.0))?;
    let parsed = parse_ip2asn(&file.text);

    if parsed.ranges.len() < limits.min_ranges {
        return Err(OwnerRefreshError::Rejected(format!(
            "it holds {} usable ranges, fewer than the {} expected",
            parsed.ranges.len(),
            limits.min_ranges
        )));
    }
    let lines = parsed.ranges.len() as u64 + u64::from(parsed.unrouted) + u64::from(parsed.bad_lines);
    if u64::from(parsed.bad_lines) * 100 > lines * u64::from(limits.max_bad_percent) {
        return Err(OwnerRefreshError::Rejected(format!(
            "{} of its {lines} lines could not be read",
            parsed.bad_lines
        )));
    }

    let rows = u32::try_from(parsed.ranges.len()).unwrap_or(u32::MAX);
    let info =
        DatasetInfo { source: file.source, downloaded_at: now, rows, bad_lines: parsed.bad_lines, sha256: file.sha256 };
    repo.replace_dataset(&parsed.ranges, &info)?;
    Ok(RefreshOutcome { rows, bad_lines: parsed.bad_lines, unrouted: parsed.unrouted })
}
