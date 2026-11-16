//! Turning the text a startup entry holds (a service's binary path, a task's command, a Run value) into the program it starts.
//! Pure: the environment and the file system are passed in. This is where the surprises are, so every shape the real machine
//! showed (docs/RUNAS-PLAN.md, I4b step 0) is here.

use std::collections::HashSet;

use syntrac_core::launch_target::{launch_target, LaunchTarget};

fn env(name: &str) -> Option<String> {
    match name.to_ascii_lowercase().as_str() {
        "systemroot" | "windir" => Some(r"C:\WINDOWS".to_owned()),
        "programfiles" => Some(r"C:\Program Files".to_owned()),
        "programfiles(x86)" => Some(r"C:\Program Files (x86)".to_owned()),
        "programdata" => Some(r"C:\ProgramData".to_owned()),
        _ => None,
    }
}

/// A file system that holds exactly these (lower-case compared).
fn files(paths: &[&str]) -> impl Fn(&str) -> bool {
    let set: HashSet<String> = paths.iter().map(|p| p.to_lowercase()).collect();
    move |path| set.contains(&path.to_lowercase())
}

fn target(command: &str, existing: &[&str]) -> Option<LaunchTarget> {
    launch_target(command, &env, &files(existing))
}

fn path_of(command: &str, existing: &[&str]) -> Option<String> {
    target(command, existing).map(|t| t.path)
}

#[test]
fn a_quoted_path_with_arguments_gives_the_program_and_the_arguments() {
    let t = target(r#""C:\Program Files\Acme\acme.exe" --silent /x"#, &[r"C:\Program Files\Acme\acme.exe"]).unwrap();

    assert_eq!(t.path, r"C:\Program Files\Acme\acme.exe");
    assert_eq!(t.arguments, "--silent /x");
    assert!(t.exists && !t.proxy);
}

#[test]
fn an_unquoted_path_with_spaces_is_found_by_asking_the_file_system() {
    let existing = [r"C:\Program Files (x86)\Acme\acmeserv.exe"];

    let t = target(r"C:\Program Files (x86)\Acme\acmeserv.exe -noconsole", &existing).unwrap();

    assert_eq!(t.path, r"C:\Program Files (x86)\Acme\acmeserv.exe");
    assert_eq!(t.arguments, "-noconsole");
    assert!(t.exists);
}

#[test]
fn an_unquoted_path_that_no_longer_exists_still_names_the_first_exe_it_can() {
    let t = target(r"C:\Program Files\Gone\gone.exe /s", &[]).unwrap();

    assert_eq!(t.path, r"C:\Program Files\Gone\gone.exe");
    assert!(!t.exists, "the entry is kept, and says the file is not there");
}

#[test]
fn the_shortest_path_that_exists_wins_as_windows_itself_would_choose() {
    let existing = [r"C:\Tools\a.exe", r"C:\Tools\a.exe b\c.exe"];

    assert_eq!(path_of(r"C:\Tools\a.exe b\c.exe", &existing).as_deref(), Some(r"C:\Tools\a.exe"));
}

#[test]
fn environment_variables_are_expanded_in_any_case_and_an_unknown_one_means_unresolved() {
    assert_eq!(
        path_of(r"%windir%\system32\svchost.exe -k netsvcs -p", &[r"C:\WINDOWS\system32\svchost.exe"]).as_deref(),
        Some(r"C:\WINDOWS\system32\svchost.exe")
    );
    assert_eq!(path_of(r"%ProgramFiles(x86)%\X\x.exe", &[]).as_deref(), Some(r"C:\Program Files (x86)\X\x.exe"));
    assert_eq!(path_of(r"%NOSUCHVAR%\x.exe", &[]), None);
}

#[test]
fn an_unknown_variable_in_the_arguments_does_not_spoil_the_program() {
    let t = target(r#""C:\a\b.exe" --dir %UNKNOWN%\z"#, &[]).unwrap();

    assert_eq!(t.path, r"C:\a\b.exe");
    assert_eq!(t.arguments, r"--dir %UNKNOWN%\z", "arguments are shown as they are written");
}

#[test]
fn the_device_prefixes_and_systemroot_spelling_are_understood() {
    assert_eq!(path_of(r"\??\C:\WINDOWS\system32\a.exe", &[]).as_deref(), Some(r"C:\WINDOWS\system32\a.exe"));
    assert_eq!(path_of(r"\\?\C:\Apps\b.exe", &[]).as_deref(), Some(r"C:\Apps\b.exe"));
    assert_eq!(path_of(r"\SystemRoot\system32\c.exe", &[]).as_deref(), Some(r"C:\WINDOWS\system32\c.exe"));
}

#[test]
fn forward_slashes_and_mixed_case_are_accepted() {
    assert_eq!(path_of("C:/Apps/Tool.EXE /q", &[]).as_deref(), Some(r"C:\Apps\Tool.EXE"));
}

#[test]
fn a_bare_name_is_looked_for_in_the_system_folder_and_is_unresolved_if_it_is_not_there() {
    assert_eq!(
        path_of("cmd.exe /c dir", &[r"C:\WINDOWS\System32\cmd.exe"]).as_deref(),
        Some(r"C:\WINDOWS\System32\cmd.exe")
    );
    assert_eq!(
        path_of("notepad", &[r"C:\WINDOWS\System32\notepad.exe"]).as_deref(),
        Some(r"C:\WINDOWS\System32\notepad.exe")
    );
    assert_eq!(path_of("mystery.exe", &[]), None);
}

#[test]
fn a_quoted_path_without_an_extension_gets_exe_only_if_that_file_exists() {
    assert_eq!(path_of(r#""C:\Apps\tool""#, &[r"C:\Apps\tool.exe"]).as_deref(), Some(r"C:\Apps\tool.exe"));
    assert_eq!(path_of(r#""C:\Apps\tool""#, &[]), None);
}

#[test]
fn proxies_are_recorded_as_themselves_with_their_arguments() {
    let t = target(r"C:\WINDOWS\system32\rundll32.exe C:\WINDOWS\system32\shell32.dll,Control_RunDLL", &[]).unwrap();
    assert!(t.proxy);
    assert_eq!(t.path, r"C:\WINDOWS\system32\rundll32.exe");
    assert_eq!(t.arguments, r"C:\WINDOWS\system32\shell32.dll,Control_RunDLL");

    for proxy in ["cmd", "powershell", "pwsh", "wscript", "cscript", "mshta", "msiexec", "regsvr32"] {
        let command = format!(r#""C:\WINDOWS\System32\{proxy}.exe" /x"#);
        assert!(target(&command, &[]).unwrap().proxy, "{proxy}");
    }
    assert!(!target(r"C:\Apps\notepad.exe", &[]).unwrap().proxy);
}

#[test]
fn nothing_usable_is_unresolved_never_a_panic() {
    for command in [
        "",
        "   ",
        "\"\"",
        "\"",
        "%",
        "%%",
        "/c something",
        r"\\",
        "C:\\",
        "no extension at all",
        "\"unterminated C:\\a.exe",
    ] {
        let _ = target(command, &[]);
    }
    assert_eq!(path_of("", &[]), None);
    assert_eq!(path_of("\"\"", &[]), None);
    assert_eq!(path_of("   ", &[]), None);
    assert_eq!(path_of("no extension at all", &[]), None);
}

#[test]
fn a_huge_value_is_bounded() {
    let long = format!(r#""C:\a\b.exe" {}"#, "x".repeat(50_000));

    let t = target(&long, &[]).unwrap();

    assert_eq!(t.path, r"C:\a\b.exe");
    assert!(t.arguments.chars().count() <= 400);
}

#[test]
fn control_and_direction_characters_are_removed_from_what_is_kept() {
    let t = target("\"C:\\a\\b.exe\" arg\u{202E}\u{0007}s", &[]).unwrap();

    assert_eq!(t.arguments, "args");
}

#[test]
fn network_paths_are_kept_as_they_are() {
    assert_eq!(path_of(r"\\server\share\tool.exe /q", &[]).as_deref(), Some(r"\\server\share\tool.exe"));
}

#[test]
fn a_batch_file_or_a_script_host_target_is_a_program_too() {
    assert_eq!(path_of(r"C:\Scripts\start.cmd", &[]).as_deref(), Some(r"C:\Scripts\start.cmd"));
    assert_eq!(path_of(r"C:\Scripts\run.bat /a", &[]).as_deref(), Some(r"C:\Scripts\run.bat"));
}
