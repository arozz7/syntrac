//! What the machine can tell us about an executable FILE: its signature, version resource, hash, size and date.
//!
//! These are facts MEASURED from the file by an adapter (`FileInspector`) and stored permanently per executable. They are
//! evidence, never a verdict: nothing here says a program is safe. Anything that can be worked out from the path alone (its
//! location class, its product) is NOT stored; it is computed on demand by `exe_location`, so it can never go stale.

use crate::rollup::{Executable, LedgerEntry};
use crate::time::Timestamp;

/// How far the file's signature can be trusted, as the operating system judged it (offline: revocation is not checked).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SignatureStatus {
    /// Signed, and the chain leads to a trusted root.
    Valid,
    /// Signed, but the chain is not trusted (unknown root, expired, ...).
    Untrusted,
    /// Signed, but the file no longer matches its signature: it was changed after signing.
    Altered,
    #[default]
    Unsigned,
}

impl SignatureStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::Untrusted => "untrusted",
            Self::Altered => "altered",
            Self::Unsigned => "unsigned",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        [Self::Valid, Self::Untrusted, Self::Altered, Self::Unsigned].into_iter().find(|s| s.as_str() == text)
    }
}

/// Where the signature is stored: inside the file, or in a Windows catalog (most operating-system files).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureKind {
    Embedded,
    Catalog,
}

impl SignatureKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Embedded => "embedded",
            Self::Catalog => "catalog",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        [Self::Embedded, Self::Catalog].into_iter().find(|s| s.as_str() == text)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Signature {
    pub status: SignatureStatus,
    /// The subject of the signing certificate, as text. Attacker-influenced: show it as text, never interpret it.
    pub signer: Option<String>,
    /// SHA-1 thumbprint of the signing certificate, as Windows shows it (upper-case hex).
    pub thumbprint: Option<String>,
    pub kind: Option<SignatureKind>,
}

/// The version resource of the file. Anyone can write anything here: a label, not proof.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VersionInfo {
    pub company: Option<String>,
    pub product: Option<String>,
    pub description: Option<String>,
    /// The name the file had when it was built; a different name on disk means it was renamed.
    pub original_filename: Option<String>,
    pub file_version: Option<String>,
}

/// Size and modification time: the cheap way to tell whether a file changed since it was last read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FileStamp {
    pub size: u64,
    pub modified: Option<Timestamp>,
}

/// Everything measured from one read of the file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileFacts {
    pub stamp: FileStamp,
    /// Lower-case hex. `None` when the file was too large to hash (the size says why).
    pub sha256: Option<String>,
    pub signature: Signature,
    pub version: VersionInfo,
    /// The Windows "mark of the web" zone (`Zone.Identifier`): 3 is the Internet. `None`: the file carries no mark.
    pub zone_id: Option<u32>,
}

impl FileFacts {
    /// The file was downloaded from the Internet (or a restricted zone) and has not been unblocked.
    pub fn downloaded(&self) -> bool {
        self.zone_id.is_some_and(|zone| zone >= 3)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InspectErrorKind {
    NotFound,
    AccessDenied,
    /// A bare name, a directory, a device, a link or a network path: not something we read.
    NotAFile,
    TimedOut,
    /// The service is stopping. Says NOTHING about the file: never stored, never counted as a failure.
    Cancelled,
    Other,
}

impl InspectErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::AccessDenied => "access_denied",
            Self::NotAFile => "not_a_file",
            Self::TimedOut => "timed_out",
            Self::Cancelled => "cancelled",
            Self::Other => "other",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        [Self::NotFound, Self::AccessDenied, Self::NotAFile, Self::TimedOut, Self::Cancelled, Self::Other]
            .into_iter()
            .find(|k| k.as_str() == text)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{kind:?}: {message}")]
pub struct InspectError {
    pub kind: InspectErrorKind,
    pub message: String,
}

/// The last attempt failed. `attempts` counts consecutive failures and sets the back-off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityFailure {
    pub kind: InspectErrorKind,
    pub message: String,
    pub attempts: u32,
}

/// What is stored for one executable. A later failure keeps the facts from the last good read (an uninstalled program is
/// still what it was).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityRecord {
    pub exe: Executable,
    pub facts: Option<FileFacts>,
    /// When `facts` were measured.
    pub inspected_at: Option<Timestamp>,
    /// When the file was last looked at, successfully or not.
    pub last_attempt: Timestamp,
    pub failure: Option<IdentityFailure>,
}

/// One executable SYNTRAC has seen, with its identity if it has one yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityEntry {
    pub exe: Executable,
    /// When the program was first and last seen running (the permanent ledger).
    pub seen: Option<LedgerEntry>,
    pub record: Option<IdentityRecord>,
}

/// Reads facts from files. The adapter for Windows lives in the collector crate; tests use a fake.
pub trait FileInspector {
    /// The size and time of the file: cheap, no content is read.
    fn stamp(&self, path: &str) -> Result<FileStamp, InspectError>;

    /// Reads the file: hash, signature, version resource, mark of the web.
    fn inspect(&self, path: &str) -> Result<FileFacts, InspectError>;
}

/// Where identities are kept (permanent: detail retention never touches them).
pub trait IdentityRepository {
    /// Every executable the rollups know (the unattributed row is not one), with its identity if it has one.
    fn identity_entries(&self) -> Result<Vec<IdentityEntry>, crate::ports::RepoError>;

    /// Stores (replaces) the identity of an executable. `NotFound` if SYNTRAC has never seen it.
    fn save_identity(&mut self, record: &IdentityRecord) -> Result<(), crate::ports::RepoError>;

    /// The identity of one executable; the path is matched without regard to case.
    fn identity(&self, path: &str) -> Result<Option<IdentityRecord>, crate::ports::RepoError>;
}
