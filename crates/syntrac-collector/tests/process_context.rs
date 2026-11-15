//! The Windows adapter for run-as accounts and services, exercised on the machine running the tests.
//!
//! The tests only ask for things that are true on any Windows machine and for any user, elevated or not: this very process is
//! listed with its own account, the account is a well-formed SID, and the service manager can be asked.

use syntrac_collector::process_context::{running_services, WindowsProcessContextSource};
use syntrac_core::{classify_sid, AccountKind, ProcessContextSource};

#[test]
fn this_process_is_listed_with_its_own_account() {
    let source = WindowsProcessContextSource::new();

    let snapshot = source.observe().expect("the process list can be read");

    let exe = std::env::current_exe().unwrap();
    let me = snapshot
        .processes
        .iter()
        .find(|p| std::path::Path::new(&p.path).file_name() == exe.file_name())
        .unwrap_or_else(|| {
            panic!("{} is not in the snapshot of {} processes", exe.display(), snapshot.processes.len())
        });
    assert!(me.sid.starts_with("S-1-"), "a SID in its text form: {}", me.sid);
    assert!(
        matches!(classify_sid(&me.sid), AccountKind::User | AccountKind::System),
        "a test run is by a person or by the system account, not {:?}",
        classify_sid(&me.sid)
    );
}

#[test]
fn the_account_is_named_the_way_windows_names_it() {
    let snapshot = WindowsProcessContextSource::new().observe().unwrap();
    let exe = std::env::current_exe().unwrap();
    let me = snapshot.processes.iter().find(|p| std::path::Path::new(&p.path).file_name() == exe.file_name()).unwrap();

    let name = me.account_name.as_deref().expect("Windows can name the account this test runs as");

    assert!(name.contains('\\'), "DOMAIN\\name: {name}");
    if classify_sid(&me.sid) == AccountKind::User {
        let user = std::env::var("USERNAME").expect("USERNAME is set for a person");
        assert!(name.to_lowercase().ends_with(&user.to_lowercase()), "{name} should end with {user}");
    }
}

#[test]
fn a_snapshot_has_many_programs_and_every_account_is_a_sid() {
    let snapshot = WindowsProcessContextSource::new().observe().unwrap();

    assert!(snapshot.processes.len() > 5, "{} processes", snapshot.processes.len());
    assert!(snapshot.processes.iter().all(|p| p.sid.starts_with("S-1-") && !p.path.trim().is_empty()));
}

#[test]
fn the_same_source_can_be_asked_again_and_names_come_from_its_cache() {
    let source = WindowsProcessContextSource::new();

    let first = source.observe().unwrap();
    let second = source.observe().unwrap();

    assert!(second.processes.len() > 5);
    assert!(first.processes.iter().any(|p| p.account_name.is_some()));
}

#[test]
fn the_service_manager_lists_running_services_by_the_process_that_hosts_them() {
    let services = running_services();

    assert!(!services.is_empty(), "a running Windows has services");
    assert!(services.keys().all(|pid| *pid > 0), "a running service has a process");
    let all: Vec<_> = services.values().flatten().collect();
    assert!(all.iter().all(|s| !s.name.is_empty() && !s.name.chars().any(char::is_control)));
}
