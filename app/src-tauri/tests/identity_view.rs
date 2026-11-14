//! What the app shows about WHO a program is: a badge on each ranking row, an identity panel for one program, and the
//! "group by product" folding of the four Acme programs into one row.

use syntrac_app_lib::history_programs::{program_history, program_ranking, Grouping, ProgramMetricParam};
use syntrac_app_lib::identity_view::{badge, detail, merged_badge, product_name};
use syntrac_core::{
    roll_up_due, Executable, FileFacts, FileStamp, IdentityFailure, IdentityRecord, IdentityRepository,
    InspectErrorKind, MetricsRepository, ProcessRepository, RollupSettings, Signature, SignatureKind, SignatureStatus,
    Timestamp, VersionInfo,
};
use syntrac_testkit::fixtures::{at_secs, metrics_sample, process_metric, running_process};
use syntrac_testkit::InMemoryRepository;

const HOUR: i64 = 3600;
const BASE: i64 = 1_794_571_200 / 3600 * 3600;
const SERV: &str = r"C:\Program Files (x86)\Acme\acmeserv.exe";
const TRANSMIT: &str = r"C:\Program Files (x86)\Acme\x64\acmetransmit64.exe";
const CHROME: &str = r"C:\Program Files\Google\Chrome\chrome.exe";
const TEMPO: &str = r"C:\Users\alice\AppData\Local\Temp\x\tool.exe";

fn secs(hour: i64, minutes: i64) -> i64 {
    BASE + hour * HOUR + minutes * 60
}

fn now() -> Timestamp {
    at_secs(BASE + 20 * HOUR)
}

fn signed(signer: &str, product: Option<&str>) -> FileFacts {
    FileFacts {
        stamp: FileStamp { size: 1234, modified: Some(at_secs(BASE)) },
        sha256: Some("ab".repeat(32)),
        signature: Signature {
            status: SignatureStatus::Valid,
            signer: Some(signer.to_owned()),
            thumbprint: Some("AA11".into()),
            kind: Some(SignatureKind::Embedded),
        },
        version: VersionInfo {
            company: Some("Company".into()),
            product: product.map(str::to_owned),
            description: Some("A program".into()),
            original_filename: Some("orig.exe".into()),
            file_version: Some("1.2.3".into()),
        },
        zone_id: None,
    }
}

fn unsigned() -> FileFacts {
    FileFacts { stamp: FileStamp { size: 10, modified: None }, ..FileFacts::default() }
}

fn record(path: &str, facts: Option<FileFacts>) -> IdentityRecord {
    IdentityRecord {
        exe: Executable::from_path(path).unwrap(),
        inspected_at: facts.as_ref().map(|_| at_secs(BASE + 3 * HOUR)),
        facts,
        last_attempt: at_secs(BASE + 3 * HOUR),
        failure: None,
    }
}

/// Acme (two programs), Chrome and a temp-folder tool, each busy in hour 0; identities for the first three only.
fn scenario() -> InMemoryRepository {
    let mut repo = InMemoryRepository::default();
    let programs = [(10_u32, SERV, 1.0_f32), (11, TRANSMIT, 2.0), (12, CHROME, 4.0), (13, TEMPO, 0.5)];
    for (pid, path, cpu) in programs {
        let mut process = running_process(pid, at_secs(secs(0, 0)));
        process.image_path = path.to_owned();
        repo.record_start(process.clone()).unwrap();
        repo.append_metrics(&metrics_sample(
            secs(0, 10 + i64::from(pid)),
            30.0,
            vec![process_metric(pid, Some(process.id), cpu)],
            0.0,
            0,
        ))
        .unwrap();
    }
    let settings = RollupSettings { settle: std::time::Duration::from_secs(300), max_hours_per_pass: 10 };
    roll_up_due(&mut repo, at_secs(secs(5, 0)), &settings).unwrap();
    for path in [SERV, TRANSMIT] {
        repo.save_identity(&record(path, Some(signed("ACME SOFTWARE, INC.", Some("Acme Backup"))))).unwrap();
    }
    repo.save_identity(&record(CHROME, Some(signed("Google LLC", Some("Google Chrome"))))).unwrap();
    repo
}

fn ranking(grouping: Grouping) -> syntrac_app_lib::history_programs::ProgramsView {
    program_ranking(&scenario(), now(), 7 * 24 * 3600, ProgramMetricParam::Cpu, 10, grouping).unwrap()
}

#[test]
fn a_program_nobody_has_looked_at_yet_says_so_and_still_has_a_location() {
    let view = badge(TEMPO, None);

    assert_eq!(view.state, "pending");
    assert_eq!((view.signature, view.publisher, view.product), (None, None, None));
    assert_eq!(view.location, "temporary");
}

#[test]
fn a_signed_program_shows_its_signature_publisher_and_product() {
    let view = badge(SERV, Some(&record(SERV, Some(signed("ACME SOFTWARE, INC.", Some("Acme Backup"))))));

    assert_eq!(view.state, "known");
    assert_eq!(view.signature.as_deref(), Some("valid"));
    assert_eq!(view.publisher.as_deref(), Some("ACME SOFTWARE, INC."));
    assert_eq!(view.product.as_deref(), Some("Acme Backup"));
    assert_eq!(view.location, "program_files");
}

#[test]
fn an_unsigned_program_falls_back_to_the_company_in_its_version_information() {
    let mut facts = unsigned();
    facts.version.company = Some("Acme Ltd".into());

    let view = badge(CHROME, Some(&record(CHROME, Some(facts))));

    assert_eq!(view.signature.as_deref(), Some("unsigned"));
    assert_eq!(view.publisher.as_deref(), Some("Acme Ltd"));
}

#[test]
fn a_file_that_could_not_be_read_is_unreadable_not_unsigned() {
    let mut gone = record(CHROME, None);
    gone.failure =
        Some(IdentityFailure { kind: InspectErrorKind::NotFound, message: "no such file".into(), attempts: 2 });

    let view = badge(CHROME, Some(&gone));

    assert_eq!(view.state, "unreadable");
    assert_eq!(view.signature, None, "we do not know whether it is signed");
}

#[test]
fn the_detail_carries_everything_that_was_measured_in_plain_fields() {
    let mut facts = signed("ACME SOFTWARE, INC.", Some("Acme Backup"));
    facts.zone_id = Some(3);
    let mut with_failure = record(SERV, Some(facts));
    with_failure.failure =
        Some(IdentityFailure { kind: InspectErrorKind::AccessDenied, message: "denied".into(), attempts: 1 });

    let view = detail(SERV, Some(&with_failure));

    assert_eq!(view.badge.state, "known");
    assert_eq!(view.signer.as_deref(), Some("ACME SOFTWARE, INC."));
    assert_eq!(view.signature_kind.as_deref(), Some("embedded"));
    assert_eq!(view.thumbprint.as_deref(), Some("AA11"));
    assert_eq!((view.company.as_deref(), view.description.as_deref()), (Some("Company"), Some("A program")));
    assert_eq!((view.original_filename.as_deref(), view.file_version.as_deref()), (Some("orig.exe"), Some("1.2.3")));
    assert_eq!(view.sha256.as_deref().map(str::len), Some(64));
    assert_eq!((view.size_bytes, view.modified_ms), (Some(1234), Some(BASE * 1000)));
    assert_eq!(view.inspected_at_ms, Some((BASE + 3 * HOUR) * 1000));
    assert!(view.downloaded);
    assert_eq!(view.install_root.as_deref(), Some(r"c:\program files (x86)\acme"));
    let failure = view.failure.expect("the last look failed, and the panel says so");
    assert_eq!((failure.kind.as_str(), failure.attempts), ("access_denied", 1));
}

#[test]
fn a_program_without_a_record_has_a_pending_detail() {
    let view = detail(CHROME, None);

    assert_eq!(view.badge.state, "pending");
    assert!(view.sha256.is_none() && view.failure.is_none() && !view.downloaded);
}

#[test]
fn ranking_rows_carry_the_badge_of_their_program() {
    let view = ranking(Grouping::None);

    let chrome = view.programs.iter().find(|p| p.name == "chrome.exe").unwrap();
    assert_eq!(chrome.identity.signature.as_deref(), Some("valid"));
    assert_eq!(chrome.identity.publisher.as_deref(), Some("Google LLC"));
    let temp = view.programs.iter().find(|p| p.name == "tool.exe").unwrap();
    assert_eq!((temp.identity.state.as_str(), temp.identity.location.as_str()), ("pending", "temporary"));
    assert!(view.programs.iter().all(|p| p.members.is_empty()), "nothing is grouped by default");
}

#[test]
fn grouped_by_product_the_acme_programs_are_one_row_with_their_members() {
    let view = ranking(Grouping::ByProduct);

    let names: Vec<_> = view.programs.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        names,
        ["chrome.exe", "Acme Backup", "tool.exe"],
        "Acme: 1% + 2% = 3%, ahead of the 0.5% tool"
    );
    let acme = &view.programs[1];
    assert_eq!(acme.path, None, "a product row has no single executable to drill into");
    assert!((acme.cpu_avg_percent - 0.75).abs() < 1e-9, "(1% + 2%) over the 4 samples of the range");
    let members: Vec<_> = acme.members.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(members, ["acmeserv.exe", "acmetransmit64.exe"]);
    assert!(acme.members.iter().all(|m| !m.path.is_empty()), "each member can be opened");
    assert_eq!(acme.identity.signature.as_deref(), Some("valid"));
    assert_eq!(acme.identity.publisher.as_deref(), Some("ACME SOFTWARE, INC."));
}

#[test]
fn a_program_alone_in_its_product_stays_an_ordinary_row() {
    let view = ranking(Grouping::ByProduct);

    let chrome = view.programs.iter().find(|p| p.name == "chrome.exe").unwrap();
    assert_eq!(chrome.path.as_deref(), Some(CHROME));
    assert!(chrome.members.is_empty());
}

#[test]
fn the_combined_row_is_the_sum_of_its_members() {
    let separate = ranking(Grouping::None);
    let grouped = ranking(Grouping::ByProduct);

    let cpu = |name: &str, view: &syntrac_app_lib::history_programs::ProgramsView| {
        view.programs.iter().find(|p| p.name == name).map(|p| p.cpu_avg_percent).unwrap()
    };
    let sum = cpu("acmeserv.exe", &separate) + cpu("acmetransmit64.exe", &separate);
    assert!((cpu("Acme Backup", &grouped) - sum).abs() < 1e-9);
}

#[test]
fn a_product_is_named_by_its_product_name_then_its_publisher_then_its_folder() {
    let named = |product: Option<&str>, publisher: Option<&str>| {
        let mut b = badge(SERV, None);
        b.product = product.map(str::to_owned);
        b.publisher = publisher.map(str::to_owned);
        b
    };

    let both = [named(Some("Acme Backup"), Some("ACME SOFTWARE, INC.")), named(Some("Acme Backup"), None)];
    assert_eq!(product_name(&[SERV, TRANSMIT], &both), "Acme Backup");
    let publisher_only = [named(None, Some("ACME SOFTWARE, INC."))];
    assert_eq!(product_name(&[SERV], &publisher_only), "ACME SOFTWARE, INC.");
    let nothing = [named(None, None), named(None, None)];
    assert_eq!(product_name(&[SERV, TRANSMIT], &nothing), "Acme", "the folder, spelled as on disk");
}

#[test]
fn the_combined_badge_shows_the_worst_signature_among_the_members() {
    let mut altered = badge(SERV, None);
    altered.state = "known".into();
    altered.signature = Some("altered".into());
    let mut valid = badge(TRANSMIT, None);
    valid.state = "known".into();
    valid.signature = Some("valid".into());
    let pending = badge(CHROME, None);

    assert_eq!(merged_badge(&[valid.clone(), altered]).signature.as_deref(), Some("altered"));
    assert_eq!(merged_badge(&[valid.clone(), pending.clone()]).signature.as_deref(), Some("valid"));
    assert_eq!(merged_badge(&[pending.clone(), pending]).state, "pending");
}

#[test]
fn one_programs_history_carries_its_identity_detail() {
    let history = program_history(&scenario(), now(), 7 * 24 * 3600, CHROME).unwrap();

    assert_eq!(history.identity.badge.signature.as_deref(), Some("valid"));
    assert_eq!(history.identity.sha256.as_deref().map(str::len), Some(64));

    let unseen = program_history(&scenario(), now(), 7 * 24 * 3600, TEMPO).unwrap();
    assert_eq!(unseen.identity.badge.state, "pending");
}

/// A repository with these programs busy in hour 0 (so they can be ranked), and identities for the ones given.
fn repo_with(programs: &[(&str, Option<FileFacts>)]) -> InMemoryRepository {
    let mut repo = InMemoryRepository::default();
    for (index, (path, _)) in programs.iter().enumerate() {
        let pid = 100 + index as u32;
        let mut process = running_process(pid, at_secs(secs(0, 0)));
        process.image_path = (*path).to_owned();
        repo.record_start(process.clone()).unwrap();
        repo.append_metrics(&metrics_sample(
            secs(0, 10 + index as i64),
            30.0,
            vec![process_metric(pid, Some(process.id), 1.0)],
            0.0,
            0,
        ))
        .unwrap();
    }
    let settings = RollupSettings { settle: std::time::Duration::from_secs(300), max_hours_per_pass: 10 };
    roll_up_due(&mut repo, at_secs(secs(5, 0)), &settings).unwrap();
    for (path, facts) in programs {
        if let Some(facts) = facts {
            repo.save_identity(&record(path, Some(facts.clone()))).unwrap();
        }
    }
    repo
}

fn grouped(repo: &InMemoryRepository) -> Vec<syntrac_app_lib::history_programs::ProgramRowView> {
    program_ranking(repo, now(), 7 * 24 * 3600, ProgramMetricParam::Cpu, 50, Grouping::ByProduct).unwrap().programs
}

#[test]
fn programs_nobody_has_read_yet_are_not_folded_together_by_their_folder() {
    let repo = repo_with(&[(r"C:\Program Files\Acme\a.exe", None), (r"C:\Program Files\Acme\b.exe", None)]);

    let rows = grouped(&repo);

    assert_eq!(rows.len(), 2, "without knowing who made them, a shared folder is only a guess");
    assert!(rows.iter().all(|row| row.members.is_empty()));
}

#[test]
fn files_that_were_read_but_say_nothing_about_their_maker_are_folded_by_folder() {
    let repo = repo_with(&[
        (r"C:\Program Files\Git\usr\bin\awk.exe", Some(unsigned())),
        (r"C:\Program Files\Git\usr\bin\cat.exe", Some(unsigned())),
    ]);

    let rows = grouped(&repo);

    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].name.as_str(), rows[0].members.len()),
        ("Git", 2),
        "named after the folder, spelled as on disk"
    );
}

#[test]
fn two_product_rows_with_the_same_name_say_who_made_each() {
    let tool = |publisher: &str| signed(publisher, Some("Tool Suite"));
    let repo = repo_with(&[
        (r"C:\Program Files\A\one.exe", Some(tool("Publisher A"))),
        (r"C:\Program Files\A\two.exe", Some(tool("Publisher A"))),
        (r"C:\Program Files\B\one.exe", Some(tool("Publisher B"))),
        (r"C:\Program Files\B\two.exe", Some(tool("Publisher B"))),
    ]);

    let names: Vec<String> = grouped(&repo).into_iter().map(|row| row.name).collect();

    assert_eq!(names, ["Tool Suite (Publisher A)", "Tool Suite (Publisher B)"]);
}

#[test]
fn a_name_that_is_not_repeated_is_left_alone() {
    let names: Vec<String> = ranking(Grouping::ByProduct).programs.into_iter().map(|row| row.name).collect();

    assert!(names.iter().all(|name| !name.contains('(')), "{names:?}");
}
