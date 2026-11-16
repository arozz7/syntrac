//! The owner dataset as the window sees it while the service downloads or has downloaded it. The stored dataset itself is read
//! from the database (`history_network::OwnerDataView`); this is the service's live state.

use serde::Serialize;
use syntrac_ipc::protocol::OwnerDataReport;

use crate::history_network::OwnerDataView;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnerDataStateView {
    pub dataset: Option<OwnerDataView>,
    /// A download is under way now.
    pub refreshing: bool,
    /// Why the last download failed, in plain words.
    pub last_error: Option<String>,
    /// `false` when the service is configured never to download.
    pub allowed: bool,
}

impl From<OwnerDataReport> for OwnerDataStateView {
    fn from(report: OwnerDataReport) -> Self {
        Self {
            dataset: report.dataset.map(|dataset| OwnerDataView {
                source: dataset.source,
                downloaded_ms: dataset.downloaded_at_micros / 1_000,
                rows: dataset.rows,
            }),
            refreshing: report.refreshing,
            last_error: report.last_error,
            allowed: report.allowed,
        }
    }
}
