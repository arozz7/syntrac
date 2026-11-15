//! What the app says about HOW a program runs: the account kinds seen on a ranking row, a "service" flag, and the accounts and
//! services in the identity panel of one program. Evidence for the screen, never a verdict.

use syntrac_app_lib::history_programs::{program_history, program_ranking, Grouping, ProgramMetricParam};
use syntrac_core::{
    classify_sid, roll_up_due, ContextObservation, ContextRepository, Executable, FileFacts, IdentityRecord,
    IdentityRepository, MetricsRepository, ProcessRepository, RollupSettings, ServiceInfo, Signature, SignatureStatus,
    Timestamp, VersionInfo,
};
use syntrac_testkit::fixtures::{at_secs, metrics_sample, process_metric, running_process};
use syntrac_testkit::InMemoryRepository;

const HOUR: i64 = 3600;
const BASE: i64 = 1_794_571_200 / 3600 * 3600;
const SERV: &str = r"C:\Program Files (x86)\Acme\acmeserv.exe";
const TRANSMIT: &str = r"C:\Program Files (x86)\Acme\x64\acmetransmit64.exe";
const CHROME: &str = r"C:\Program Files\Google\Chrome\chrome.exe";
const SYSTEM: &str = "S-1-5-18";
const USER: &str = "S-1-5-21-1004336348-1177238915-682003330-1001";

fn secs(hour: i64, minutes: i64) -> i64 {
    BASE + hour * HOUR + minutes * 60
}

fn now() -> Timestamp {
    at_secs(BASE + 20 * HOUR)
}

fn seen(path: &str, sid: &str, name: Option<&str>, services: &[(&str, &str)]) -> ContextObservation {
    ContextObservation {
        exe: Executable::from_path(path).unwrap(),
        sid: sid.to_owned(),
        account_name: name.map(str::to_owned),
        kind: classify_sid(sid),
        services: services
            .iter()
            .map(|(name, display)| ServiceInfo { name: (*name).to_owned(), display_name: Some((*display).to_owned()) })
            .collect(),
    }
}

fn acme_facts() -> FileFacts {
    FileFacts {
        signature: Signature {
            status: SignatureStatus::Valid,
            signer: Some("ACME SOFTWARE, INC.".to_owned()),
            ..Signature::default()
        },
        version: VersionInfo { product: Some("Acme Backup".to_owned()), ..VersionInfo::default() },
        ..FileFacts::default()
    }
}

/// Three programs that were running; Acme's two are signed (so they group). Nothing about accounts yet.
fn scenario() -> InMemoryRepository {
    let mut repo = InMemoryRepository::default();
    for (index, path) in [SERV, TRANSMIT, CHROME].iter().enumerate() {
        let pid = 10 + index as u32;
        let mut process = running_process(pid, at_secs(secs(0, 0)));
        process.image_path = (*path).to_owned();
        repo.record_start(process.clone()).unwrap();
        repo.append_metrics(&metrics_sample(
            secs(0, 10 + index as i64),
            30.0,
            vec![process_metric(pid, Some(process.id), 1.0 + index as f32)],
            0.0,
            0,
        ))
        .unwrap();
    }
    let settings = RollupSettings { settle: std::time::Duration::from_secs(300), max_hours_per_pass: 10 };
    roll_up_due(&mut repo, at_secs(secs(5, 0)), &settings).unwrap();
    for path in [SERV, TRANSMIT] {
        repo.save_identity(&IdentityRecord {
            exe: Executable::from_path(path).unwrap(),
            facts: Some(acme_facts()),
            inspected_at: Some(at_secs(BASE)),
            last_attempt: at_secs(BASE),
            failure: None,
        })
        .unwrap();
    }
    repo
}

fn rows(repo: &InMemoryRepository, grouping: Grouping) -> Vec<syntrac_app_lib::history_programs::ProgramRowView> {
    program_ranking(repo, now(), 7 * 24 * 3600, ProgramMetricParam::Cpu, 10, grouping).unwrap().programs
}

fn row_named<'a>(
    rows: &'a [syntrac_app_lib::history_programs::ProgramRowView],
    name: &str,
) -> &'a syntrac_app_lib::history_programs::ProgramRowView {
    rows.iter().find(|row| row.name == name).unwrap_or_else(|| panic!("no row named {name}"))
}

#[test]
fn a_program_never_looked_at_says_nothing_about_its_account() {
    let repo = scenario();

    let all = rows(&repo, Grouping::None);

    let chrome = row_named(&all, "chrome.exe");
    assert!(chrome.identity.runs_as.is_empty());
    assert!(!chrome.identity.service);
}

#[test]
fn a_row_carries_the_account_kinds_it_has_been_seen_running_as_and_whether_it_is_a_service() {
    let mut repo = scenario();
    repo.record_context(
        &[seen(SERV, SYSTEM, Some(r"NT AUTHORITY\SYSTEM"), &[("acmeserv", "Acme Service")])],
        at_secs(secs(1, 0)),
    )
    .unwrap();
    repo.record_context(&[seen(CHROME, USER, Some(r"PC\alice"), &[])], at_secs(secs(1, 0))).unwrap();

    let all = rows(&repo, Grouping::None);

    let serv = row_named(&all, "acmeserv.exe");
    assert_eq!(serv.identity.runs_as, ["system"]);
    assert!(serv.identity.service);
    let chrome = row_named(&all, "chrome.exe");
    assert_eq!(chrome.identity.runs_as, ["user"]);
    assert!(!chrome.identity.service);
}

#[test]
fn a_program_seen_as_two_accounts_lists_the_most_recent_first_each_once() {
    let mut repo = scenario();
    repo.record_context(&[seen(CHROME, USER, None, &[])], at_secs(secs(1, 0))).unwrap();
    repo.record_context(&[seen(CHROME, SYSTEM, None, &[])], at_secs(secs(3, 0))).unwrap();
    repo.record_context(&[seen(CHROME, SYSTEM, None, &[])], at_secs(secs(4, 0))).unwrap();

    let all = rows(&repo, Grouping::None);

    assert_eq!(row_named(&all, "chrome.exe").identity.runs_as, ["system", "user"]);
}

#[test]
fn a_product_row_merges_its_programs_accounts_and_is_a_service_if_any_member_is() {
    let mut repo = scenario();
    repo.record_context(&[seen(SERV, SYSTEM, None, &[("acmeserv", "Acme Service")])], at_secs(secs(1, 0))).unwrap();
    repo.record_context(&[seen(TRANSMIT, SYSTEM, None, &[]), seen(TRANSMIT, USER, None, &[])], at_secs(secs(2, 0)))
        .unwrap();

    let grouped = rows(&repo, Grouping::ByProduct);

    let acme = grouped.iter().find(|row| !row.members.is_empty()).expect("the two Acme programs group");
    assert!(acme.identity.service);
    assert_eq!(acme.identity.runs_as.len(), 2, "system and user, each once: {:?}", acme.identity.runs_as);
    assert!(acme.identity.runs_as.contains(&"system".to_owned()));
    assert!(acme.identity.runs_as.contains(&"user".to_owned()));
    let member = acme.members.iter().find(|m| m.name == "acmeserv.exe").unwrap();
    assert!(member.identity.service, "each member keeps its own facts");
}

#[test]
fn the_identity_panel_lists_the_accounts_and_services_with_when_they_were_seen() {
    let mut repo = scenario();
    repo.record_context(
        &[seen(SERV, SYSTEM, Some(r"NT AUTHORITY\SYSTEM"), &[("acmeserv", "Acme Service")])],
        at_secs(secs(1, 0)),
    )
    .unwrap();
    repo.record_context(&[seen(SERV, SYSTEM, None, &[])], at_secs(secs(3, 0))).unwrap();

    let view = program_history(&repo, now(), 7 * 24 * 3600, SERV).unwrap();

    let identity = &view.identity;
    assert_eq!(identity.accounts.len(), 1);
    let account = &identity.accounts[0];
    assert_eq!((account.kind.as_str(), account.name.as_deref()), ("system", Some(r"NT AUTHORITY\SYSTEM")));
    assert_eq!((account.first_seen_ms, account.last_seen_ms), (secs(1, 0) * 1000, secs(3, 0) * 1000));
    assert_eq!(identity.services.len(), 1);
    assert_eq!(identity.services[0].name, "acmeserv");
    assert_eq!(identity.services[0].display_name.as_deref(), Some("Acme Service"));
    assert!(identity.badge.service && identity.badge.runs_as == ["system"]);
}

#[test]
fn the_identity_panel_of_a_program_never_observed_running_has_no_accounts_not_a_guess() {
    let repo = scenario();

    let view = program_history(&repo, now(), 7 * 24 * 3600, CHROME).unwrap();

    assert!(view.identity.accounts.is_empty() && view.identity.services.is_empty());
    assert!(!view.identity.badge.service);
}
