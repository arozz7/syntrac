//! How programs get started: the ports and the plain data for launch records (services, scheduled tasks, Run keys, Startup
//! folders), which program each one starts and since when. Evidence, never a verdict (`docs/RUNAS-PLAN.md`, I4b).
//!
//! A SOURCE reads one place on the machine and returns raw entries (a command line as written, where it was found). The pure
//! `launch_run` turns those into programs; the REPOSITORY keeps what was seen, with first and last seen, permanently.

use crate::ports::RepoError;
use crate::rollup::Executable;
use crate::time::Timestamp;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LauncherKind {
    /// A Windows service that starts with Windows (automatic, including delayed automatic).
    ServiceAuto,
    /// A service that starts on demand or is disabled: a fact, but a quiet one.
    ServiceOther,
    /// A scheduled task with a command to run.
    Task,
    RunKey,
    RunOnce,
    StartupFolder,
}

impl LauncherKind {
    pub const ALL: [LauncherKind; 6] =
        [Self::ServiceAuto, Self::ServiceOther, Self::Task, Self::RunKey, Self::RunOnce, Self::StartupFolder];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ServiceAuto => "service_auto",
            Self::ServiceOther => "service_other",
            Self::Task => "task",
            Self::RunKey => "run_key",
            Self::RunOnce => "run_once",
            Self::StartupFolder => "startup_folder",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == text)
    }

    /// Whether this starts the program when Windows (or a user) starts, as opposed to a fact that does not by itself start it.
    pub fn starts_with_windows(self) -> bool {
        !matches!(self, Self::ServiceOther)
    }
}

/// One entry as a source found it: nothing resolved yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawEntry {
    pub kind: LauncherKind,
    /// Where it was found: the service name, the task path, the key and value name, the shortcut path.
    pub location: String,
    /// The command line as written (variables not expanded).
    pub command: String,
    /// What the source knows besides the command: the kinds of trigger of a task, the start type of a service.
    pub detail: String,
    /// The profile folder of the user it belongs to (`C:\Users\name`), for entries that live in one user's registry hive or
    /// folder. `%USERPROFILE%`, `%APPDATA%` and `%LOCALAPPDATA%` in the command mean this user's folders.
    pub profile: Option<String>,
}

/// What one source read.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SourceRead {
    pub entries: Vec<RawEntry>,
    /// Items the source could not read (a file it was not allowed to open). Counted, never an error.
    pub unreadable: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct LaunchSourceError(pub String);

/// A place on the machine that says what starts. Reads only.
pub trait LaunchSource {
    fn name(&self) -> &'static str;
    /// The kinds this source covers completely when `read` succeeds.
    fn kinds(&self) -> &'static [LauncherKind];
    fn read(&self) -> Result<SourceRead, LaunchSourceError>;
}

/// A program that an entry starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherObservation {
    pub exe: Executable,
    pub kind: LauncherKind,
    pub location: String,
    pub detail: String,
    pub arguments: String,
}

/// An entry that has been seen starting a program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherSeen {
    pub kind: LauncherKind,
    pub location: String,
    pub detail: String,
    pub arguments: String,
    pub first_seen: Timestamp,
    pub last_seen: Timestamp,
    /// The entry was still there when its kind was last read completely.
    pub current: bool,
}

/// What a write changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LauncherWrite {
    pub written: u32,
    /// Entries never seen before.
    pub new: u32,
}

pub trait LauncherRepository {
    /// Records the entries seen at `at` and marks every kind in `completed` as read completely at `at`: an entry of such a kind
    /// that is not among `rows` is, from now on, no longer current. Idempotent: only first and last seen are kept (MIN and MAX).
    /// A program the rollups have not met gets its row here, with the spelling first seen.
    fn record_launchers(
        &mut self,
        rows: &[LauncherObservation],
        completed: &[LauncherKind],
        at: Timestamp,
    ) -> Result<LauncherWrite, RepoError>;

    /// Every entry seen starting the program at `path` (newest first); empty when none.
    fn launchers_of(&self, path: &str) -> Result<Vec<LauncherSeen>, RepoError>;

    /// Every program that has an entry, with its entries.
    fn all_launchers(&self) -> Result<Vec<(Executable, Vec<LauncherSeen>)>, RepoError>;
}
