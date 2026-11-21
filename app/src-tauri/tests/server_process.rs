//! The real way of starting a program (docs/PHASE4-PLAN.md, L4c), with real harmless Windows programs: starting, the exit code, stopping the
//! program together with what it started, and the refusals.

use std::time::{Duration, Instant};

use syntrac_app_lib::server_process::WindowsSpawner;
use syntrac_core::server_launch::{LaunchCommand, LaunchProblem};
use syntrac_core::server_manager::{ChildHandle, LaunchError};

fn system_file(name: &str) -> String {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_owned());
    format!(r"{root}\System32\{name}")
}

fn cmd(args: &[&str]) -> LaunchCommand {
    LaunchCommand {
        program: system_file("cmd.exe"),
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        working_dir: None,
    }
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if done() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out waiting for: {what}");
}

#[test]
fn a_program_that_ends_by_itself_reports_its_exit_code() {
    let mut child = WindowsSpawner.spawn_job(&cmd(&["/c", "exit", "7"])).unwrap();
    wait_until("the exit", || child.try_exit().unwrap().is_some());
    assert_eq!(child.try_exit().unwrap(), Some(7));
}

#[test]
fn stopping_ends_the_program_and_the_program_it_started() {
    // cmd.exe starts ping.exe, which runs for about a minute: both are in the job.
    let mut child = WindowsSpawner.spawn_job(&cmd(&["/c", "ping", "-n", "60", "127.0.0.1"])).unwrap();
    // Windows may add a hidden console host to the job as well, so "at least those two".
    wait_until("the program and what it started to be running", || {
        child.active_processes().is_some_and(|count| count >= 2)
    });
    assert_eq!(child.try_exit().unwrap(), None, "it keeps running");

    child.kill().unwrap();

    wait_until("the job to be empty", || child.active_processes() == Some(0));
    assert!(child.try_exit().unwrap().is_some());
}

#[test]
fn the_working_folder_is_used() {
    let dir = tempfile::tempdir().unwrap();
    let mut command = cmd(&["/c", "cd"]);
    command.working_dir = Some(dir.path().to_string_lossy().into_owned());
    let mut child = WindowsSpawner.spawn_job(&command).unwrap();
    wait_until("the exit", || child.try_exit().unwrap().is_some());
    assert_eq!(child.try_exit().unwrap(), Some(0));
}

#[test]
fn a_missing_program_or_folder_is_reported_before_anything_starts() {
    let mut missing = cmd(&[]);
    missing.program = system_file("no-such-server-program.exe");
    assert!(matches!(WindowsSpawner.spawn_job(&missing), Err(LaunchError::ProgramMissing)));

    let mut no_folder = cmd(&["/c", "exit"]);
    no_folder.working_dir = Some(system_file("no-such-folder"));
    assert!(matches!(WindowsSpawner.spawn_job(&no_folder), Err(LaunchError::FolderMissing)));
}

#[test]
fn the_rules_are_checked_again_at_the_moment_of_starting() {
    let mut script = cmd(&[]);
    script.program = system_file("anything.bat");
    assert!(matches!(WindowsSpawner.spawn_job(&script), Err(LaunchError::Refused(LaunchProblem::NotAnExe))));
}
