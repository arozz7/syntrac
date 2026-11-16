//! One sweep of the places that say what starts: what is recorded, what is counted, and what a failing source cannot damage.

use std::collections::HashSet;

use syntrac_core::{sweep_launchers, LaunchSource, LauncherKind, LauncherRepository, RawEntry, SourceRead, Timestamp};
use syntrac_testkit::{FakeLaunchSource, InMemoryRepository};

fn env(name: &str) -> Option<String> {
    match name.to_ascii_lowercase().as_str() {
        "windir" | "systemroot" => Some(r"C:\WINDOWS".to_owned()),
        "programfiles" => Some(r"C:\Program Files".to_owned()),
        _ => None,
    }
}

fn entry(kind: LauncherKind, location: &str, command: &str) -> RawEntry {
    RawEntry { kind, location: location.to_owned(), command: command.to_owned(), detail: String::new(), profile: None }
}

fn at(micros: i64) -> Timestamp {
    Timestamp::from_micros(micros)
}

fn sweep(
    repo: &mut InMemoryRepository,
    sources: &[&dyn LaunchSource],
    existing: &[&str],
    now: i64,
) -> syntrac_core::launch_run::SweepReport {
    let set: HashSet<String> = existing.iter().map(|p| p.to_lowercase()).collect();
    sweep_launchers(sources, &env, &|p| set.contains(&p.to_lowercase()), repo, at(now)).unwrap()
}

const SERV: &str = r"C:\Program Files (x86)\Acme\acmeserv.exe";

#[test]
fn entries_are_mapped_to_programs_and_recorded_with_their_arguments() {
    let mut repo = InMemoryRepository::default();
    let services = FakeLaunchSource::new(
        "services",
        &[LauncherKind::ServiceAuto],
        vec![entry(LauncherKind::ServiceAuto, "acmeserv", &format!("{SERV} -noconsole"))],
    );

    let report = sweep(&mut repo, &[&services], &[SERV], 100);

    assert_eq!((report.entries, report.recorded, report.unresolved, report.new), (1, 1, 0, 1));
    let seen = repo.launchers_of(SERV).unwrap();
    assert_eq!(
        (seen[0].kind, seen[0].location.as_str(), seen[0].arguments.as_str()),
        (LauncherKind::ServiceAuto, "acmeserv", "-noconsole")
    );
}

#[test]
fn a_second_sweep_adds_nothing_new_and_a_removed_entry_stops_being_current() {
    let mut repo = InMemoryRepository::default();
    let both = FakeLaunchSource::new(
        "tasks",
        &[LauncherKind::Task],
        vec![
            entry(LauncherKind::Task, r"\One", r"%windir%\a.exe"),
            entry(LauncherKind::Task, r"\Two", r"%windir%\b.exe"),
        ],
    );
    sweep(&mut repo, &[&both], &[], 100);

    let only_one = FakeLaunchSource::new(
        "tasks",
        &[LauncherKind::Task],
        vec![entry(LauncherKind::Task, r"\One", r"%windir%\a.exe")],
    );
    let report = sweep(&mut repo, &[&only_one], &[], 200);

    assert_eq!(report.new, 0);
    assert!(repo.launchers_of(r"C:\WINDOWS\a.exe").unwrap()[0].current);
    assert!(!repo.launchers_of(r"C:\WINDOWS\b.exe").unwrap()[0].current);
}

#[test]
fn a_command_that_cannot_be_mapped_is_counted_not_stored_and_a_missing_file_is_stored_and_counted() {
    let mut repo = InMemoryRepository::default();
    let source = FakeLaunchSource::new(
        "run",
        &[LauncherKind::RunKey],
        vec![
            entry(LauncherKind::RunKey, "unknown-var", r"%NOSUCH%\x.exe"),
            entry(LauncherKind::RunKey, "empty", "   "),
            entry(LauncherKind::RunKey, "gone", r"C:\Gone\gone.exe"),
        ],
    );

    let report = sweep(&mut repo, &[&source], &[], 100);

    assert_eq!((report.entries, report.unresolved, report.recorded, report.missing_files), (3, 2, 1, 1));
    assert_eq!(repo.all_launchers().unwrap().len(), 1);
}

#[test]
fn a_source_that_fails_is_reported_the_others_still_run_and_its_kinds_are_not_marked_as_read() {
    let mut repo = InMemoryRepository::default();
    let tasks =
        FakeLaunchSource::new("tasks", &[LauncherKind::Task], vec![entry(LauncherKind::Task, r"\One", r"C:\a.exe")]);
    let runs =
        FakeLaunchSource::new("run", &[LauncherKind::RunKey], vec![entry(LauncherKind::RunKey, "r", r"C:\r.exe")]);
    sweep(&mut repo, &[&tasks, &runs], &[], 100);

    let broken_runs = FakeLaunchSource::failing("run", &[LauncherKind::RunKey], "registry unavailable");
    let report = sweep(&mut repo, &[&tasks, &broken_runs], &[], 200);

    assert_eq!(report.failed, [("run".to_owned(), "registry unavailable".to_owned())]);
    assert!(repo.launchers_of(r"C:\r.exe").unwrap()[0].current, "not read, so not known to be gone");
    assert!(repo.launchers_of(r"C:\a.exe").unwrap()[0].current);
}

#[test]
fn when_every_source_fails_nothing_is_written() {
    let mut repo = InMemoryRepository::default();
    let broken = FakeLaunchSource::failing("tasks", &[LauncherKind::Task], "denied");

    let report = sweep(&mut repo, &[&broken], &[], 100);

    assert_eq!((report.entries, report.recorded), (0, 0));
    assert_eq!(report.failed.len(), 1);
    assert!(repo.all_launchers().unwrap().is_empty());
}

#[test]
fn entries_of_one_user_use_that_users_folders_and_without_a_profile_they_cannot_be_mapped() {
    let mut repo = InMemoryRepository::default();
    let mut with_profile = entry(LauncherKind::RunKey, "HKU\\x\\Run\\Sync", r"%LOCALAPPDATA%\Sync\sync.exe --hidden");
    with_profile.profile = Some(r"C:\Users\someone".to_owned());
    let without_profile = entry(LauncherKind::RunKey, "HKLM\\Run\\Sync", r"%LOCALAPPDATA%\Sync\sync.exe");
    let source = FakeLaunchSource::new("run", &[LauncherKind::RunKey], vec![with_profile, without_profile]);

    let report = sweep(&mut repo, &[&source], &[], 100);

    assert_eq!((report.recorded, report.unresolved), (1, 1));
    let seen = repo.launchers_of(r"C:\Users\someone\AppData\Local\Sync\sync.exe").unwrap();
    assert_eq!(seen[0].arguments, "--hidden");
}

#[test]
fn the_same_entry_found_twice_is_one_record_and_a_runaway_source_is_bounded() {
    let mut repo = InMemoryRepository::default();
    let twice = FakeLaunchSource::new(
        "tasks",
        &[LauncherKind::Task],
        vec![entry(LauncherKind::Task, r"\One", r"C:\a.exe"), entry(LauncherKind::Task, r"\One", r"C:\A.EXE")],
    );
    assert_eq!(sweep(&mut repo, &[&twice], &[], 100).recorded, 1);

    let flood: Vec<RawEntry> =
        (0..6_000).map(|n| entry(LauncherKind::Task, &format!(r"\T{n}"), &format!(r"C:\x{n}.exe"))).collect();
    let runaway = FakeLaunchSource::new("tasks", &[LauncherKind::Task], flood);
    let report = sweep(&mut repo, &[&runaway], &[], 200);

    assert_eq!(report.entries, 5_000, "bounded");
}

#[test]
fn unreadable_items_are_counted() {
    let mut repo = InMemoryRepository::default();
    let source = FakeLaunchSource::with_read(
        "tasks",
        &[LauncherKind::Task],
        SourceRead { entries: vec![entry(LauncherKind::Task, r"\One", r"C:\a.exe")], unreadable: 3 },
    );

    assert_eq!(sweep(&mut repo, &[&source], &[], 100).unreadable, 3);
}
