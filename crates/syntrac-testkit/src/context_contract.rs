//! Contract suite for the memory of how programs have been seen running: every `ContextRepository` (in-memory, SQLite, ...) must
//! pass these, so the two cannot drift apart.

use syntrac_core::{
    roll_up_due, AccountKind, ContextObservation, ContextRepository, ContextWrite, EventRepository, Executable,
    MetricsRepository, ProcessRepository, Retention, RollupRepository, ServiceInfo, Timestamp,
};

use crate::fixtures::{metrics_sample, process_metric};
use crate::rollup_contract::{secs, settings, start_process, t};

pub trait ContextSubject:
    ContextRepository + RollupRepository + EventRepository + ProcessRepository + MetricsRepository + Retention
{
}
impl<T> ContextSubject for T where
    T: ContextRepository + RollupRepository + EventRepository + ProcessRepository + MetricsRepository + Retention
{
}

const SYSTEM_SID: &str = "S-1-5-18";
const USER_SID: &str = "S-1-5-21-1004336348-1177238915-682003330-1001";
const ACMESERV: &str = r"C:\Program Files (x86)\Acme\acmeserv.exe";
const SVCHOST: &str = r"C:\Windows\System32\svchost.exe";

pub fn context_contract<R: ContextSubject>(make: impl Fn() -> R) {
    an_empty_database_has_no_context(make());
    a_recorded_account_can_be_read_back_by_path_ignoring_case(make());
    recording_widens_first_and_last_seen_and_counts_only_what_is_new(make());
    the_order_of_recording_does_not_matter(make());
    a_missing_name_never_erases_a_known_one(make());
    several_accounts_come_most_recent_first(make());
    services_are_a_set_and_keep_their_display_name(make());
    all_contexts_lists_every_program_sorted(make());
    context_survives_the_purge_of_detail(make());
    a_program_recorded_before_the_rollups_met_it_keeps_one_row_and_its_first_spelling(make());
}

fn observation(path: &str, sid: &str, name: Option<&str>, services: &[(&str, Option<&str>)]) -> ContextObservation {
    ContextObservation {
        exe: Executable::from_path(path).unwrap(),
        sid: sid.to_owned(),
        account_name: name.map(str::to_owned),
        kind: syntrac_core::classify_sid(sid),
        services: services
            .iter()
            .map(|(service, display)| ServiceInfo {
                name: (*service).to_owned(),
                display_name: display.map(str::to_owned),
            })
            .collect(),
    }
}

fn record<R: ContextRepository>(repo: &mut R, at: Timestamp, observations: &[ContextObservation]) -> ContextWrite {
    repo.record_context(observations, at).unwrap()
}

fn an_empty_database_has_no_context<R: ContextSubject>(repo: R) {
    let context = repo.context_of(ACMESERV).unwrap();
    assert!(context.accounts.is_empty() && context.services.is_empty());
    assert!(repo.all_contexts().unwrap().is_empty());
}

fn a_recorded_account_can_be_read_back_by_path_ignoring_case<R: ContextSubject>(mut repo: R) {
    let seen = observation(ACMESERV, SYSTEM_SID, Some(r"NT AUTHORITY\SYSTEM"), &[("acmeserv", Some("Acme Service"))]);
    let write = record(&mut repo, t(0, 5), &[seen]);

    assert_eq!(write, ContextWrite { new_accounts: 1, new_services: 1 });
    let context = repo.context_of(&ACMESERV.to_uppercase()).unwrap();
    assert_eq!(context.accounts.len(), 1);
    let account = &context.accounts[0];
    assert_eq!((account.sid.as_str(), account.kind), (SYSTEM_SID, AccountKind::System));
    assert_eq!(account.name.as_deref(), Some(r"NT AUTHORITY\SYSTEM"));
    assert_eq!((account.first_seen, account.last_seen), (t(0, 5), t(0, 5)));
    assert_eq!(context.services.len(), 1);
    assert_eq!(context.services[0].name, "acmeserv");
    assert_eq!(context.services[0].display_name.as_deref(), Some("Acme Service"));
}

fn recording_widens_first_and_last_seen_and_counts_only_what_is_new<R: ContextSubject>(mut repo: R) {
    let seen = observation(ACMESERV, SYSTEM_SID, None, &[("acmeserv", None)]);
    record(&mut repo, t(0, 5), std::slice::from_ref(&seen));

    let again = record(&mut repo, t(3, 0), std::slice::from_ref(&seen));

    assert_eq!(again, ContextWrite::default(), "nothing new the second time");
    let context = repo.context_of(ACMESERV).unwrap();
    assert_eq!((context.accounts[0].first_seen, context.accounts[0].last_seen), (t(0, 5), t(3, 0)));
    assert_eq!((context.services[0].first_seen, context.services[0].last_seen), (t(0, 5), t(3, 0)));
}

fn the_order_of_recording_does_not_matter<R: ContextSubject>(mut repo: R) {
    let seen = observation(ACMESERV, SYSTEM_SID, None, &[]);
    record(&mut repo, t(3, 0), std::slice::from_ref(&seen));
    record(&mut repo, t(0, 5), std::slice::from_ref(&seen));

    let account = &repo.context_of(ACMESERV).unwrap().accounts[0];

    assert_eq!(
        (account.first_seen, account.last_seen),
        (t(0, 5), t(3, 0)),
        "an older sighting still moves first seen back"
    );
}

fn a_missing_name_never_erases_a_known_one<R: ContextSubject>(mut repo: R) {
    record(&mut repo, t(0, 5), &[observation(ACMESERV, USER_SID, Some(r"PC\alice"), &[])]);

    record(&mut repo, t(1, 0), &[observation(ACMESERV, USER_SID, None, &[])]);
    assert_eq!(repo.context_of(ACMESERV).unwrap().accounts[0].name.as_deref(), Some(r"PC\alice"));

    record(&mut repo, t(2, 0), &[observation(ACMESERV, USER_SID, Some(r"PC\alice2"), &[])]);
    assert_eq!(
        repo.context_of(ACMESERV).unwrap().accounts[0].name.as_deref(),
        Some(r"PC\alice2"),
        "a present name replaces"
    );
}

fn several_accounts_come_most_recent_first<R: ContextSubject>(mut repo: R) {
    record(&mut repo, t(0, 5), &[observation(ACMESERV, USER_SID, None, &[])]);
    record(&mut repo, t(2, 0), &[observation(ACMESERV, SYSTEM_SID, None, &[])]);

    let accounts = repo.context_of(ACMESERV).unwrap().accounts;

    let sids: Vec<&str> = accounts.iter().map(|a| a.sid.as_str()).collect();
    assert_eq!(sids, [SYSTEM_SID, USER_SID]);
}

fn services_are_a_set_and_keep_their_display_name<R: ContextSubject>(mut repo: R) {
    record(
        &mut repo,
        t(0, 5),
        &[observation(SVCHOST, "S-1-5-19", None, &[("Dnscache", Some("DNS Client")), ("Audiosrv", None)])],
    );
    let write = record(
        &mut repo,
        t(1, 0),
        &[observation(SVCHOST, "S-1-5-19", None, &[("Dnscache", None), ("Wcmsvc", Some("Connection Manager"))])],
    );

    assert_eq!(write, ContextWrite { new_accounts: 0, new_services: 1 }, "only Wcmsvc is new");
    let services = repo.context_of(SVCHOST).unwrap().services;
    let names: Vec<&str> = services.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        ["Dnscache", "Wcmsvc", "Audiosrv"],
        "most recently seen first (Dnscache and Wcmsvc tie at 1:00: by name)"
    );
    let dns = services.iter().find(|s| s.name == "Dnscache").unwrap();
    assert_eq!(dns.display_name.as_deref(), Some("DNS Client"), "a missing display name never erases a known one");
}

fn all_contexts_lists_every_program_sorted<R: ContextSubject>(mut repo: R) {
    record(
        &mut repo,
        t(0, 5),
        &[observation(SVCHOST, "S-1-5-19", None, &[]), observation(ACMESERV, SYSTEM_SID, None, &[])],
    );

    let all = repo.all_contexts().unwrap();

    let keys: Vec<&str> = all.iter().map(|(exe, _)| exe.key.as_str()).collect();
    assert_eq!(keys, [r"c:\program files (x86)\acme\acmeserv.exe", r"c:\windows\system32\svchost.exe"]);
    assert_eq!(all[0].0.path, ACMESERV);
    assert_eq!(all[0].1.accounts.len(), 1);
}

fn context_survives_the_purge_of_detail<R: ContextSubject>(mut repo: R) {
    record(&mut repo, t(0, 5), &[observation(ACMESERV, SYSTEM_SID, None, &[("acmeserv", None)])]);
    let before = repo.context_of(ACMESERV).unwrap();

    repo.purge_before(t(100, 0)).unwrap();

    assert_eq!(repo.context_of(ACMESERV).unwrap(), before);
}

fn a_program_recorded_before_the_rollups_met_it_keeps_one_row_and_its_first_spelling<R: ContextSubject>(mut repo: R) {
    record(&mut repo, t(0, 5), &[observation(r"C:\Apps\Late.exe", USER_SID, None, &[])]);

    // Later the rollups meet the same program under another spelling.
    let late = start_process(&mut repo, 10, t(0, 0), r"c:\apps\LATE.EXE");
    repo.append_metrics(&metrics_sample(secs(0, 10), 5.0, vec![process_metric(10, Some(late.id), 3.0)], 0.0, 0))
        .unwrap();
    roll_up_due(&mut repo, t(2, 0), &settings(4)).unwrap();

    let all = repo.all_contexts().unwrap();
    assert_eq!(all.len(), 1, "one program, one row");
    assert_eq!(all[0].0.path, r"C:\Apps\Late.exe", "the spelling first seen is kept");
    let ledger = repo.executable_ledger(r"C:\APPS\LATE.EXE").unwrap().expect("the rollups still find it");
    assert_eq!(ledger.first_seen, t(0, 10));
}
