//! What the app says about HOW a program gets started: a "starts at boot" flag on a ranking row, and the entries (service, task,
//! Run key, Startup folder) in the identity panel of one program. Evidence for the screen, never a verdict.

use syntrac_app_lib::history_programs::{program_history, program_ranking, Grouping, ProgramMetricParam};
use syntrac_core::{
    roll_up_due, Executable, FileFacts, IdentityRecord, IdentityRepository, LauncherKind, LauncherObservation,
    LauncherRepository, MetricsRepository, ProcessRepository, RollupSettings, Signature, SignatureStatus, Timestamp,
    VersionInfo,
};
use syntrac_testkit::fixtures::{at_secs, metrics_sample, process_metric, running_process};
use syntrac_testkit::InMemoryRepository;

const HOUR: i64 = 3600;
const BASE: i64 = 1_794_571_200 / 3600 * 3600;
const SERV: &str = r"C:\Program Files (x86)\Acme\acmeserv.exe";
const TRANSMIT: &str = r"C:\Program Files (x86)\Acme\x64\acmetransmit64.exe";
const CHROME: &str = r"C:\Program Files\Google\Chrome\chrome.exe";

fn secs(hour: i64, minutes: i64) -> i64 {
    BASE + hour * HOUR + minutes * 60
}

fn now() -> Timestamp {
    at_secs(BASE + 20 * HOUR)
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

/// Three programs that were running; Acme's two are signed (so they group). Nothing about how they start yet.
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
    program_ranking(repo, now(), 7 * 24 * 3600, ProgramMetricParam::Cpu, 20, grouping).unwrap().programs
}

fn row_of<'a>(
    rows: &'a [syntrac_app_lib::history_programs::ProgramRowView],
    name: &str,
) -> &'a syntrac_app_lib::history_programs::ProgramRowView {
    rows.iter().find(|r| r.name == name).unwrap_or_else(|| panic!("no row {name}"))
}

#[test]
fn a_program_with_a_start_entry_is_flagged_and_one_without_is_not() {
    let mut repo = scenario();
    repo.record_launchers(
        &[entry(SERV, LauncherKind::ServiceAuto, "acmeserv", "automatic", "-noconsole")],
        &[LauncherKind::ServiceAuto],
        at_secs(secs(6, 0)),
    )
    .unwrap();

    let list = rows(&repo, Grouping::None);

    assert!(row_of(&list, "acmeserv.exe").identity.starts_at_boot);
    assert!(
        !row_of(&list, "chrome.exe").identity.starts_at_boot,
        "no entry found is not 'does not autostart': just not flagged"
    );
}

#[test]
fn a_service_that_only_starts_on_demand_and_an_entry_that_is_gone_do_not_count_as_starting_at_boot() {
    let mut repo = scenario();
    repo.record_launchers(
        &[
            entry(SERV, LauncherKind::ServiceOther, "acmeserv", "on demand", ""),
            entry(CHROME, LauncherKind::RunKey, r"HKLM\Run\Chrome", "", ""),
        ],
        &[LauncherKind::ServiceOther, LauncherKind::RunKey],
        at_secs(secs(6, 0)),
    )
    .unwrap();
    // The Run key was read again later and the entry was no longer there.
    repo.record_launchers(&[], &[LauncherKind::RunKey], at_secs(secs(7, 0))).unwrap();

    let list = rows(&repo, Grouping::None);

    assert!(!row_of(&list, "acmeserv.exe").identity.starts_at_boot);
    assert!(!row_of(&list, "chrome.exe").identity.starts_at_boot);
}

#[test]
fn a_product_group_starts_at_boot_when_any_of_its_programs_does() {
    let mut repo = scenario();
    repo.record_launchers(
        &[entry(TRANSMIT, LauncherKind::Task, r"\Acme\Transmit", "logon", "")],
        &[LauncherKind::Task],
        at_secs(secs(6, 0)),
    )
    .unwrap();

    let list = rows(&repo, Grouping::ByProduct);

    let group = list.iter().find(|r| r.name.contains("Acme")).expect("the Acme group");
    assert!(group.identity.starts_at_boot);
}

#[test]
fn the_panel_of_one_program_lists_its_entries_with_where_when_and_whether_they_are_still_there() {
    let mut repo = scenario();
    repo.record_launchers(
        &[
            entry(SERV, LauncherKind::ServiceAuto, "acmeserv", "automatic", "-noconsole"),
            entry(SERV, LauncherKind::RunKey, r"HKLM\Run\acme", "", ""),
        ],
        &[LauncherKind::ServiceAuto, LauncherKind::RunKey],
        at_secs(secs(6, 0)),
    )
    .unwrap();
    repo.record_launchers(&[], &[LauncherKind::RunKey], at_secs(secs(8, 0))).unwrap();

    let view = program_history(&repo, now(), 7 * 24 * 3600, SERV).unwrap();

    let launchers = &view.identity.launchers;
    assert_eq!(launchers.len(), 2);
    let service = launchers.iter().find(|l| l.kind == "service_auto").unwrap();
    assert_eq!(
        (service.location.as_str(), service.detail.as_str(), service.arguments.as_str()),
        ("acmeserv", "automatic", "-noconsole")
    );
    assert_eq!(service.first_seen_ms, secs(6, 0) * 1000);
    assert!(service.current);
    assert!(!launchers.iter().find(|l| l.kind == "run_key").unwrap().current, "gone from a complete read");
}

#[test]
fn a_program_with_no_entries_has_an_empty_list_not_a_claim() {
    let repo = scenario();

    let view = program_history(&repo, now(), 7 * 24 * 3600, CHROME).unwrap();

    assert!(view.identity.launchers.is_empty());
    assert!(!view.identity.badge.starts_at_boot);
}
