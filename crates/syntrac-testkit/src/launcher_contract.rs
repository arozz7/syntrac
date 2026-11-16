//! Contract suite for launch records: every `LauncherRepository` (in-memory, SQLite, ...) must pass these, so the two cannot drift
//! apart.

use syntrac_core::{Executable, LauncherKind, LauncherObservation, LauncherRepository, Timestamp};

pub fn launcher_contract<R: LauncherRepository>(make: impl Fn() -> R) {
    nothing_recorded_means_nothing_listed(make());
    an_entry_is_recorded_and_found_by_any_spelling_of_the_path(make());
    recording_again_changes_only_last_seen_and_the_text(make());
    an_entry_missing_from_a_complete_sweep_is_no_longer_current_but_is_kept(make());
    a_kind_that_was_not_read_completely_keeps_its_entries_current(make());
    a_program_started_two_ways_has_two_entries_and_keeps_its_first_spelling(make());
    an_older_report_never_moves_last_seen_back(make());
}

const APP: &str = r"C:\Apps\App.exe";
const APP_LOWER: &str = r"c:\apps\app.exe";

fn at(micros: i64) -> Timestamp {
    Timestamp::from_micros(micros)
}

fn entry(path: &str, kind: LauncherKind, location: &str, detail: &str, arguments: &str) -> LauncherObservation {
    LauncherObservation {
        exe: Executable::from_path(path).unwrap(),
        kind,
        location: location.to_owned(),
        detail: detail.to_owned(),
        arguments: arguments.to_owned(),
    }
}

fn nothing_recorded_means_nothing_listed<R: LauncherRepository>(repo: R) {
    assert!(repo.launchers_of(APP).unwrap().is_empty());
    assert!(repo.all_launchers().unwrap().is_empty());
    assert!(repo.launchers_of("").unwrap().is_empty(), "not a path: nothing, not an error");
}

fn an_entry_is_recorded_and_found_by_any_spelling_of_the_path<R: LauncherRepository>(mut repo: R) {
    let write = repo
        .record_launchers(
            &[entry(APP, LauncherKind::Task, r"\Vendor\Updater", "boot, logon", "/check")],
            &[LauncherKind::Task],
            at(10),
        )
        .unwrap();

    assert_eq!((write.written, write.new), (1, 1));
    for spelling in [APP, APP_LOWER] {
        let seen = repo.launchers_of(spelling).unwrap();
        assert_eq!(seen.len(), 1, "{spelling}");
        let one = &seen[0];
        assert_eq!(
            (one.kind, one.location.as_str(), one.detail.as_str(), one.arguments.as_str()),
            (LauncherKind::Task, r"\Vendor\Updater", "boot, logon", "/check")
        );
        assert_eq!((one.first_seen, one.last_seen, one.current), (at(10), at(10), true));
    }
}

fn recording_again_changes_only_last_seen_and_the_text<R: LauncherRepository>(mut repo: R) {
    let first = entry(APP, LauncherKind::RunKey, r"HKLM\Run\App", "", "/background");
    repo.record_launchers(&[first], &[LauncherKind::RunKey], at(10)).unwrap();

    let again = entry(APP, LauncherKind::RunKey, r"HKLM\Run\App", "", "/background /new");
    let write = repo.record_launchers(&[again], &[LauncherKind::RunKey], at(20)).unwrap();

    assert_eq!((write.written, write.new), (1, 0), "not new the second time");
    let seen = repo.launchers_of(APP).unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!((seen[0].first_seen, seen[0].last_seen), (at(10), at(20)));
    assert_eq!(seen[0].arguments, "/background /new", "the newest text is kept");
    assert!(seen[0].current);
}

fn an_entry_missing_from_a_complete_sweep_is_no_longer_current_but_is_kept<R: LauncherRepository>(mut repo: R) {
    let a = entry(APP, LauncherKind::Task, r"\A", "logon", "");
    let b = entry(APP, LauncherKind::Task, r"\B", "boot", "");
    repo.record_launchers(&[a.clone(), b], &[LauncherKind::Task], at(10)).unwrap();

    repo.record_launchers(&[a], &[LauncherKind::Task], at(20)).unwrap();

    let seen = repo.launchers_of(APP).unwrap();
    let by_location = |location: &str| seen.iter().find(|s| s.location == location).unwrap();
    assert!(by_location(r"\A").current);
    assert!(!by_location(r"\B").current, "gone from a complete sweep");
    assert_eq!((by_location(r"\B").first_seen, by_location(r"\B").last_seen), (at(10), at(10)), "its history is kept");
}

fn a_kind_that_was_not_read_completely_keeps_its_entries_current<R: LauncherRepository>(mut repo: R) {
    let task = entry(APP, LauncherKind::Task, r"\A", "logon", "");
    let run = entry(APP, LauncherKind::RunKey, r"HKLM\Run\App", "", "");
    repo.record_launchers(&[task, run], &[LauncherKind::Task, LauncherKind::RunKey], at(10)).unwrap();

    // The next sweep read only the tasks (the Run key source failed): nothing about Run keys changes.
    repo.record_launchers(&[], &[LauncherKind::Task], at(20)).unwrap();
    // And a sweep that read nothing at all changes nothing.
    repo.record_launchers(&[], &[], at(30)).unwrap();

    let seen = repo.launchers_of(APP).unwrap();
    let kind = |k: LauncherKind| seen.iter().find(|s| s.kind == k).unwrap();
    assert!(!kind(LauncherKind::Task).current, "the tasks were read completely and it was not there");
    assert!(kind(LauncherKind::RunKey).current, "the Run keys were not read, so nothing is known to have gone");
}

fn a_program_started_two_ways_has_two_entries_and_keeps_its_first_spelling<R: LauncherRepository>(mut repo: R) {
    repo.record_launchers(
        &[entry(APP, LauncherKind::ServiceAuto, "appsvc", "auto", "-service")],
        &[LauncherKind::ServiceAuto],
        at(10),
    )
    .unwrap();
    repo.record_launchers(
        &[entry(APP_LOWER, LauncherKind::StartupFolder, r"C:\Startup\App.lnk", "", "")],
        &[LauncherKind::StartupFolder],
        at(20),
    )
    .unwrap();

    let seen = repo.launchers_of(APP).unwrap();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].kind, LauncherKind::StartupFolder, "newest first");
    let all = repo.all_launchers().unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].0.path, APP, "the spelling first seen");
    assert_eq!(all[0].1.len(), 2);
}

fn an_older_report_never_moves_last_seen_back<R: LauncherRepository>(mut repo: R) {
    let e = entry(APP, LauncherKind::RunOnce, r"HKLM\RunOnce\x", "", "");
    repo.record_launchers(std::slice::from_ref(&e), &[LauncherKind::RunOnce], at(50)).unwrap();

    repo.record_launchers(&[e], &[LauncherKind::RunOnce], at(30)).unwrap();

    let seen = repo.launchers_of(APP).unwrap();
    assert_eq!((seen[0].first_seen, seen[0].last_seen), (at(30), at(50)));
    assert!(seen[0].current);
}
