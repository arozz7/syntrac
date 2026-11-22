//! The stand-in service used to try the installer's rollback: it answers `--version` like the real one and refuses to do anything else.

use std::process::Command;

fn run(argument: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_broken-service")).arg(argument).output().expect("the stand-in starts")
}

#[test]
fn it_reports_a_version_and_a_schema_so_the_installer_goes_ahead() {
    let output = run("--version");
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.lines().nth(1).is_some_and(|line| line.starts_with("schema:    v")), "{text}");
}

#[test]
fn it_fails_when_started_as_a_service() {
    assert!(!run("--service").status.success());
}
