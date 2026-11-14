//! `FileInspector` for Windows: what the machine can tell us about an executable file.
//!
//! The service runs as SYSTEM and the files are not trusted, so the rules are strict:
//! - only an absolute path on a local drive is read (`file_parse::precheck_path`), and only a regular file: a symbolic link,
//!   junction or other reparse point is refused, never followed;
//! - the file is opened ONCE, read-only and with full sharing (so a running program, or its updater replacing or deleting the
//!   file, is never blocked), and the hash and the signature check use that same handle;
//! - the hash is read in chunks and stops when the service is stopping; a file over the size cap is not hashed;
//! - nothing leaves the machine (see `file_trust`).

use std::fs::{File, Metadata, OpenOptions};
use std::io::Seek;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use syntrac_core::{FileFacts, FileInspector, FileStamp, InspectError, InspectErrorKind, Timestamp};

use crate::file_hash::{sha256_hex, HashError};
use crate::file_parse::{parse_zone_id, precheck_path};
use crate::file_trust::read_signature;
use crate::file_version::read_version;

const FILE_SHARE_ALL: u32 = 0x1 | 0x2 | 0x4; // read, write and delete
const FILE_FLAG_SEQUENTIAL_SCAN: u32 = 0x0800_0000;
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
/// The mark of the web is a few dozen bytes; anything bigger is not one.
const MAX_ZONE_STREAM_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InspectorSettings {
    /// A file larger than this is not hashed (its size is still recorded).
    pub max_hash_bytes: u64,
}

impl Default for InspectorSettings {
    fn default() -> Self {
        Self { max_hash_bytes: 512 * 1024 * 1024 }
    }
}

#[derive(Debug, Clone)]
pub struct WindowsFileInspector {
    settings: InspectorSettings,
    /// Set when the service is stopping: a hash in progress ends at the next chunk.
    stop: Arc<AtomicBool>,
}

impl Default for WindowsFileInspector {
    fn default() -> Self {
        Self::new(InspectorSettings::default(), Arc::new(AtomicBool::new(false)))
    }
}

impl WindowsFileInspector {
    pub fn new(settings: InspectorSettings, stop: Arc<AtomicBool>) -> Self {
        Self { settings, stop }
    }
}

fn error(kind: InspectErrorKind, message: impl Into<String>) -> InspectError {
    InspectError { kind, message: message.into() }
}

/// The service is stopping. Not a verdict on the file: the pass drops it without recording anything.
fn stopped() -> InspectError {
    error(InspectErrorKind::Cancelled, "the service is stopping")
}

/// The operating system's own error number decides the kind: a file that is gone, a file we may not read, anything else.
fn from_io(failure: &std::io::Error) -> InspectError {
    const NOT_FOUND: [i32; 2] = [2, 3]; // file not found, path not found
    const DENIED: [i32; 4] = [5, 32, 33, 1920]; // access denied, sharing violation, lock violation, cannot access
    let kind = match failure.raw_os_error() {
        Some(code) if NOT_FOUND.contains(&code) => InspectErrorKind::NotFound,
        Some(code) if DENIED.contains(&code) => InspectErrorKind::AccessDenied,
        _ => InspectErrorKind::Other,
    };
    error(kind, failure.to_string())
}

/// Opens a regular file for reading, without following a link and without blocking anyone else.
fn open_regular(path: &str) -> Result<(File, Metadata), InspectError> {
    precheck_path(path)?;
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_ALL)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_SEQUENTIAL_SCAN)
        .open(path.trim())
        .map_err(|e| {
            let found = from_io(&e);
            // Windows refuses to open a folder as a file with "access denied"; say what it is instead.
            let is_folder = found.kind == InspectErrorKind::AccessDenied
                && std::fs::symlink_metadata(path.trim()).is_ok_and(|m| m.is_dir());
            if is_folder {
                error(InspectErrorKind::NotAFile, "a folder, not a file")
            } else {
                found
            }
        })?;
    let metadata = file.metadata().map_err(|e| from_io(&e))?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(error(InspectErrorKind::NotAFile, "a link or other reparse point is not followed"));
    }
    if !metadata.is_file() {
        return Err(error(InspectErrorKind::NotAFile, "not a regular file"));
    }
    Ok((file, metadata))
}

/// The ONE place a stamp is made, for both the cheap check and the full read, so they can never disagree.
fn stamp_of(metadata: &Metadata) -> FileStamp {
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .and_then(|since| i64::try_from(since.as_micros()).ok())
        .map(Timestamp::from_micros);
    FileStamp { size: metadata.len(), modified }
}

/// The Zone.Identifier alternate data stream, if the file has one.
fn read_zone(path: &str) -> Option<u32> {
    let stream = std::fs::File::open(format!("{}:Zone.Identifier", path.trim())).ok()?;
    if stream.metadata().ok()?.len() > MAX_ZONE_STREAM_BYTES {
        return None;
    }
    let text = std::io::read_to_string(&stream).ok()?;
    parse_zone_id(&text)
}

impl FileInspector for WindowsFileInspector {
    fn stamp(&self, path: &str) -> Result<FileStamp, InspectError> {
        let (_file, metadata) = open_regular(path)?;
        Ok(stamp_of(&metadata))
    }

    fn inspect(&self, path: &str) -> Result<FileFacts, InspectError> {
        // Checked before anything is opened, so a stopping service starts no new file, whatever its size.
        if self.stop.load(Ordering::SeqCst) {
            return Err(stopped());
        }
        let (mut file, metadata) = open_regular(path)?;
        let stamp = stamp_of(&metadata);

        let sha256 = if stamp.size <= self.settings.max_hash_bytes {
            let hash = sha256_hex(&mut file, || self.stop.load(Ordering::SeqCst)).map_err(|e| match e {
                HashError::Stopped => stopped(),
                HashError::Io(io) => from_io(&io),
            })?;
            file.rewind().map_err(|e| from_io(&e))?;
            Some(hash)
        } else {
            None
        };

        let signature = read_signature(path.trim(), &mut file)?;
        Ok(FileFacts { stamp, sha256, signature, version: read_version(path.trim()), zone_id: read_zone(path) })
    }
}
