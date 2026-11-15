//! What account a program runs as and which services it hosts: the pure rules (SID classes, merging a snapshot, throttling
//! writes). No Windows, no database.

use std::time::Duration;

use syntrac_core::exe_context::{
    classify_sid, merge_snapshot, AccountKind, ContextSnapshot, ContextTracker, ProcessContext, ServiceInfo,
};
use syntrac_core::Timestamp;

const SYSTEM: &str = "S-1-5-18";
const USER: &str = "S-1-5-21-1004336348-1177238915-682003330-1001";

fn service(name: &str) -> ServiceInfo {
    ServiceInfo { name: name.to_owned(), display_name: Some(format!("{name} service")) }
}

fn process(path: &str, sid: &str, services: Vec<ServiceInfo>) -> ProcessContext {
    ProcessContext { path: path.to_owned(), sid: sid.to_owned(), account_name: None, services }
}

fn snapshot(processes: Vec<ProcessContext>) -> ContextSnapshot {
    ContextSnapshot { processes, unreadable: 0 }
}

fn at(seconds: i64) -> Timestamp {
    Timestamp::from_micros(seconds * 1_000_000)
}

// ---- SID classes

#[test]
fn well_known_accounts_have_their_own_kinds() {
    assert_eq!(classify_sid("S-1-5-18"), AccountKind::System);
    assert_eq!(classify_sid("S-1-5-19"), AccountKind::LocalService);
    assert_eq!(classify_sid("S-1-5-20"), AccountKind::NetworkService);
}

#[test]
fn a_per_service_account_is_a_service_account() {
    assert_eq!(
        classify_sid("S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464"),
        AccountKind::ServiceAccount
    );
}

#[test]
fn an_ordinary_machine_or_domain_account_is_a_user() {
    assert_eq!(classify_sid(USER), AccountKind::User);
    assert_eq!(classify_sid("S-1-5-21-1-2-3-500"), AccountKind::User, "the built-in administrator is still an account");
}

#[test]
fn other_system_identities_are_other() {
    assert_eq!(classify_sid("S-1-5-90-0-1"), AccountKind::Other, "the window manager");
    assert_eq!(classify_sid("S-1-5-96-0-1"), AccountKind::Other, "the font driver host");
    assert_eq!(classify_sid("S-1-5-32-544"), AccountKind::Other, "a group, not an account a process runs as");
}

#[test]
fn text_that_is_not_a_sid_is_other_not_a_guess() {
    assert_eq!(classify_sid(""), AccountKind::Other);
    assert_eq!(classify_sid("not a sid"), AccountKind::Other);
    assert_eq!(classify_sid("S-1-5-18-extra"), AccountKind::Other, "only the exact SID is SYSTEM");
}

#[test]
fn the_text_is_trimmed_and_case_does_not_matter() {
    assert_eq!(classify_sid("  s-1-5-18 "), AccountKind::System);
}

#[test]
fn a_kind_round_trips_through_its_stored_word() {
    for kind in [
        AccountKind::System,
        AccountKind::LocalService,
        AccountKind::NetworkService,
        AccountKind::ServiceAccount,
        AccountKind::User,
        AccountKind::Other,
    ] {
        assert_eq!(AccountKind::parse(kind.as_str()), kind);
    }
    assert_eq!(AccountKind::parse("something new"), AccountKind::Other, "an unknown word is never a crash");
}

// ---- merging a snapshot

#[test]
fn two_instances_of_one_program_under_one_account_are_one_observation() {
    let merged = merge_snapshot(snapshot(vec![
        process(r"C:\Apps\A.exe", USER, vec![]),
        process(r"c:\apps\a.exe", USER, vec![]),
    ]));

    assert_eq!(merged.observations.len(), 1);
    assert_eq!(merged.observations[0].exe.key, r"c:\apps\a.exe");
    assert_eq!(merged.observations[0].kind, AccountKind::User);
}

#[test]
fn the_same_program_under_two_accounts_is_two_observations() {
    let merged = merge_snapshot(snapshot(vec![
        process(r"C:\Apps\A.exe", USER, vec![]),
        process(r"C:\Apps\A.exe", SYSTEM, vec![]),
    ]));

    let kinds: Vec<AccountKind> = merged.observations.iter().map(|o| o.kind).collect();
    assert_eq!(kinds.len(), 2);
    assert!(kinds.contains(&AccountKind::User) && kinds.contains(&AccountKind::System));
}

#[test]
fn the_services_of_several_instances_are_added_together_without_repeats() {
    let merged = merge_snapshot(snapshot(vec![
        process(r"C:\Windows\System32\svchost.exe", "S-1-5-19", vec![service("Dnscache"), service("Audiosrv")]),
        process(r"C:\Windows\System32\svchost.exe", "S-1-5-19", vec![service("Dnscache"), service("Wcmsvc")]),
    ]));

    let names: Vec<&str> = merged.observations[0].services.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Audiosrv", "Dnscache", "Wcmsvc"], "sorted, each once");
}

#[test]
fn the_first_account_name_that_is_known_is_kept() {
    let mut anonymous = process(r"C:\Apps\A.exe", USER, vec![]);
    anonymous.account_name = None;
    let mut named = process(r"C:\Apps\A.exe", USER, vec![]);
    named.account_name = Some(r"PC\alice".to_owned());

    let merged = merge_snapshot(snapshot(vec![anonymous, named]));

    assert_eq!(merged.observations[0].account_name.as_deref(), Some(r"PC\alice"));
}

#[test]
fn a_process_with_no_path_or_no_sid_is_skipped_and_counted() {
    let merged = merge_snapshot(ContextSnapshot {
        processes: vec![
            process("   ", USER, vec![]),
            process(r"C:\Apps\A.exe", "", vec![]),
            process(r"C:\Apps\B.exe", USER, vec![]),
        ],
        unreadable: 4,
    });

    assert_eq!(merged.observations.len(), 1);
    assert_eq!(merged.skipped, 2);
    assert_eq!(merged.unreadable, 4, "processes the adapter could not open are passed on");
}

#[test]
fn the_order_does_not_depend_on_the_order_of_the_snapshot() {
    let a = process(r"C:\Apps\A.exe", USER, vec![service("one")]);
    let b = process(r"C:\Apps\B.exe", SYSTEM, vec![]);

    assert_eq!(
        merge_snapshot(snapshot(vec![a.clone(), b.clone()])).observations,
        merge_snapshot(snapshot(vec![b, a])).observations
    );
}

#[test]
fn the_path_is_shown_as_first_spelled_the_smallest_spelling() {
    let merged = merge_snapshot(snapshot(vec![
        process(r"C:\Apps\A.exe", USER, vec![]),
        process(r"C:\APPS\A.EXE", USER, vec![]),
    ]));

    assert_eq!(merged.observations[0].exe.path, r"C:\APPS\A.EXE", "uppercase sorts first: a deterministic choice");
}

// ---- throttling writes

fn one(path: &str, sid: &str, services: Vec<ServiceInfo>) -> syntrac_core::exe_context::ContextObservation {
    merge_snapshot(snapshot(vec![process(path, sid, services)])).observations.remove(0)
}

#[test]
fn a_program_not_written_before_is_due() {
    let tracker = ContextTracker::new(Duration::from_secs(600));

    let due = tracker.due(&[one(r"C:\Apps\A.exe", USER, vec![])], at(0));

    assert_eq!(due.len(), 1);
}

#[test]
fn a_program_just_written_is_not_due_again_until_the_refresh_has_passed() {
    let mut tracker = ContextTracker::new(Duration::from_secs(600));
    let seen = one(r"C:\Apps\A.exe", USER, vec![]);
    tracker.mark_written(std::slice::from_ref(&seen), at(0));

    assert!(tracker.due(std::slice::from_ref(&seen), at(30)).is_empty());
    assert!(tracker.due(std::slice::from_ref(&seen), at(599)).is_empty());
    assert_eq!(tracker.due(std::slice::from_ref(&seen), at(600)).len(), 1, "ten minutes: last seen is moved");
}

#[test]
fn a_change_in_the_services_is_due_at_once() {
    let mut tracker = ContextTracker::new(Duration::from_secs(600));
    tracker.mark_written(&[one(r"C:\Windows\System32\svchost.exe", "S-1-5-19", vec![service("a")])], at(0));

    let changed = one(r"C:\Windows\System32\svchost.exe", "S-1-5-19", vec![service("a"), service("b")]);

    assert_eq!(tracker.due(&[changed], at(5)).len(), 1);
}

#[test]
fn a_second_account_of_a_known_program_is_due_at_once() {
    let mut tracker = ContextTracker::new(Duration::from_secs(600));
    tracker.mark_written(&[one(r"C:\Apps\A.exe", USER, vec![])], at(0));

    assert_eq!(tracker.due(&[one(r"C:\Apps\A.exe", SYSTEM, vec![])], at(5)).len(), 1);
}

#[test]
fn something_that_was_never_marked_written_stays_due_so_a_failed_write_is_retried() {
    let tracker = ContextTracker::new(Duration::from_secs(600));
    let seen = one(r"C:\Apps\A.exe", USER, vec![]);

    assert_eq!(tracker.due(std::slice::from_ref(&seen), at(0)).len(), 1);
    assert_eq!(tracker.due(std::slice::from_ref(&seen), at(30)).len(), 1, "nothing was marked, so it is still due");
}

#[test]
fn the_tracker_forgets_programs_that_stopped_running_so_it_cannot_grow_without_limit() {
    let mut tracker = ContextTracker::new(Duration::from_secs(600));
    tracker.mark_written(&[one(r"C:\Apps\A.exe", USER, vec![])], at(0));
    tracker.mark_written(&[one(r"C:\Apps\B.exe", USER, vec![])], at(0));

    tracker.forget_unseen(&[one(r"C:\Apps\B.exe", USER, vec![])]);

    assert_eq!(tracker.len(), 1);
}
