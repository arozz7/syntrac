//! What account a program runs as, and which Windows services it hosts: the pure rules (docs/RUNAS-PLAN.md, I4a).
//!
//! Evidence, never a verdict: an account says how a program was started, not whether it is safe. A program can run as more than
//! one account (an updater that runs as SYSTEM and as you), and one service host can carry dozens of services, so both are
//! sets. Only first and last seen are stored, so recording the same sighting twice changes nothing.
//!
//! The Windows adapter (`syntrac-collector`) turns the running processes into a [`ContextSnapshot`]; this module merges it into
//! observations, decides which are worth writing again ([`ContextTracker`]) and defines the repository port.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::time::Duration;

use crate::ports::RepoError;
use crate::rollup::Executable;
use crate::time::Timestamp;

/// The kind of account, from its SID alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AccountKind {
    /// S-1-5-18, the most privileged account.
    System,
    /// S-1-5-19.
    LocalService,
    /// S-1-5-20.
    NetworkService,
    /// A per-service account, `NT SERVICE\...` (S-1-5-80-...).
    ServiceAccount,
    /// An ordinary account of this machine or a domain (S-1-5-21-...).
    User,
    /// Anything else: the window manager, the font driver host, groups, text that is not a SID.
    Other,
}

impl AccountKind {
    /// The word stored in the database.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::LocalService => "local_service",
            Self::NetworkService => "network_service",
            Self::ServiceAccount => "service_account",
            Self::User => "user",
            Self::Other => "other",
        }
    }

    /// The kind for a stored word; an unknown word is `Other`, never a failure.
    pub fn parse(word: &str) -> Self {
        match word {
            "system" => Self::System,
            "local_service" => Self::LocalService,
            "network_service" => Self::NetworkService,
            "service_account" => Self::ServiceAccount,
            "user" => Self::User,
            _ => Self::Other,
        }
    }
}

/// Whether `rest` is a non-empty run of numeric sub-authorities (`1-2-3`).
fn numeric_parts(rest: &str) -> bool {
    !rest.is_empty() && rest.split('-').all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

/// The kind of account a SID names. Surrounding spaces and letter case are ignored; text that is not one of the known shapes is
/// [`AccountKind::Other`], never a guess.
pub fn classify_sid(sid: &str) -> AccountKind {
    let sid = sid.trim().to_uppercase();
    match sid.as_str() {
        "S-1-5-18" => AccountKind::System,
        "S-1-5-19" => AccountKind::LocalService,
        "S-1-5-20" => AccountKind::NetworkService,
        _ => {
            if sid.strip_prefix("S-1-5-80-").is_some_and(numeric_parts) {
                AccountKind::ServiceAccount
            } else if sid.strip_prefix("S-1-5-21-").is_some_and(numeric_parts) {
                AccountKind::User
            } else {
                AccountKind::Other
            }
        }
    }
}

/// A Windows service a process hosts.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ServiceInfo {
    pub name: String,
    pub display_name: Option<String>,
}

/// One running process, as the adapter found it. Strings were already cleaned by the adapter (control characters removed,
/// length capped).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessContext {
    pub path: String,
    pub sid: String,
    pub account_name: Option<String>,
    pub services: Vec<ServiceInfo>,
}

/// Everything running at one moment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContextSnapshot {
    pub processes: Vec<ProcessContext>,
    /// Processes that could not be opened (protected system processes): counted, never an error.
    pub unreadable: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextSourceError(pub String);

impl fmt::Display for ContextSourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ContextSourceError {}

/// Looks at what is running now. The Windows adapter and the test fake implement it.
pub trait ProcessContextSource {
    fn observe(&self) -> Result<ContextSnapshot, ContextSourceError>;
}

/// One program seen running as one account, with the services it hosted at that moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextObservation {
    pub exe: Executable,
    pub sid: String,
    pub account_name: Option<String>,
    pub kind: AccountKind,
    /// Sorted by name, each once.
    pub services: Vec<ServiceInfo>,
}

/// A snapshot merged into observations.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MergedSnapshot {
    /// Sorted by executable key then SID, so the order never depends on the order the processes were listed in.
    pub observations: Vec<ContextObservation>,
    /// Processes dropped for having no path or no SID.
    pub skipped: usize,
    pub unreadable: usize,
}

#[derive(Default)]
struct Group {
    paths: BTreeSet<String>,
    account_name: Option<String>,
    services: BTreeMap<String, Option<String>>,
}

/// Merges a snapshot: instances of one program under one account become one observation, their services added together. A
/// process with no usable path or no SID is skipped and counted. The path kept is the smallest spelling, so the choice is
/// deterministic.
pub fn merge_snapshot(snapshot: ContextSnapshot) -> MergedSnapshot {
    let mut groups: BTreeMap<(String, String), Group> = BTreeMap::new();
    let mut skipped = 0;
    for process in snapshot.processes {
        let sid = process.sid.trim().to_uppercase();
        let Some(exe) = Executable::from_path(&process.path).filter(|_| !sid.is_empty()) else {
            skipped += 1;
            continue;
        };
        let group = groups.entry((exe.key, sid)).or_default();
        group.paths.insert(exe.path);
        if group.account_name.is_none() {
            group.account_name = process.account_name;
        }
        for service in process.services {
            let display = group.services.entry(service.name).or_insert(None);
            if display.is_none() {
                *display = service.display_name;
            }
        }
    }
    let observations = groups
        .into_iter()
        .filter_map(|((key, sid), group)| {
            let path = group.paths.into_iter().next()?;
            Some(ContextObservation {
                exe: Executable { key, path },
                kind: classify_sid(&sid),
                sid,
                account_name: group.account_name,
                services: group
                    .services
                    .into_iter()
                    .map(|(name, display_name)| ServiceInfo { name, display_name })
                    .collect(),
            })
        })
        .collect();
    MergedSnapshot { observations, skipped, unreadable: snapshot.unreadable }
}

struct Written {
    at: Timestamp,
    services: Vec<String>,
}

/// Remembers what has been stored, so a program that keeps running is not rewritten every pass. An observation is due when it
/// was never stored, when its set of services changed, or when the refresh time has passed (to move "last seen"). Something
/// that was never marked written stays due, so a failed write is retried at the next pass.
pub struct ContextTracker {
    refresh_every: Duration,
    written: HashMap<(String, String), Written>,
}

impl ContextTracker {
    pub fn new(refresh_every: Duration) -> Self {
        Self { refresh_every, written: HashMap::new() }
    }

    fn service_names(observation: &ContextObservation) -> Vec<String> {
        observation.services.iter().map(|s| s.name.clone()).collect()
    }

    /// The observations that should be written now.
    pub fn due(&self, observations: &[ContextObservation], now: Timestamp) -> Vec<ContextObservation> {
        let refresh = i64::try_from(self.refresh_every.as_micros()).unwrap_or(i64::MAX);
        observations
            .iter()
            .filter(|observation| match self.written.get(&(observation.exe.key.clone(), observation.sid.clone())) {
                None => true,
                Some(written) => {
                    written.services != Self::service_names(observation)
                        || now.as_micros().saturating_sub(written.at.as_micros()) >= refresh
                }
            })
            .cloned()
            .collect()
    }

    /// Records that these were stored at `now`.
    pub fn mark_written(&mut self, observations: &[ContextObservation], now: Timestamp) {
        for observation in observations {
            self.written.insert(
                (observation.exe.key.clone(), observation.sid.clone()),
                Written { at: now, services: Self::service_names(observation) },
            );
        }
    }

    /// Drops what is no longer running, so the memory cannot grow without limit over weeks. A program that comes back is simply
    /// written again.
    pub fn forget_unseen(&mut self, current: &[ContextObservation]) {
        let seen: BTreeSet<(&str, &str)> = current.iter().map(|o| (o.exe.key.as_str(), o.sid.as_str())).collect();
        self.written.retain(|(key, sid), _| seen.contains(&(key.as_str(), sid.as_str())));
    }

    pub fn len(&self) -> usize {
        self.written.len()
    }

    pub fn is_empty(&self) -> bool {
        self.written.is_empty()
    }
}

/// One account a program has been seen running as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountSeen {
    pub sid: String,
    pub name: Option<String>,
    pub kind: AccountKind,
    pub first_seen: Timestamp,
    pub last_seen: Timestamp,
}

/// One service a program has been seen hosting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceSeen {
    pub name: String,
    pub display_name: Option<String>,
    pub first_seen: Timestamp,
    pub last_seen: Timestamp,
}

/// Everything known about how one program has been seen running. Accounts are most recently seen first (a tie: by SID),
/// services likewise (by name).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecutableContext {
    pub accounts: Vec<AccountSeen>,
    pub services: Vec<ServiceSeen>,
}

/// What a write added that was not known before.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ContextWrite {
    pub new_accounts: usize,
    pub new_services: usize,
}

/// The memory of how programs have been seen running. Permanent: the purge of detail never touches it.
pub trait ContextRepository {
    /// ONE atomic step: records every observation as seen at `at`. A program the rollups have not met yet is added (its first and
    /// last seen come later, from the rollups). First seen only moves earlier and last seen only later, so recording the same
    /// sighting twice changes nothing. A name replaces an older one only when it is present.
    fn record_context(&mut self, observations: &[ContextObservation], at: Timestamp)
        -> Result<ContextWrite, RepoError>;

    /// How one program has been seen running; the path is matched without regard to case. Empty when never seen.
    fn context_of(&self, path: &str) -> Result<ExecutableContext, RepoError>;

    /// Every program that has been seen running, with its context, sorted by executable key.
    fn all_contexts(&self) -> Result<Vec<(Executable, ExecutableContext)>, RepoError>;
}
