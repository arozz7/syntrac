//! The launch-record sources: scheduled tasks and Startup folders against files in a temporary folder, the services and the
//! registry against the machine running the tests (asserting only what is true on every Windows machine).

use std::path::{Path, PathBuf};

use syntrac_collector::launch_env::{environment, file_exists};
use syntrac_collector::launch_registry::RunKeyLaunchSource;
use syntrac_collector::launch_services::ServiceLaunchSource;
use syntrac_collector::launch_startup::{ComGuard, ShellShortcuts, ShortcutResolver, StartupLaunchSource};
use syntrac_collector::launch_tasks::TaskLaunchSource;
use syntrac_collector::registry::{RegKey, Root};
use syntrac_core::{LaunchSource, LauncherKind};

fn utf16_with_bom(text: &str) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xFE];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

fn task(enabled: bool, body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.4" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <Triggers><LogonTrigger/><BootTrigger/></Triggers>
  <Settings><Enabled>{enabled}</Enabled></Settings>
  <Actions Context="Author">{body}</Actions>
</Task>"#
    )
}

fn write(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

// ---- scheduled tasks

#[test]
fn tasks_are_read_from_utf16_files_in_nested_folders_with_their_triggers() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join(r"Vendor\Updater"),
        &utf16_with_bom(&task(
            true,
            r"<Exec><Command>%ProgramFiles%\Vendor\up.exe</Command><Arguments>/check</Arguments></Exec>",
        )),
    );
    write(&dir.path().join("Plain"), task(true, r#"<Exec><Command>"C:\Tools\plain.exe"</Command></Exec>"#).as_bytes());

    let read = TaskLaunchSource::new(dir.path().to_path_buf()).read().unwrap();

    let mut found: Vec<_> =
        read.entries.iter().map(|e| (e.location.as_str(), e.command.as_str(), e.detail.as_str())).collect();
    found.sort();
    assert_eq!(
        found,
        [
            (r"\Plain", r#""C:\Tools\plain.exe""#, "boot, logon"),
            (r"\Vendor\Updater", r"%ProgramFiles%\Vendor\up.exe /check", "boot, logon"),
        ]
    );
    assert!(read.entries.iter().all(|e| e.kind == LauncherKind::Task && e.profile.is_none()));
    assert_eq!(read.unreadable, 0);
}

#[test]
fn a_switched_off_task_and_a_com_handler_task_have_nothing_to_record_and_a_broken_file_is_counted() {
    let dir = tempfile::tempdir().unwrap();
    write(&dir.path().join("Off"), &utf16_with_bom(&task(false, r"<Exec><Command>C:\a.exe</Command></Exec>")));
    write(&dir.path().join("Com"), &utf16_with_bom(&task(true, "<ComHandler><ClassId>{1}</ClassId></ComHandler>")));
    write(&dir.path().join("Broken"), b"\xFF\xFE<\x00T\x00");
    write(&dir.path().join("Garbage"), b"this is not xml");

    let read = TaskLaunchSource::new(dir.path().to_path_buf()).read().unwrap();

    assert!(read.entries.is_empty());
    assert_eq!(read.unreadable, 2, "the two files that could not be read as tasks");
}

#[test]
fn several_commands_in_one_task_are_several_entries_and_the_folder_missing_is_an_error_not_an_empty_sweep() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("Two"),
        &utf16_with_bom(&task(
            true,
            r"<Exec><Command>C:\a.exe</Command></Exec><Exec><Command>C:\b.exe</Command></Exec>",
        )),
    );
    assert_eq!(TaskLaunchSource::new(dir.path().to_path_buf()).read().unwrap().entries.len(), 2);

    let missing = TaskLaunchSource::new(dir.path().join("nope"));
    assert!(
        missing.read().is_err(),
        "an unreadable task folder must not look like 'no tasks' (that would mark every task as gone)"
    );
}

// ---- startup folders

struct FakeShortcuts;

impl ShortcutResolver for FakeShortcuts {
    fn resolve(&self, shortcut: &Path) -> Option<(String, String)> {
        match shortcut.file_name()?.to_str()? {
            "Sync.lnk" => Some((r"C:\Program Files\Sync\sync.exe".to_owned(), "--hidden".to_owned())),
            "Plain.lnk" => Some((r"C:\Apps\plain.exe".to_owned(), String::new())),
            _ => None,
        }
    }
}

#[test]
fn startup_folders_give_shortcut_targets_and_programs_placed_there_and_ignore_the_rest() {
    let all = tempfile::tempdir().unwrap();
    let user = tempfile::tempdir().unwrap();
    for name in ["Sync.lnk", "Plain.lnk", "Broken.lnk", "desktop.ini", "notes.txt", "run.bat"] {
        write(&all.path().join(name), b"x");
    }
    write(&user.path().join("direct.exe"), b"MZ");
    let source = StartupLaunchSource::new(
        vec![(all.path().to_path_buf(), None), (user.path().to_path_buf(), Some(r"C:\Users\someone".to_owned()))],
        FakeShortcuts,
        false,
    );

    let read = source.read().unwrap();

    let mut found: Vec<(String, String, Option<String>)> =
        read.entries.iter().map(|e| (e.location.clone(), e.command.clone(), e.profile.clone())).collect();
    found.sort();
    let expected: Vec<(String, String, Option<String>)> = vec![
        (r"Startup (all users)\Plain.lnk".to_owned(), r#""C:\Apps\plain.exe""#.to_owned(), None),
        (r"Startup (all users)\Sync.lnk".to_owned(), r#""C:\Program Files\Sync\sync.exe" --hidden"#.to_owned(), None),
        (r"Startup (all users)\run.bat".to_owned(), format!("\"{}\"", all.path().join("run.bat").display()), None),
        (
            r"Startup (user someone)\direct.exe".to_owned(),
            format!("\"{}\"", user.path().join("direct.exe").display()),
            Some(r"C:\Users\someone".to_owned()),
        ),
    ];
    let mut expected = expected;
    expected.sort();
    assert_eq!(found, expected);
    assert_eq!(read.unreadable, 1, "the shortcut that could not be resolved");
}

#[test]
fn a_startup_folder_that_does_not_exist_has_no_entries() {
    let source = StartupLaunchSource::new(vec![(PathBuf::from(r"Z:\nowhere\Startup"), None)], FakeShortcuts, false);

    assert!(source.read().unwrap().entries.is_empty());
}

#[test]
fn the_real_shell_reads_back_the_target_of_a_shortcut_it_wrote() {
    use windows::core::{Interface, PCWSTR};
    use windows::Win32::System::Com::{CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

    let dir = tempfile::tempdir().unwrap();
    let target = std::env::current_exe().unwrap();
    let lnk = dir.path().join("Self.lnk");
    let _com = ComGuard::start();
    let wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() };
    // SAFETY: the wide strings are NUL-terminated and outlive the calls.
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
        link.SetPath(PCWSTR(wide(&target.to_string_lossy()).as_ptr())).unwrap();
        link.SetArguments(PCWSTR(wide("--quiet --x").as_ptr())).unwrap();
        let file: IPersistFile = link.cast().unwrap();
        file.Save(PCWSTR(wide(&lnk.to_string_lossy()).as_ptr()), true).unwrap();
    }

    let (program, arguments) = ShellShortcuts.resolve(&lnk).expect("the shell resolves it");

    assert_eq!(program.to_lowercase(), target.to_string_lossy().to_lowercase());
    assert_eq!(arguments, "--quiet --x");
}

// ---- the machine

#[test]
fn the_services_of_this_machine_are_listed_with_their_commands_and_start_types() {
    let read = ServiceLaunchSource.read().expect("the service manager can be read by any user");

    assert!(read.entries.len() > 50, "a Windows machine has well over fifty services: {}", read.entries.len());
    assert!(read.entries.iter().any(|e| e.kind == LauncherKind::ServiceAuto), "some start automatically");
    assert!(
        read.entries.iter().any(|e| e.command.to_lowercase().contains("svchost.exe")),
        "the shared service host is there"
    );
    assert!(read.entries.iter().all(|e| !e.location.is_empty() && !e.command.is_empty()));
    assert!(read.entries.iter().all(|e| e.profile.is_none()));
}

#[test]
fn the_run_keys_can_be_read_and_every_entry_says_where_it_was_found() {
    let read = RunKeyLaunchSource.read().expect("the machine registry can be read");

    assert!(read.entries.iter().all(|e| e.location.starts_with("HKLM") || e.location.starts_with("HKU")));
    assert!(read.entries.iter().all(|e| matches!(e.kind, LauncherKind::RunKey | LauncherKind::RunOnce)));
    assert!(
        read.entries.iter().filter(|e| e.location.starts_with("HKU")).all(|e| e.profile.is_some()),
        "a user's entry carries the profile"
    );
}

#[test]
fn the_registry_reader_reads_text_values_and_subkeys_and_nothing_that_is_not_there() {
    let key = RegKey::open(Root::LocalMachine, r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
        .expect("exists on every Windows");

    assert!(key.text("ProductName").is_some_and(|name| name.contains("Windows")));
    assert!(key.subkeys().iter().any(|name| name == "ProfileList"));
    assert!(RegKey::open(Root::LocalMachine, r"SOFTWARE\Syntrac\DoesNotExist").is_none());
    assert_eq!(key.text("NoSuchValue"), None);
}

#[test]
fn the_environment_gives_machine_folders_but_never_the_service_account_s_own_user_folders() {
    assert!(environment("SystemRoot").is_some());
    assert!(environment("windir").is_some());
    for name in ["USERPROFILE", "AppData", "LocalAppData", "TEMP", "tmp", "UserName"] {
        assert_eq!(environment(name), None, "{name}");
    }
    assert!(file_exists(&format!(r"{}\System32\cmd.exe", environment("SystemRoot").unwrap())));
    assert!(!file_exists(r"C:\definitely\not\here.exe"));
}

#[test]
fn the_tasks_of_this_machine_are_either_read_or_reported_as_unreadable_never_an_empty_success() {
    // Unelevated, the Task Scheduler folder cannot be listed; the service (SYSTEM) can. Either way, an empty success would make
    // every task look as if it had been removed.
    match TaskLaunchSource::for_this_machine().read() {
        Ok(read) => assert!(!read.entries.is_empty(), "a Windows machine has scheduled tasks that run programs"),
        Err(error) => assert!(error.0.contains("cannot be read") || error.0.contains("not there"), "{error}"),
    }
}
