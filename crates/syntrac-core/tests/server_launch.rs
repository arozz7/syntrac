//! Phase 4, slice L4c (docs/PHASE4-PLAN.md): the rules a launch command must pass before the program may start it.

use syntrac_core::llm::AiSettings;
use syntrac_core::server_launch::{LaunchCommand, LaunchProblem};

fn command(program: &str) -> LaunchCommand {
    LaunchCommand { program: program.into(), args: vec!["serve".into()], working_dir: Some(r"C:\Tools\server".into()) }
}

fn problem(command: &LaunchCommand) -> LaunchProblem {
    command.check().expect_err("refused")
}

#[test]
fn an_ordinary_command_is_accepted() {
    let mut accepted = command(r"C:\Tools\server\srv.exe");
    accepted.args = vec!["serve".into(), "--config".into(), r"C:\a b\c.toml".into()];
    assert_eq!(accepted.check(), Ok(()));
    assert_eq!(command(r"D:/Tools/srv.EXE").check(), Ok(()));
    let mut no_folder = command(r"C:\Program Files\Srv\srv.exe");
    no_folder.working_dir = None;
    assert_eq!(no_folder.check(), Ok(()), "the working folder is optional");
}

#[test]
fn the_program_must_be_a_full_local_path_to_an_exe() {
    assert_eq!(problem(&command("")), LaunchProblem::NoProgram);
    assert_eq!(problem(&command("   ")), LaunchProblem::NoProgram);
    assert_eq!(problem(&command("srv.exe")), LaunchProblem::NotAbsolute);
    assert_eq!(problem(&command(r"tools\srv.exe")), LaunchProblem::NotAbsolute);
    assert_eq!(problem(&command(r"\\host\share\srv.exe")), LaunchProblem::UnsafeFolder);
    assert_eq!(problem(&command(r"C:\Tools\..\Windows\srv.exe")), LaunchProblem::Malformed);
    assert_eq!(problem(&command("C:\\Tools\\sr\"v.exe")), LaunchProblem::Malformed);
    assert_eq!(problem(&command("C:\\Tools\\sr\nv.exe")), LaunchProblem::Malformed);
    // A stray space or line break around the whole path is not part of it.
    assert_eq!(command("  C:\\Tools\\srv.exe\n").check(), Ok(()));
    assert_eq!(command("  C:\\Tools\\srv.exe\n").program_path(), "C:\\Tools\\srv.exe");
}

#[test]
fn only_exe_files_are_started_because_scripts_would_need_a_shell() {
    for name in ["srv.bat", "srv.cmd", "srv.ps1", "srv.js", "srv", "srv.exe.txt"] {
        assert_eq!(problem(&command(&format!(r"C:\Tools\{name}"))), LaunchProblem::NotAnExe, "{name}");
    }
}

#[test]
fn temporary_and_download_folders_are_refused() {
    for path in [
        r"C:\Users\a\AppData\Local\Temp\srv.exe",
        r"C:\Windows\Temp\srv.exe",
        r"C:\Users\a\Downloads\srv.exe",
        r"D:\Downloads\tools\srv.exe",
    ] {
        assert_eq!(problem(&command(path)), LaunchProblem::UnsafeFolder, "{path}");
    }
}

#[test]
fn arguments_are_a_short_list_of_plain_text() {
    let mut many = command(r"C:\Tools\srv.exe");
    many.args = vec!["x".into(); 65];
    assert_eq!(problem(&many), LaunchProblem::BadArgument);
    many.args = vec!["x".repeat(1025)];
    assert_eq!(problem(&many), LaunchProblem::BadArgument);
    many.args = vec!["line\nbreak".into()];
    assert_eq!(problem(&many), LaunchProblem::BadArgument);
    many.args = vec!["nul\0".into()];
    assert_eq!(problem(&many), LaunchProblem::BadArgument);
    many.args = vec!["x".repeat(1024); 64];
    assert_eq!(many.check(), Ok(()), "the limits themselves are allowed");
}

#[test]
fn the_working_folder_must_be_a_full_local_path_too() {
    let mut cmd = command(r"C:\Tools\srv.exe");
    cmd.working_dir = Some("tools".into());
    assert_eq!(problem(&cmd), LaunchProblem::BadWorkingFolder);
    cmd.working_dir = Some(r"C:\Tools\..".into());
    assert_eq!(problem(&cmd), LaunchProblem::BadWorkingFolder);
    cmd.working_dir = Some(r"\\host\share".into());
    assert_eq!(problem(&cmd), LaunchProblem::BadWorkingFolder);
    cmd.working_dir = Some("  ".into());
    assert_eq!(cmd.check(), Ok(()), "an empty working folder means none");
}

#[test]
fn the_command_is_shown_the_way_it_will_run() {
    let mut cmd = command(r"C:\Program Files\Srv\srv.exe");
    cmd.args = vec!["serve".into(), "--config".into(), r"C:\a b\c.toml".into(), "say \"hi\"".into(), String::new()];
    assert_eq!(cmd.display(), r#""C:\Program Files\Srv\srv.exe" serve --config "C:\a b\c.toml" "say \"hi\"" """#);
}

#[test]
fn the_settings_have_no_launch_command_and_start_nothing_by_themselves() {
    let settings = AiSettings::default();
    assert!(settings.launch.is_none());
    assert!(!settings.start_on_investigate && !settings.stop_on_quit);
    assert_eq!(settings.launch_command().unwrap_err(), LaunchProblem::NoProgram);

    let old: AiSettings = serde_json::from_str(r#"{"enabled":true,"model":"m"}"#).unwrap();
    assert!(old.launch.is_none());
}

#[test]
fn the_settings_hand_out_a_launch_command_only_when_it_passes_the_rules() {
    let mut settings = AiSettings { launch: Some(command(r"C:\Tools\srv.exe")), ..AiSettings::default() };
    assert_eq!(settings.launch_command().unwrap().program, r"C:\Tools\srv.exe");
    settings.launch = Some(command(r"C:\Users\a\Downloads\srv.exe"));
    assert_eq!(settings.launch_command().unwrap_err(), LaunchProblem::UnsafeFolder);
}
