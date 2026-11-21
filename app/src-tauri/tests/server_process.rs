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
    let mut child = WindowsSpawner::default().spawn_job(&cmd(&["/c", "exit", "7"])).unwrap();
    wait_until("the exit", || child.try_exit().unwrap().is_some());
    assert_eq!(child.try_exit().unwrap(), Some(7));
}

#[test]
fn stopping_ends_the_program_and_the_program_it_started() {
    // cmd.exe starts ping.exe, which runs for about a minute: both are in the job.
    let mut child = WindowsSpawner::default().spawn_job(&cmd(&["/c", "ping", "-n", "60", "127.0.0.1"])).unwrap();
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
    let mut child = WindowsSpawner::default().spawn_job(&command).unwrap();
    wait_until("the exit", || child.try_exit().unwrap().is_some());
    assert_eq!(child.try_exit().unwrap(), Some(0));
}

#[test]
fn a_missing_program_or_folder_is_reported_before_anything_starts() {
    let mut missing = cmd(&[]);
    missing.program = system_file("no-such-server-program.exe");
    assert!(matches!(WindowsSpawner::default().spawn_job(&missing), Err(LaunchError::ProgramMissing)));

    let mut no_folder = cmd(&["/c", "exit"]);
    no_folder.working_dir = Some(system_file("no-such-folder"));
    assert!(matches!(WindowsSpawner::default().spawn_job(&no_folder), Err(LaunchError::FolderMissing)));
}

#[test]
fn the_rules_are_checked_again_at_the_moment_of_starting() {
    let mut script = cmd(&[]);
    script.program = system_file("anything.bat");
    assert!(matches!(WindowsSpawner::default().spawn_job(&script), Err(LaunchError::Refused(LaunchProblem::NotAnExe))));
}

#[test]
fn what_the_program_prints_is_saved_to_a_log_file_that_starts_fresh_each_time() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("Syntrac").join("model-server.log");
    let spawner = WindowsSpawner::with_log(log.clone());

    // Output and errors both land in the file (the folder for it is made).
    let mut child = spawner.spawn_job(&cmd(&["/c", "echo to-output & echo to-error 1>&2"])).unwrap();
    wait_until("the exit", || child.try_exit().unwrap().is_some());
    let text = std::fs::read_to_string(&log).unwrap();
    assert!(text.contains("to-output") && text.contains("to-error"), "{text:?}");

    // The next start begins a new log, so the file cannot grow for ever across starts.
    let mut again = spawner.spawn_job(&cmd(&["/c", "echo second-run"])).unwrap();
    wait_until("the exit", || again.try_exit().unwrap().is_some());
    let text = std::fs::read_to_string(&log).unwrap();
    assert!(text.contains("second-run") && !text.contains("to-output"), "{text:?}");
}

#[test]
fn without_a_log_the_output_is_discarded_as_before() {
    let mut child = WindowsSpawner::default().spawn_job(&cmd(&["/c", "echo nobody-reads-this"])).unwrap();
    wait_until("the exit", || child.try_exit().unwrap().is_some());
    assert_eq!(child.try_exit().unwrap(), Some(0));
}
