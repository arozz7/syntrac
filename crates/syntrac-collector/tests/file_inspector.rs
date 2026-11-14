//! `WindowsFileInspector` on real files: ones every Windows install has, files created here, and files made to fail.
//! These talk to the operating system, so they only run on Windows (the whole project does).

use std::fs;
use std::io::Write;
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use syntrac_collector::file_inspector::{InspectorSettings, WindowsFileInspector};
use syntrac_core::{FileInspector, InspectErrorKind, SignatureStatus};

const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

fn inspector() -> WindowsFileInspector {
    WindowsFileInspector::default()
}

fn system_file(name: &str) -> String {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    format!(r"{root}\System32\{name}")
}

fn temp_file(dir: &Path, name: &str, content: &[u8]) -> PathBuf {
    let path = dir.join(name);
    fs::File::create(&path).unwrap().write_all(content).unwrap();
    path
}

#[test]
fn a_windows_file_is_signed_by_microsoft_through_its_catalog() {
    let facts = inspector().inspect(&system_file("cmd.exe")).unwrap();

    assert_eq!(facts.signature.status, SignatureStatus::Valid, "{:?}", facts.signature);
    assert!(facts.signature.signer.as_deref().unwrap_or("").contains("Microsoft"), "{:?}", facts.signature.signer);
    assert_eq!(facts.signature.thumbprint.as_deref().map(str::len), Some(40), "a SHA-1 thumbprint in hex");
    assert_eq!(facts.version.company.as_deref(), Some("Microsoft Corporation"));
    assert!(facts.version.original_filename.as_deref().unwrap_or("").eq_ignore_ascii_case("cmd.exe"));
    assert_eq!(facts.sha256.as_deref().map(str::len), Some(64));
    assert!(facts.stamp.size > 10_000 && facts.stamp.modified.is_some());
}

#[test]
fn an_unchanged_file_has_the_same_stamp_from_the_cheap_check_and_the_full_read() {
    let path = system_file("cmd.exe");

    let cheap = inspector().stamp(&path).unwrap();
    let full = inspector().inspect(&path).unwrap().stamp;

    assert_eq!(cheap, full, "otherwise every recheck would read the whole file again");
}

#[test]
fn a_program_nobody_signed_is_unsigned_and_has_the_right_hash_and_size() {
    let dir = tempfile::tempdir().unwrap();
    let path = temp_file(dir.path(), "tool.exe", b"abc");

    let facts = inspector().inspect(path.to_str().unwrap()).unwrap();

    assert_eq!(facts.signature.status, SignatureStatus::Unsigned);
    assert_eq!((facts.signature.signer, facts.signature.thumbprint, facts.signature.kind), (None, None, None));
    assert_eq!(facts.sha256.as_deref(), Some(ABC_SHA256));
    assert_eq!(facts.stamp.size, 3);
    assert_eq!(facts.version, Default::default(), "a text file has no version resource");
    assert_eq!(facts.zone_id, None);
}

#[test]
fn a_real_but_unsigned_executable_is_unsigned() {
    let me = std::env::current_exe().unwrap();

    let facts = inspector().inspect(me.to_str().unwrap()).unwrap();

    assert_eq!(facts.signature.status, SignatureStatus::Unsigned, "the test program is not signed");
}

#[test]
fn a_downloaded_file_carries_its_zone() {
    let dir = tempfile::tempdir().unwrap();
    let path = temp_file(dir.path(), "setup.exe", b"abc");
    let stream = format!("{}:Zone.Identifier", path.display());
    fs::write(stream, "[ZoneTransfer]\r\nZoneId=3\r\n").unwrap();

    let facts = inspector().inspect(path.to_str().unwrap()).unwrap();

    assert_eq!(facts.zone_id, Some(3));
    assert!(facts.downloaded());
}

#[test]
fn a_file_larger_than_the_cap_is_not_hashed_but_is_still_checked() {
    let dir = tempfile::tempdir().unwrap();
    let path = temp_file(dir.path(), "big.exe", &[1_u8; 100]);
    let small_cap =
        WindowsFileInspector::new(InspectorSettings { max_hash_bytes: 50 }, Arc::new(AtomicBool::new(false)));

    let facts = small_cap.inspect(path.to_str().unwrap()).unwrap();

    assert_eq!((facts.sha256, facts.stamp.size), (None, 100));
    assert_eq!(facts.signature.status, SignatureStatus::Unsigned);
}

#[test]
fn a_stop_request_ends_the_read_with_a_timed_out_error() {
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = WindowsFileInspector::new(InspectorSettings::default(), Arc::clone(&stop));
    stop.store(true, Ordering::SeqCst);

    let error = stopping.inspect(&system_file("cmd.exe")).unwrap_err();

    assert_eq!(error.kind, InspectErrorKind::TimedOut);
}

#[test]
fn things_that_are_not_readable_files_give_errors_not_panics() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("gone.exe");

    assert_eq!(inspector().inspect(missing.to_str().unwrap()).unwrap_err().kind, InspectErrorKind::NotFound);
    assert_eq!(inspector().stamp(missing.to_str().unwrap()).unwrap_err().kind, InspectErrorKind::NotFound);
    assert_eq!(inspector().inspect(dir.path().to_str().unwrap()).unwrap_err().kind, InspectErrorKind::NotAFile);
    assert_eq!(inspector().inspect("vmmemWSL").unwrap_err().kind, InspectErrorKind::NotAFile);
    assert_eq!(
        inspector().inspect(r"\\localhost\c$\Windows\notepad.exe").unwrap_err().kind,
        InspectErrorKind::NotAFile
    );
}

#[test]
fn a_file_another_program_holds_exclusively_is_reported_as_access_denied() {
    let dir = tempfile::tempdir().unwrap();
    let path = temp_file(dir.path(), "locked.exe", b"abc");
    let _holder = fs::OpenOptions::new().read(true).share_mode(0).open(&path).unwrap();

    let error = inspector().inspect(path.to_str().unwrap()).unwrap_err();

    assert_eq!(error.kind, InspectErrorKind::AccessDenied);
}

#[test]
fn a_file_that_is_open_for_writing_elsewhere_can_still_be_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = temp_file(dir.path(), "running.exe", b"abc");
    let _writer = fs::OpenOptions::new().write(true).open(&path).unwrap();

    let facts = inspector().inspect(path.to_str().unwrap()).unwrap();

    assert_eq!(
        facts.sha256.as_deref(),
        Some(ABC_SHA256),
        "reading must not get in the way of the program or its updater"
    );
}

#[test]
fn reading_a_file_does_not_stop_it_being_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let path = temp_file(dir.path(), "update.exe", b"abc");
    inspector().inspect(path.to_str().unwrap()).unwrap();

    // The inspector holds nothing after it returns, so an updater can replace or delete the file.
    fs::remove_file(&path).unwrap();
}

#[test]
fn a_symbolic_link_is_refused_not_followed() {
    let dir = tempfile::tempdir().unwrap();
    let target = temp_file(dir.path(), "real.exe", b"abc");
    let link = dir.path().join("link.exe");
    if std::os::windows::fs::symlink_file(&target, &link).is_err() {
        eprintln!("skipped: creating a symbolic link needs a privilege this account does not have");
        return;
    }

    let error = inspector().inspect(link.to_str().unwrap()).unwrap_err();

    assert_eq!(error.kind, InspectErrorKind::NotAFile);
}

/// Files that carry their own signature (not a catalog) exist on most machines: look for one rather than assume where.
fn embedded_signed_file() -> Option<PathBuf> {
    let candidates = [
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft OneDrive\OneDrive.exe",
        r"C:\Program Files (x86)\Acme\acmeserv.exe",
        r"C:\Program Files\Windows Defender\MpCmdRun.exe",
        r"C:\Windows\System32\OneDriveSetup.exe",
    ];
    candidates.iter().map(PathBuf::from).find(|path| {
        path.exists()
            && inspector().inspect(path.to_str().unwrap()).is_ok_and(|facts| {
                facts.signature.status == SignatureStatus::Valid
                    && facts.signature.kind == Some(syntrac_core::SignatureKind::Embedded)
            })
    })
}

#[test]
fn a_signed_file_with_a_changed_byte_is_reported_as_altered() {
    let Some(original) = embedded_signed_file() else {
        eprintln!("skipped: no file with an embedded signature was found on this machine");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let copy = dir.path().join("copy.exe");
    fs::copy(&original, &copy).unwrap();
    let genuine = inspector().inspect(copy.to_str().unwrap()).unwrap();
    assert_eq!(genuine.signature.status, SignatureStatus::Valid, "the copy is signed like the original");
    assert!(genuine.signature.signer.is_some() && genuine.signature.thumbprint.is_some());

    let mut bytes = fs::read(&copy).unwrap();
    let statuses: Vec<SignatureStatus> = [3, 2, 4]
        .iter()
        .map(|divisor| {
            let mut changed = bytes.clone();
            let at = changed.len() / divisor;
            changed[at] ^= 0xFF;
            fs::write(&copy, &changed).unwrap();
            inspector().inspect(copy.to_str().unwrap()).unwrap().signature.status
        })
        .collect();
    bytes.clear();

    assert!(statuses.contains(&SignatureStatus::Altered), "{statuses:?}");
}

#[test]
fn an_app_execution_alias_is_a_reparse_point_and_is_refused() {
    use std::os::windows::fs::MetadataExt;
    let Some(local) = std::env::var_os("LOCALAPPDATA") else { return };
    let aliases = PathBuf::from(local).join(r"Microsoft\WindowsApps");
    let Some(alias) = fs::read_dir(aliases).ok().into_iter().flatten().flatten().map(|e| e.path()).find(|p| {
        p.extension().is_some_and(|e| e == "exe")
            && fs::symlink_metadata(p).is_ok_and(|m| m.file_attributes() & 0x400 != 0)
    }) else {
        eprintln!("skipped: no app execution alias on this machine");
        return;
    };

    let error = inspector().inspect(alias.to_str().unwrap()).unwrap_err();

    assert_eq!(error.kind, InspectErrorKind::NotAFile, "{alias:?}");
}
