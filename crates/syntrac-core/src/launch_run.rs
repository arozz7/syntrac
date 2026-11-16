//! One sweep of the places that say what starts: read every source, map each entry to the program it starts, record the result.
//! Pure: the sources, the environment, the file system and the repository are passed in. A source that fails is skipped and the
//! others still run; a kind that was not read completely is not marked as read, so nothing is wrongly shown as removed.

use std::collections::BTreeMap;

use crate::install_records::InstallSweep;
use crate::launch_target::launch_target;
use crate::launchers::{LaunchSource, LauncherKind, LauncherObservation, LauncherRepository, RawEntry};
use crate::ports::RepoError;
use crate::rollup::Executable;
use crate::time::Timestamp;

/// More entries than this in one sweep are ignored (a machine has hundreds; this is a bound against a runaway source).
pub const MAX_ENTRIES: usize = 5_000;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SweepReport {
    /// Entries read from all sources that worked.
    pub entries: u32,
    /// Entries that were mapped to a program and recorded.
    pub recorded: u32,
    /// Entries whose command could not be turned into a program path (counted, not stored).
    pub unresolved: u32,
    /// Recorded entries whose program file is not there.
    pub missing_files: u32,
    pub unreadable: u32,
    /// Entries never seen before.
    pub new: u32,
    /// The sources that failed, with why.
    pub failed: Vec<(String, String)>,
    /// The Uninstall keys read in the same pass (install records), when that source is used.
    pub installs: Option<InstallSweep>,
}

/// `%USERPROFILE%` and friends for an entry that belongs to one user.
fn profile_variable(profile: &str, name: &str) -> Option<String> {
    match name.to_ascii_lowercase().as_str() {
        "userprofile" => Some(profile.to_owned()),
        "appdata" => Some(format!(r"{profile}\AppData\Roaming")),
        "localappdata" => Some(format!(r"{profile}\AppData\Local")),
        _ => None,
    }
}

fn observation(
    entry: &RawEntry,
    env: &dyn Fn(&str) -> Option<String>,
    exists: &dyn Fn(&str) -> bool,
    report: &mut SweepReport,
) -> Option<LauncherObservation> {
    let entry_env =
        |name: &str| entry.profile.as_deref().and_then(|profile| profile_variable(profile, name)).or_else(|| env(name));
    let Some(target) = launch_target(&entry.command, &entry_env, exists) else {
        report.unresolved += 1;
        return None;
    };
    let Some(exe) = Executable::from_path(&target.path) else {
        report.unresolved += 1;
        return None;
    };
    if !target.exists {
        report.missing_files += 1;
    }
    Some(LauncherObservation {
        exe,
        kind: entry.kind,
        location: entry.location.clone(),
        detail: entry.detail.clone(),
        arguments: target.arguments,
    })
}

/// Reads every source, maps and records. `Err` only when the repository fails.
pub fn sweep_launchers<R: LauncherRepository>(
    sources: &[&dyn LaunchSource],
    env: &dyn Fn(&str) -> Option<String>,
    exists: &dyn Fn(&str) -> bool,
    repo: &mut R,
    now: Timestamp,
) -> Result<SweepReport, RepoError> {
    let mut report = SweepReport::default();
    let mut completed: Vec<LauncherKind> = Vec::new();
    let mut entries: Vec<RawEntry> = Vec::new();
    for source in sources {
        match source.read() {
            Ok(read) => {
                completed.extend_from_slice(source.kinds());
                report.unreadable += read.unreadable;
                entries.extend(read.entries);
            }
            Err(error) => report.failed.push((source.name().to_owned(), error.0)),
        }
    }
    if completed.is_empty() {
        return Ok(report);
    }
    entries.truncate(MAX_ENTRIES);
    report.entries = u32::try_from(entries.len()).unwrap_or(u32::MAX);

    // The same program started by the same entry twice is one record.
    let mut found: BTreeMap<(String, LauncherKind, String), LauncherObservation> = BTreeMap::new();
    for entry in &entries {
        if let Some(seen) = observation(entry, env, exists, &mut report) {
            found.insert((seen.exe.key.clone(), seen.kind, seen.location.clone()), seen);
        }
    }
    let rows: Vec<LauncherObservation> = found.into_values().collect();
    report.recorded = u32::try_from(rows.len()).unwrap_or(u32::MAX);
    completed.sort();
    completed.dedup();
    let write = repo.record_launchers(&rows, &completed, now)?;
    report.new = write.new;
    Ok(report)
}
