//! The digital signature of a file, read through Windows' own trust provider (`WinVerifyTrust`), OFFLINE.
//!
//! Two kinds of signature are tried, in this order: one embedded in the file, then (for files that carry none, which is most
//! of Windows itself) one in a Windows catalog found by the file's hash.
//!
//! Nothing here uses the network: revocation checking is off and URL retrieval is cache-only, so a certificate chain that
//! needs a missing intermediate certificate is reported as untrusted rather than fetched. This was a decision of the user
//! (docs/IDENTITY-PLAN.md, decision 3). The file is passed by the SAME open handle the hash was computed from.

use std::fs::File;
use std::io::{Seek, SeekFrom};
use std::os::windows::io::AsRawHandle;

use syntrac_core::{InspectError, InspectErrorKind, Signature, SignatureKind, SignatureStatus};
use windows::core::{GUID, PCWSTR};
use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::Security::Cryptography::Catalog::{
    CryptCATAdminAcquireContext2, CryptCATAdminCalcHashFromFileHandle2, CryptCATAdminEnumCatalogFromHash,
    CryptCATAdminReleaseCatalogContext, CryptCATAdminReleaseContext, CryptCATCatalogInfoFromContext, CATALOG_INFO,
};
use windows::Win32::Security::Cryptography::{
    CertGetCertificateContextProperty, CertGetNameStringW, CERT_CONTEXT, CERT_NAME_SIMPLE_DISPLAY_TYPE,
    CERT_SHA1_HASH_PROP_ID,
};
use windows::Win32::Security::WinTrust::{
    WTHelperGetProvCertFromChain, WTHelperGetProvSignerFromChain, WTHelperProvDataFromStateData, WinVerifyTrust,
    WINTRUST_CATALOG_INFO, WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_DATA_PROVIDER_FLAGS, WINTRUST_FILE_INFO,
    WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_CATALOG, WTD_CHOICE_FILE, WTD_REVOCATION_CHECK_NONE, WTD_REVOKE_NONE,
    WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UI_NONE,
};

use crate::file_parse::{sanitize, thumbprint_hex, trust_outcome, TrustOutcome};

/// `WINTRUST_ACTION_GENERIC_VERIFY_V2`: the standard policy for code signatures.
const GENERIC_VERIFY_V2: GUID = GUID::from_u128(0x00aac56b_cd44_11d0_8cc2_00c04fc295ee);

fn failure(message: String) -> InspectError {
    InspectError { kind: InspectErrorKind::Other, message }
}

/// What a verification produced: Windows' verdict and, when the file is signed at all, who signed it.
struct Verified {
    outcome: TrustOutcome,
    signer: Option<String>,
    thumbprint: Option<String>,
}

/// Reads the signature of `file` (opened from `path`). `Unsigned` is a normal answer, not an error.
pub fn read_signature(path: &str, file: &mut File) -> Result<Signature, InspectError> {
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let handle = HANDLE(file.as_raw_handle() as isize);

    let embedded = verify_embedded(&wide, handle);
    if embedded.outcome != TrustOutcome::NoSignature {
        return signature_of(embedded, SignatureKind::Embedded);
    }
    match verify_in_catalog(&wide, handle, file)? {
        Some(catalog) => signature_of(catalog, SignatureKind::Catalog),
        None => Ok(Signature::default()),
    }
}

fn signature_of(verified: Verified, kind: SignatureKind) -> Result<Signature, InspectError> {
    let status = match verified.outcome {
        TrustOutcome::Valid => SignatureStatus::Valid,
        TrustOutcome::Altered => SignatureStatus::Altered,
        TrustOutcome::Untrusted => SignatureStatus::Untrusted,
        TrustOutcome::NoSignature => return Ok(Signature::default()),
        TrustOutcome::Failed(code) => return Err(failure(format!("the signature check failed ({code:#010x})"))),
    };
    Ok(Signature { status, signer: verified.signer, thumbprint: verified.thumbprint, kind: Some(kind) })
}

/// The settings every verification uses: no window, no revocation check, no network.
fn offline_data() -> WINTRUST_DATA {
    WINTRUST_DATA {
        cbStruct: std::mem::size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwStateAction: WTD_STATEACTION_VERIFY,
        dwProvFlags: WINTRUST_DATA_PROVIDER_FLAGS(WTD_CACHE_ONLY_URL_RETRIEVAL.0 | WTD_REVOCATION_CHECK_NONE.0),
        ..Default::default()
    }
}

/// Runs `WinVerifyTrust` on prepared data, reads the signer from the state it leaves, and ALWAYS closes that state.
fn run_verification(data: &mut WINTRUST_DATA) -> Verified {
    let mut action = GENERIC_VERIFY_V2;
    // SAFETY: `data` is fully initialised by the callers and its pointers refer to values that outlive this call; `action` is
    // a live local. The "no window" handle is INVALID_HANDLE_VALUE, as documented.
    let result = unsafe { WinVerifyTrust(HWND(-1), &mut action, std::ptr::from_mut(data).cast()) };
    let outcome = trust_outcome(result);
    let (signer, thumbprint) = if matches!(outcome, TrustOutcome::NoSignature | TrustOutcome::Failed(_)) {
        (None, None)
    } else {
        // SAFETY: the state handle was just returned by a successful VERIFY and is closed below, after the last use.
        unsafe { signer_of_state(data) }
    };
    data.dwStateAction = WTD_STATEACTION_CLOSE;
    // SAFETY: closes the state opened above; this must happen exactly once.
    unsafe { WinVerifyTrust(HWND(-1), &mut action, std::ptr::from_mut(data).cast()) };
    Verified { outcome, signer, thumbprint }
}

/// # Safety
/// `data.hWVTStateData` must be the state of a verification that has not been closed yet.
unsafe fn signer_of_state(data: &WINTRUST_DATA) -> (Option<String>, Option<String>) {
    let provider = unsafe { WTHelperProvDataFromStateData(data.hWVTStateData) };
    if provider.is_null() || unsafe { (*provider).csSigners } == 0 {
        return (None, None);
    }
    let signer = unsafe { WTHelperGetProvSignerFromChain(provider, 0, false, 0) };
    if signer.is_null() {
        return (None, None);
    }
    let cert = unsafe { WTHelperGetProvCertFromChain(signer, 0) };
    if cert.is_null() || unsafe { (*cert).pCert }.is_null() {
        return (None, None);
    }
    let context = unsafe { (*cert).pCert };
    (unsafe { certificate_name(context) }, unsafe { certificate_thumbprint(context) })
}

/// # Safety
/// `context` must point to a live certificate context.
unsafe fn certificate_name(context: *const CERT_CONTEXT) -> Option<String> {
    let mut buffer = [0_u16; 512];
    let written = unsafe { CertGetNameStringW(context, CERT_NAME_SIMPLE_DISPLAY_TYPE, 0, None, Some(&mut buffer)) };
    // The count includes the terminating zero; 1 means an empty name.
    let length = (written as usize).saturating_sub(1).min(buffer.len());
    sanitize(&String::from_utf16_lossy(&buffer[..length]))
}

/// # Safety
/// `context` must point to a live certificate context.
unsafe fn certificate_thumbprint(context: *const CERT_CONTEXT) -> Option<String> {
    let mut hash = [0_u8; 20];
    let mut size = hash.len() as u32;
    unsafe {
        CertGetCertificateContextProperty(context, CERT_SHA1_HASH_PROP_ID, Some(hash.as_mut_ptr().cast()), &mut size)
    }
    .ok()?;
    Some(thumbprint_hex(&hash[..(size as usize).min(hash.len())]))
}

fn verify_embedded(path: &[u16], handle: HANDLE) -> Verified {
    let mut file_info = WINTRUST_FILE_INFO {
        cbStruct: std::mem::size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: PCWSTR(path.as_ptr()),
        hFile: handle,
        ..Default::default()
    };
    let mut data = offline_data();
    data.dwUnionChoice = WTD_CHOICE_FILE;
    data.Anonymous = WINTRUST_DATA_0 { pFile: &mut file_info };
    run_verification(&mut data)
}

/// Releases a catalog administrator context when it goes out of scope, however the function ends.
struct CatalogAdmin(isize);

impl Drop for CatalogAdmin {
    fn drop(&mut self) {
        // SAFETY: the context was acquired by `CryptCATAdminAcquireContext2` and is released once.
        unsafe {
            let _ = CryptCATAdminReleaseContext(self.0, 0);
        }
    }
}

/// Releases a catalog context found by hash.
struct CatalogContext {
    admin: isize,
    info: isize,
}

impl Drop for CatalogContext {
    fn drop(&mut self) {
        // SAFETY: `info` came from `CryptCATAdminEnumCatalogFromHash` on this admin context and is released once.
        unsafe {
            let _ = CryptCATAdminReleaseCatalogContext(self.admin, self.info, 0);
        }
    }
}

/// Looks the file up in the Windows catalogs by its hash (SHA-256 first; older catalogs only know SHA-1) and verifies it
/// against the catalog it finds. `Ok(None)`: no catalog covers this file.
fn verify_in_catalog(path: &[u16], handle: HANDLE, file: &mut File) -> Result<Option<Verified>, InspectError> {
    for algorithm in ["SHA256", "SHA1"] {
        let wide_algorithm: Vec<u16> = algorithm.encode_utf16().chain(std::iter::once(0)).collect();
        let mut admin = 0_isize;
        // SAFETY: `admin` is a live local; the algorithm name is a zero-terminated wide string that outlives the call.
        let acquired =
            unsafe { CryptCATAdminAcquireContext2(&mut admin, None, PCWSTR(wide_algorithm.as_ptr()), None, 0) };
        if acquired.is_err() {
            continue;
        }
        let admin = CatalogAdmin(admin);
        file.seek(SeekFrom::Start(0)).map_err(|e| failure(format!("could not rewind the file: {e}")))?;

        let mut size = 0_u32;
        // SAFETY: the first call only asks for the size of the hash.
        let sized = unsafe { CryptCATAdminCalcHashFromFileHandle2(admin.0, handle, &mut size, None, 0) };
        if sized.is_err() || size == 0 || size > 64 {
            continue;
        }
        let mut hash = vec![0_u8; size as usize];
        // SAFETY: `hash` has exactly `size` bytes.
        let hashed =
            unsafe { CryptCATAdminCalcHashFromFileHandle2(admin.0, handle, &mut size, Some(hash.as_mut_ptr()), 0) };
        if hashed.is_err() {
            continue;
        }
        hash.truncate(size as usize);

        // SAFETY: `admin.0` is a live context and `hash` is its input.
        let found = unsafe { CryptCATAdminEnumCatalogFromHash(admin.0, &hash, 0, None) };
        if found == 0 {
            continue;
        }
        let context = CatalogContext { admin: admin.0, info: found };
        let mut catalog = CATALOG_INFO { cbStruct: std::mem::size_of::<CATALOG_INFO>() as u32, ..Default::default() };
        // SAFETY: `context.info` is live and `catalog` is a properly sized output structure.
        if unsafe { CryptCATCatalogInfoFromContext(context.info, &mut catalog, 0) }.is_err() {
            continue;
        }
        return Ok(Some(verify_catalog_member(path, handle, &admin, &mut catalog, &mut hash)));
    }
    Ok(None)
}

fn verify_catalog_member(
    path: &[u16],
    handle: HANDLE,
    admin: &CatalogAdmin,
    catalog: &mut CATALOG_INFO,
    hash: &mut [u8],
) -> Verified {
    // The member tag the catalog knows the file by is the hash in upper-case hex, as a wide string.
    let tag: Vec<u16> = thumbprint_hex(hash).encode_utf16().chain(std::iter::once(0)).collect();
    let mut info = WINTRUST_CATALOG_INFO {
        cbStruct: std::mem::size_of::<WINTRUST_CATALOG_INFO>() as u32,
        pcwszCatalogFilePath: PCWSTR(catalog.wszCatalogFile.as_ptr()),
        pcwszMemberTag: PCWSTR(tag.as_ptr()),
        pcwszMemberFilePath: PCWSTR(path.as_ptr()),
        hMemberFile: handle,
        pbCalculatedFileHash: hash.as_mut_ptr(),
        cbCalculatedFileHash: hash.len() as u32,
        hCatAdmin: admin.0,
        ..Default::default()
    };
    let mut data = offline_data();
    data.dwUnionChoice = WTD_CHOICE_CATALOG;
    data.Anonymous = WINTRUST_DATA_0 { pCatalog: &mut info };
    run_verification(&mut data)
}
