//! Which folders an ordinary program cannot write to (docs/ALERTS-PLAN.md, F6), and which writable folders may be a "build folder".

use syntrac_core::{
    build_folder_problem, is_protected_folder_on_system_drive, is_protected_on_system_drive, is_protected_path,
};

#[test]
fn windows_and_program_files_are_protected() {
    for path in [
        r"C:\Windows\System32\svchost.exe",
        r"C:\Windows\explorer.exe",
        r"C:\Windows\System32\spool\drivers\x64\3\driver.exe",
        r"C:\Program Files\Acme\acme.exe",
        r"C:\Program Files (x86)\Acme\acme.exe",
        r"c:/program files/acme/acme.exe",
    ] {
        assert!(is_protected_path(path), "{path}");
    }
}

#[test]
fn folders_inside_windows_that_ordinary_users_can_write_to_are_not_protected() {
    // Found with `icacls` on a real machine: users may create files in each of these.
    for path in [
        r"C:\Windows\Temp\setup.exe",
        r"C:\Windows\Tasks\x.exe",
        r"C:\Windows\Tracing\x.exe",
        r"C:\Windows\System32\Tasks\Microsoft\x.exe",
        r"C:\Windows\SysWOW64\Tasks\x.exe",
        r"C:\Windows\System32\spool\drivers\color\x.exe",
        r"C:\Windows\Registration\CRMLog\x.exe",
        r"C:\Windows\Debug\WIA\x.exe",
        r"C:\Windows\System32\Microsoft\Crypto\RSA\MachineKeys\x.exe",
        r"C:\Windows\System32\spool\PRINTERS\x.exe",
        r"C:\WINDOWS\SYSTEM32\TASKS\x.exe",
    ] {
        assert!(!is_protected_path(path), "{path}");
    }
    // A folder merely named like one of them elsewhere is still protected.
    assert!(is_protected_path(r"C:\Windows\System32\Tasks2\x.exe"));
    assert!(is_protected_path(r"C:\Program Files\Acme\Tasks\x.exe"));
}

#[test]
fn everything_else_is_writable() {
    for path in [
        r"C:\Users\alice\.cargo\bin\cargo.exe",
        r"C:\Users\alice\Downloads\setup.exe",
        r"C:\Users\alice\AppData\Local\Temp\x.exe",
        r"C:\ProgramData\Acme\acme.exe",
        r"D:\Tools\tool.exe",
        r"\\server\share\tool.exe",
        r"tool.exe",
        r"relative\tool.exe",
    ] {
        assert!(!is_protected_path(path), "{path}");
    }
}

#[test]
fn only_the_windows_drive_counts_as_protected_for_creating_an_expectation() {
    assert!(is_protected_on_system_drive(r"C:\Program Files\Acme\acme.exe", "C:"));
    assert!(is_protected_on_system_drive(r"c:\windows\system32\a.exe", "c:"));
    // A data drive's "Program Files" is judged by its own permissions, which the path cannot show (on one drive of the
    // test machine every signed-in user may modify it).
    assert!(!is_protected_on_system_drive(r"D:\Program Files\Acme\acme.exe", "C:"));
    assert!(!is_protected_on_system_drive(r"J:\Windows\System32\a.exe", "C:"));
    // The drive may be written several ways.
    for drive in ["C:", "c:", "C", "c", r"C:\"] {
        assert!(is_protected_on_system_drive(r"c:\program files\a\a.exe", drive), "{drive}");
    }
    assert!(!is_protected_on_system_drive(r"c:\program files\a\a.exe", ""));
    assert!(!is_protected_on_system_drive(r"c:\program files\a\a.exe", "not a drive"));
}

#[test]
fn a_folder_is_judged_like_a_file_inside_it() {
    assert!(is_protected_folder_on_system_drive(r"c:\windows", "C:"));
    assert!(is_protected_folder_on_system_drive(r"c:\windows\system32", "C:"));
    assert!(is_protected_folder_on_system_drive(r"c:\program files\acme", "C:"));
    assert!(is_protected_folder_on_system_drive(r"C:\Program Files\Acme\", "C:"), "a trailing separator is ignored");
    assert!(!is_protected_folder_on_system_drive(r"c:\windows\temp", "C:"));
    assert!(!is_protected_folder_on_system_drive(r"c:\windows\tasks", "C:"));
    assert!(!is_protected_folder_on_system_drive(r"c:\users\alice\.cargo", "C:"));
    assert!(!is_protected_folder_on_system_drive(r"d:\program files\acme", "C:"));
    assert!(!is_protected_folder_on_system_drive(r"c:\", "C:"));
}

#[test]
fn a_build_folder_is_a_tool_or_project_folder_a_user_can_write_to() {
    for root in [
        r"c:\users\alice\.cargo",
        r"c:\users\alice\.rustup",
        r"c:\users\alice\appdata\local\pnpm",
        r"c:\users\alice\appdata\local\programs\someapp",
        r"d:\src",
        r"e:\work",
        r"d:\program files\acme",
        r"c:\programdata\acme",
    ] {
        assert_eq!(build_folder_problem(root, "C:"), None, "{root}");
    }
}

#[test]
fn a_build_folder_is_never_a_place_things_are_dropped_or_a_place_too_general() {
    for (root, reason) in [
        (r"c:\users\alice\appdata\local\temp\toolx", "temporary"),
        (r"c:\users\alice\appdata\local\temp", "temporary"),
        (r"c:\windows\temp", "temporary"),
        (r"c:\users\alice\downloads", "too general"),
        (r"c:\users\alice\Desktop", "too general"),
        (r"c:\users\alice\documents", "too general"),
        (r"c:\users\alice", "profile"),
        (r"c:\users\public", "profile"),
        (r"c:\users\alice\appdata\local", "too general"),
        (r"c:\users\alice\appdata\roaming", "too general"),
        (r"c:\users\alice\appdata", "too general"),
        // Vendor folders everyone's programs share (the Startup folder lives under Microsoft), packages included.
        (r"c:\users\alice\appdata\roaming\microsoft", "too general"),
        (r"c:\users\alice\appdata\local\microsoft", "too general"),
        (r"c:\users\alice\appdata\local\packages", "too general"),
        (r"c:\programdata\microsoft", "too general"),
        (r"c:\programdata", "too general"),
        (r"d:\downloads", "too general"),
        (r"c:\", "drive"),
        (r"c:", "drive"),
        (r"\\server\share\tools", "network"),
        (r"c:\windows\tasks", "Windows"),
        (r"c:\windows\system32", "Windows"),
        (r"c:\program files\acme", "protected"),
        (r"tools", "absolute"),
        (r"", "absolute"),
    ] {
        let problem = build_folder_problem(root, "C:").unwrap_or_else(|| panic!("{root} should be refused"));
        assert!(problem.to_lowercase().contains(&reason.to_lowercase()), "{root}: {problem}");
    }
}
