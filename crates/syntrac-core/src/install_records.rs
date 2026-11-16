//! Install records: what the Uninstall registry keys say about installed products (name, publisher, version, install folder), kept as
//! a CLUE about where a program came from (`docs/RUNAS-PLAN.md`, I4c).
//!
//! Anything that can write the registry can claim any name, so a record never replaces the signature and never changes a standing.
//! Records are not stored per program (one product has many executables): a program is matched to the record whose install folder
//! holds it, when it is shown. Pure: no I/O.

use crate::clean_text::clean_text;
use crate::ports::RepoError;
use crate::time::Timestamp;

const MAX_NAME_CHARS: usize = 200;
const MAX_FIELD_CHARS: usize = 120;
/// More records than this in one sweep are ignored (a machine has hundreds).
pub const MAX_RECORDS: usize = 5_000;
/// The most records shown for one program.
const MAX_MATCHES: usize = 3;

/// One Uninstall entry as the registry holds it: the text values, nothing interpreted.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RawInstall {
    /// Which registry view it came from: `machine` (64-bit), `machine32` (32-bit programs) or `user:<sid>`.
    pub hive: String,
    /// The name of the entry's key (a product code or a name).
    pub key_name: String,
    pub display_name: String,
    pub publisher: String,
    pub version: String,
    pub install_date: String,
    pub install_location: String,
    pub display_icon: String,
    pub uninstall_string: String,
    /// The entry is marked as a hidden system component.
    pub system_component: bool,
}

/// An Uninstall entry, cleaned and with its install folder worked out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallRecord {
    pub hive: String,
    pub key_name: String,
    pub display_name: String,
    pub publisher: Option<String>,
    pub version: Option<String>,
    pub install_date: Option<String>,
    /// The folder the product is installed in: lower case, no trailing backslash. `None` when the entry names none we can use
    /// (or only one too general to mean anything, such as `C:\Program Files`).
    pub location: Option<String>,
    /// The folder was worked out from the icon or the uninstall command, not stated: weaker evidence, and said so.
    pub derived: bool,
    pub system_component: bool,
}

/// A record that has been seen, and whether it was still there when the keys were last read completely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallSeen {
    pub record: InstallRecord,
    pub first_seen: Timestamp,
    pub last_seen: Timestamp,
    pub current: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InstallWrite {
    pub written: u32,
    /// Records never seen before.
    pub new: u32,
}

pub trait InstallRepository {
    /// Records the entries seen at `at` and marks the Uninstall keys as read completely at `at`: a record not among `rows` is, from
    /// now on, no longer installed (its history stays). Idempotent; only first and last seen are kept (MIN and MAX), and the other
    /// fields are those of the newest sighting.
    fn record_installs(&mut self, rows: &[InstallRecord], at: Timestamp) -> Result<InstallWrite, RepoError>;

    /// Every record ever seen.
    fn installs(&self) -> Result<Vec<InstallSeen>, RepoError>;
}

/// What a source read.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SourceInstalls {
    pub entries: Vec<RawInstall>,
    /// Keys that could not be read; counted, never an error.
    pub unreadable: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct InstallSourceError(pub String);

/// The Uninstall keys of the machine. Reads only.
pub trait InstallSource {
    fn read(&self) -> Result<SourceInstalls, InstallSourceError>;
}

// ---- from a registry entry to a record

fn components(path: &str) -> Vec<&str> {
    path.split('\\').filter(|part| !part.is_empty()).collect()
}

/// A folder as written (quotes, forward slashes, a trailing backslash, a `\\?\` prefix) made plain and lower case. `None` when it
/// is not an absolute path or is too general to say anything about one product.
fn plain_folder(text: &str) -> Option<String> {
    let mut path = text.trim().trim_matches('"').trim().replace('/', r"\");
    if let Some(rest) = path.strip_prefix(r"\\?\") {
        path = rest.to_owned();
    }
    let path = path.trim_end_matches('\\').to_ascii_lowercase();
    let parts = components(&path);
    let drive = path.len() >= 3
        && path.as_bytes()[1] == b':'
        && path.as_bytes()[2] == b'\\'
        && path.as_bytes()[0].is_ascii_alphabetic();
    if path.starts_with(r"\\") {
        // \\server\share is a share, not a product; a folder inside it is.
        return (parts.len() >= 3).then_some(path);
    }
    if !drive {
        return None;
    }
    let rest = &parts[1..];
    let general = match rest {
        [] => true,
        // Anything under the operating system's own folder is the system, not a product.
        ["windows", ..] => true,
        ["program files" | "program files (x86)" | "programdata" | "users", ..] if rest.len() == 1 => true,
        ["users", ..] if rest.len() <= 2 => true,
        ["users", _, "appdata", tail @ ..] => {
            matches!(tail, [] | ["local"] | ["roaming"] | ["locallow"] | ["local", "programs"] | ["local", "temp"])
        }
        ["users", _, "downloads" | "desktop" | "documents", ..] => rest.len() == 3,
        _ => false,
    };
    (!general).then_some(path)
}

/// The folder a file path sits in.
fn folder_of_file(path: &str) -> Option<String> {
    let path = path.trim().trim_matches('"').replace('/', r"\");
    let (folder, _) = path.rsplit_once('\\')?;
    plain_folder(folder)
}

/// The program path at the start of a command (`"C:\x\unins.exe" /S` or `C:\x\unins.exe /S`).
fn program_of_command(command: &str) -> Option<String> {
    let command = command.trim();
    if let Some(after) = command.strip_prefix('"') {
        return after.find('"').map(|end| after[..end].to_owned());
    }
    let lower = command.to_ascii_lowercase();
    let end = lower.find(".exe")? + 4;
    Some(command[..end].to_owned())
}

/// A `DisplayIcon` is a path, often with a resource index (`"C:\x\x.exe",0`).
fn icon_path(icon: &str) -> String {
    let icon = icon.trim();
    match icon.rsplit_once(',') {
        Some((path, index)) if index.trim().trim_start_matches('-').chars().all(|c| c.is_ascii_digit()) => {
            path.to_owned()
        }
        _ => icon.to_owned(),
    }
}

fn cleaned(text: &str, max: usize) -> Option<String> {
    clean_text(text, max)
}

/// A record from a registry entry. `None` when it has no usable name (patches and sub-components often have none).
pub fn to_record(raw: &RawInstall) -> Option<InstallRecord> {
    let display_name = cleaned(&raw.display_name, MAX_NAME_CHARS)?;
    let stated = plain_folder(&raw.install_location);
    let (location, derived) = match stated {
        Some(folder) => (Some(folder), false),
        None => {
            let from_icon = folder_of_file(&icon_path(&raw.display_icon));
            let from_uninstall = program_of_command(&raw.uninstall_string).and_then(|program| folder_of_file(&program));
            match from_icon.or(from_uninstall) {
                Some(folder) => (Some(folder), true),
                None => (None, false),
            }
        }
    };
    Some(InstallRecord {
        hive: raw.hive.clone(),
        key_name: cleaned(&raw.key_name, MAX_NAME_CHARS).unwrap_or_default(),
        display_name,
        publisher: cleaned(&raw.publisher, MAX_FIELD_CHARS),
        version: cleaned(&raw.version, MAX_FIELD_CHARS),
        install_date: cleaned(&raw.install_date, MAX_FIELD_CHARS),
        location,
        derived,
        system_component: raw.system_component,
    })
}

// ---- which record holds a program

fn plain_path(path: &str) -> String {
    let path = path.trim().trim_matches('"').replace('/', r"\");
    path.strip_prefix(r"\\?\").unwrap_or(&path).to_ascii_lowercase()
}

/// The records whose install folder holds the program at `path`: the most specific folder wins, equally specific ones are all
/// returned (not one picked silently), the same product listed twice (the 32-bit and 64-bit views) once, a record still installed
/// before one that is gone. At most three.
pub fn registered_for<'a>(installs: &'a [InstallSeen], path: &str) -> Vec<&'a InstallSeen> {
    let path = plain_path(path);
    let holds =
        |folder: &str| path.len() > folder.len() && path.starts_with(folder) && path.as_bytes()[folder.len()] == b'\\';
    let candidates: Vec<&InstallSeen> =
        installs.iter().filter(|seen| seen.record.location.as_deref().is_some_and(&holds)).collect();
    let longest = candidates.iter().filter_map(|seen| seen.record.location.as_ref().map(String::len)).max();
    let mut best: Vec<&InstallSeen> =
        candidates.into_iter().filter(|seen| seen.record.location.as_ref().map(String::len) == longest).collect();
    best.sort_by(|a, b| {
        b.current
            .cmp(&a.current)
            .then_with(|| a.record.display_name.to_lowercase().cmp(&b.record.display_name.to_lowercase()))
    });
    let mut names: Vec<String> = Vec::new();
    best.retain(|seen| {
        let name = seen.record.display_name.to_lowercase();
        let first = !names.contains(&name);
        names.push(name);
        first
    });
    best.truncate(MAX_MATCHES);
    best
}

// ---- a sweep

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InstallSweep {
    /// Entries read.
    pub entries: u32,
    /// Records kept (entries with a usable name).
    pub recorded: u32,
    /// Of those, how many name an install folder.
    pub with_folder: u32,
    pub unreadable: u32,
    pub new: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum InstallSweepError {
    /// The Uninstall keys could not be read; nothing was recorded and nothing is marked as removed.
    #[error("{0}")]
    Source(String),
    #[error("{0}")]
    Store(#[from] RepoError),
}

/// Reads the Uninstall keys and records them.
pub fn sweep_installs<R: InstallRepository>(
    source: &dyn InstallSource,
    repo: &mut R,
    now: Timestamp,
) -> Result<InstallSweep, InstallSweepError> {
    let read = source.read().map_err(|error| InstallSweepError::Source(error.0))?;
    let mut sweep = InstallSweep {
        entries: u32::try_from(read.entries.len()).unwrap_or(u32::MAX),
        unreadable: read.unreadable,
        ..InstallSweep::default()
    };
    let mut seen = std::collections::BTreeSet::new();
    let records: Vec<InstallRecord> = read
        .entries
        .iter()
        .take(MAX_RECORDS)
        .filter_map(to_record)
        .filter(|record| seen.insert((record.hive.clone(), record.key_name.clone())))
        .collect();
    sweep.recorded = u32::try_from(records.len()).unwrap_or(u32::MAX);
    sweep.with_folder = u32::try_from(records.iter().filter(|r| r.location.is_some()).count()).unwrap_or(u32::MAX);
    sweep.new = repo.record_installs(&records, now)?.new;
    Ok(sweep)
}
