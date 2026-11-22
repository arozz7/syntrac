//! The process tree view (docs/PHASE5-PLAN.md, A1): who started whom over a window, built from the processes alive in it.
//! Same conventions as `history.rs`: camelCase on the wire, times in milliseconds since the Unix epoch.

use serde::Serialize;
use syntrac_core::process_tree::{build_forest, TreeLimits, TreeNode};
use syntrac_core::{
    clean_text, ContextRepository, IdentityRepository, InstallRepository, KnownProductRepository, LauncherRepository,
    ProcessFilter, ProcessQueries, RepoError, Timestamp,
};

use crate::history::ms;
use crate::identity_view::{Identities, IdentityBadgeView};
use crate::names::display_name;

/// The days of per-process detail the service keeps; a longer window has nothing more to show.
pub const DETAIL_DAYS: u64 = 14;
const MIN_WINDOW_SECS: u64 = 60;
/// A command line is shown cut to this many characters (it comes from outside and can be very long).
pub const MAX_COMMAND_LINE_CHARS: usize = 300;
/// How many processes are read for one tree (the newest ones, when the window holds more).
pub const FETCH_LIMIT: usize = 20_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessNodeView {
    pub id: String,
    pub pid: u32,
    pub name: String,
    pub image_path: String,
    pub command_line: Option<String>,
    pub started_ms: i64,
    pub ended_ms: Option<i64>,
    /// The end time is only an upper bound (the process ended while the collector was not running).
    pub ended_approximate: bool,
    pub running: bool,
    /// Its starter is named but not in this window (it ended earlier, or was not recorded).
    pub parent_missing: bool,
    /// Processes below this one that exist but are not shown.
    pub hidden_descendants: usize,
    pub identity: IdentityBadgeView,
    pub children: Vec<ProcessNodeView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessTreeView {
    pub from_ms: i64,
    pub to_ms: i64,
    /// Processes alive in the window that were read, and how many of them are shown.
    pub total: usize,
    pub shown: usize,
    /// How many processes were read (at most `FETCH_LIMIT`).
    pub read: usize,
    /// The window held more than could be read: these are the newest ones, so a process may look parentless only because its starter is
    /// among the older ones that were left out.
    pub truncated: bool,
    pub roots: Vec<ProcessNodeView>,
}

/// The tree of the processes alive in the last `window_secs` (kept within one minute and the days of detail that are stored).
pub fn process_tree<R>(repo: &R, now: Timestamp, window_secs: u64) -> Result<ProcessTreeView, RepoError>
where
    R: ProcessQueries
        + IdentityRepository
        + ContextRepository
        + LauncherRepository
        + InstallRepository
        + KnownProductRepository,
{
    let window_secs = window_secs.clamp(MIN_WINDOW_SECS, DETAIL_DAYS * 24 * 3600);
    let from = Timestamp::from_micros(now.as_micros() - i64::try_from(window_secs).unwrap_or(i64::MAX) * 1_000_000);
    // One more than the limit is asked for, to know whether the window held more (the list is newest first, so the extra is the oldest).
    let filter = ProcessFilter { alive_between: Some((from, now)), limit: FETCH_LIMIT + 1, ..ProcessFilter::default() };
    let mut records = repo.list_processes(&filter)?;
    let truncated = records.len() > FETCH_LIMIT;
    records.truncate(FETCH_LIMIT);
    let read = records.len();
    let forest = build_forest(records, TreeLimits::default());
    let identities = Identities::load(repo)?;
    Ok(ProcessTreeView {
        from_ms: ms(from),
        to_ms: ms(now),
        total: forest.total,
        shown: forest.shown,
        read,
        truncated,
        roots: forest.roots.into_iter().map(|node| node_view(node, &identities)).collect(),
    })
}

fn node_view(node: TreeNode, identities: &Identities) -> ProcessNodeView {
    let process = node.process;
    ProcessNodeView {
        id: process.id.as_uuid().to_string(),
        pid: process.pid,
        name: display_name(&process.image_path, process.pid),
        identity: identities.badge(&process.image_path),
        command_line: process.command_line.as_deref().and_then(|line| clean_text(line, MAX_COMMAND_LINE_CHARS)),
        started_ms: ms(process.started),
        ended_ms: process.ended.map(ms),
        ended_approximate: process.ended_approximate,
        running: process.ended.is_none(),
        parent_missing: node.parent_missing,
        hidden_descendants: node.hidden_descendants,
        children: node.children.into_iter().map(|child| node_view(child, identities)).collect(),
        image_path: process.image_path,
    }
}
