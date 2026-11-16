//! What raw values from Windows MEAN when reading a file: trust results, text from the file, the mark of the web, and which
//! paths we refuse to open. PURE and unit-tested; the calls that fetch the raw values live in `file_trust`, `file_version` and
//! `file_inspector`.

use syntrac_core::{InspectError, InspectErrorKind};

/// What `WinVerifyTrust` said, reduced to what SYNTRAC records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustOutcome {
    Valid,
    /// The file carries no signature of this kind (it may still be covered by a catalog).
    NoSignature,
    /// Signed, but the file no longer matches its signature.
    Altered,
    /// Signed, but the certificate chain is not trusted (unknown root, expired, revoked, ...).
    Untrusted,
    /// Something went wrong that says nothing about the file (access denied, out of memory, ...).
    Failed(i32),
}

const fn hresult(code: u32) -> i32 {
    code as i32
}

pub fn trust_outcome(result: i32) -> TrustOutcome {
    const NO_SIGNATURE: [i32; 3] = [hresult(0x800B_0100), hresult(0x800B_0003), hresult(0x800B_0001)];
    const BAD_DIGEST: i32 = hresult(0x8009_6010);
    // The CERT_E_* family (expired, chaining, untrusted root, revoked, explicit distrust, ...), plus the other
    // "signed, but not acceptable" results.
    let untrusted_range = hresult(0x800B_0101)..=hresult(0x800B_0112);
    const UNTRUSTED: [i32; 6] = [
        hresult(0x800B_0004),
        hresult(0x8009_6003),
        hresult(0x8009_6004),
        hresult(0x8009_6005),
        hresult(0x8009_6019),
        hresult(0x8009_2026),
    ];

    if result == 0 {
        TrustOutcome::Valid
    } else if NO_SIGNATURE.contains(&result) {
        TrustOutcome::NoSignature
    } else if result == BAD_DIGEST {
        TrustOutcome::Altered
    } else if untrusted_range.contains(&result) || UNTRUSTED.contains(&result) {
        TrustOutcome::Untrusted
    } else {
        TrustOutcome::Failed(result)
    }
}

const MAX_TEXT_CHARS: usize = 256;

/// Text read from a file (signer, company, description...) is attacker-influenced: control characters and direction
/// tricks are removed, ends are trimmed, and the length is capped. `None` when nothing printable is left.
pub fn sanitize(text: &str) -> Option<String> {
    syntrac_core::clean_text(text, MAX_TEXT_CHARS)
}

pub fn thumbprint_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

/// The `ZoneId` of a `Zone.Identifier` stream (3 = Internet, 4 = restricted sites). `None` if there is none.
pub fn parse_zone_id(text: &str) -> Option<u32> {
    text.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        if key.trim().eq_ignore_ascii_case("zoneid") {
            value.trim().parse::<u32>().ok()
        } else {
            None
        }
    })
}

/// Only an absolute path on a local drive is read. Bare names (`vmmemWSL`), relative paths, device paths and network paths
/// are refused BEFORE the disk is touched: the service runs as SYSTEM, and a network path could make it wait on a remote
/// machine or send it credentials.
pub fn precheck_path(path: &str) -> Result<(), InspectError> {
    let trimmed = path.trim();
    let local = trimmed.strip_prefix(r"\\?\").unwrap_or(trimmed);
    let bytes = local.as_bytes();
    let drive_path = bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\';
    if drive_path {
        Ok(())
    } else {
        Err(InspectError {
            kind: InspectErrorKind::NotAFile,
            message: "not an absolute path on a local drive, so it is not read".into(),
        })
    }
}

/// The sub-block of a version resource that holds one named string for a language and code page.
pub fn version_value_path(language: u16, code_page: u16, name: &str) -> String {
    format!(r"\StringFileInfo\{language:04X}{code_page:04X}\{name}")
}

/// The `\VarFileInfo\Translation` table: pairs of (language, code page), little endian. A trailing partial pair is ignored.
pub fn translation_pairs(bytes: &[u8]) -> Vec<(u16, u16)> {
    bytes
        .chunks_exact(4)
        .map(|pair| (u16::from_le_bytes([pair[0], pair[1]]), u16::from_le_bytes([pair[2], pair[3]])))
        .collect()
}
