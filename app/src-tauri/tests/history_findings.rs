//! The Findings tab's read model: what was found, what the user decided, what they said is expected, and whether detection is on.

use syntrac_app_lib::history_findings::{findings_view, review_view, FindingsView};
use syntrac_core::findings::{Candidate, FindingStatus, FindingWrite, NewExpectation, Tier};
use syntrac_core::{DetectionStateRepository, ExpectationRepository, FindingRepository, Timestamp};
use syntrac_testkit::InMemoryRepository;

const SECOND: i64 = 1_000_000;

fn at(secs: i64) -> Timestamp {
    Timestamp::from_micros(secs * SECOND)
}

fn candidate(subject: &str, tier: Tier, when: i64) -> Candidate {
    Candidate {
        kind: "new_program".into(),
        subject: subject.into(),
        subject_label: subject.to_uppercase(),
        tier,
        title: format!("New program {subject}"),
        why: "It was first seen after detection was switched on.".into(),
        evidence: vec!["first line".into(), "second line".into()],
        dedupe_key: format!("new_program:{subject}"),
        level: None,
        at: at(when),
    }
}

fn repo() -> InMemoryRepository {
    let mut repo = InMemoryRepository::default();
    repo.apply_writes(&[
        FindingWrite::Insert(candidate("a.exe", Tier::Unusual, 100)),
        FindingWrite::Insert(candidate("b.exe", Tier::Suspicious, 200)),
        FindingWrite::Insert(candidate("c.exe", Tier::Unusual, 300)),
    ])
    .unwrap();
    repo.set_status(2, FindingStatus::Expected, at(400), None).unwrap();
    repo.set_status(3, FindingStatus::Snoozed, at(410), Some(at(9_000))).unwrap();
    repo
}

fn view(repo: &InMemoryRepository, status: Option<&str>) -> FindingsView {
    findings_view(repo, status, 100, t(9, 0), "c:").unwrap()
}

#[test]
fn a_finding_carries_everything_the_screen_shows_in_milliseconds_and_plain_names() {
    let view = view(&repo(), Some("snoozed"));

    let found = &view.findings[0];
    assert_eq!((found.id, found.kind.as_str(), found.subject.as_str()), (3, "new_program", "c.exe"));
    assert_eq!((found.subject_label.as_str(), found.tier, found.status), ("C.EXE", "unusual", "snoozed"));
    assert_eq!(found.evidence, vec!["first line".to_owned(), "second line".to_owned()]);
    assert_eq!((found.first_at_ms, found.last_at_ms, found.hits), (300_000, 300_000, 1));
    assert_eq!((found.decided_at_ms, found.snoozed_until_ms), (Some(410_000), Some(9_000_000)));
}

#[test]
fn the_counts_cover_every_status_whatever_is_listed() {
    let view = view(&repo(), Some("open"));

    assert_eq!((view.counts.open, view.counts.expected, view.counts.snoozed), (1, 1, 1));
    assert_eq!((view.counts.not_expected, view.counts.resolved), (0, 0));
    assert_eq!(view.findings.len(), 1, "only the open one is listed");
}

#[test]
fn no_status_lists_everything_newest_first_and_a_limit_applies() {
    let all = view(&repo(), None);
    assert_eq!(all.findings.iter().map(|f| f.subject.as_str()).collect::<Vec<_>>(), vec!["c.exe", "b.exe", "a.exe"]);

    let one = findings_view(&repo(), None, 1, t(9, 0), "c:").unwrap();
    assert_eq!(one.findings.len(), 1);
}

#[test]
fn an_unknown_status_is_refused_in_plain_words() {
    let error = findings_view(&repo(), Some("malicious"), 10, t(9, 0), "c:").unwrap_err();

    assert!(error.to_string().contains("status"), "{error}");
}

#[test]
fn detection_is_off_until_a_run_has_switched_it_on_and_then_says_when() {
    let mut repo = repo();
    assert_eq!(view(&repo, None).detection, None);

    repo.switch_on(at(1_000)).unwrap();
    repo.record_run(at(1_300), Some("fails: no data")).unwrap();

    let detection = view(&repo, None).detection.expect("on");
    assert_eq!((detection.switched_on_ms, detection.last_run_ms), (1_000_000, Some(1_300_000)));
    assert_eq!(detection.last_error.as_deref(), Some("fails: no data"));
}

#[test]
fn what_the_user_said_is_expected_is_listed_with_its_note_and_ceiling() {
    let mut repo = repo();
    repo.add_expectation(
        &NewExpectation {
            kind: "new_program".into(),
            subject: "b.exe".into(),
            note: Some("my backup".into()),
            source_finding: Some(2),
            ceiling: Some(2.5),
        },
        at(500),
    )
    .unwrap();

    let view = view(&repo, None);

    let expectation = &view.expectations[0];
    assert_eq!((expectation.kind.as_str(), expectation.subject.as_str()), ("new_program", "b.exe"));
    assert_eq!(
        (expectation.note.as_deref(), expectation.created_ms, expectation.source_finding),
        (Some("my backup"), 500_000, Some(2))
    );
    assert_eq!(expectation.ceiling, Some(2.5));
}

#[test]
fn an_empty_database_gives_an_empty_view() {
    let view = view(&InMemoryRepository::default(), None);

    assert!(view.findings.is_empty() && view.expectations.is_empty());
    assert_eq!((view.counts.open, view.detection), (0, None));
}

#[test]
fn a_program_finding_offers_the_install_folder_as_a_scope_and_other_findings_do_not() {
    let mut repo = InMemoryRepository::default();
    let mut program = candidate(r"c:\program files\acme\bin\acme.exe", Tier::Suspicious, 100);
    program.dedupe_key = "new_program:acme".into();
    let mut other = candidate(r"c:\program files\acme\bin\acme.exe", Tier::Suspicious, 200);
    other.kind = "new_start_entry".into();
    other.dedupe_key = "new_start_entry:acme".into();
    repo.apply_writes(&[FindingWrite::Insert(program), FindingWrite::Insert(other)]).unwrap();

    let view = view(&repo, None);

    let by_kind = |kind: &str| view.findings.iter().find(|f| f.kind == kind).unwrap();
    assert_eq!(by_kind("new_program").folder_scope.as_deref(), Some(r"c:\program files\acme"));
    assert_eq!(
        by_kind("new_program").build_scope,
        None,
        "a protected folder has the whole-folder choice, not a build folder"
    );
    assert_eq!(by_kind("new_start_entry").folder_scope, None);
    assert_eq!(by_kind("new_start_entry").build_scope, None);
}

#[test]
fn a_program_in_a_writable_tool_folder_offers_a_build_folder_and_never_the_whole_folder() {
    let mut repo = InMemoryRepository::default();
    for (n, path) in
        [r"c:\users\someone\.cargo\bin\cargo.exe", r"c:\users\someone\downloads\setup.exe", r"c:\windows\temp\x.exe"]
            .into_iter()
            .enumerate()
    {
        let mut program = candidate(path, Tier::Suspicious, 100 + n as i64);
        program.dedupe_key = format!("new_program:{n}");
        repo.apply_writes(&[FindingWrite::Insert(program)]).unwrap();
    }

    let view = view(&repo, None);

    let scopes = |subject: &str| {
        let finding = view.findings.iter().find(|f| f.subject == subject).unwrap();
        (finding.folder_scope.clone(), finding.build_scope.clone())
    };
    assert_eq!(scopes(r"c:\users\someone\.cargo\bin\cargo.exe"), (None, Some(r"c:\users\someone\.cargo".to_owned())));
    assert_eq!(scopes(r"c:\users\someone\downloads\setup.exe"), (None, None), "a place things get dropped");
    assert_eq!(scopes(r"c:\windows\temp\x.exe"), (None, None));
}

fn expectation(kind: &str, subject: &str) -> NewExpectation {
    NewExpectation { kind: kind.into(), subject: subject.into(), note: None, source_finding: None, ceiling: None }
}

#[test]
fn the_list_of_what_is_expected_says_how_wide_each_one_is_and_warns_about_a_folder_anyone_can_write_to() {
    let mut repo = InMemoryRepository::default();
    for (kind, subject) in [
        ("new_program", r"c:\tool.exe"),
        ("new_program", r"folder:c:\program files\acme"),
        ("new_program", r"folder:c:\users\someone\.cargo"),
        ("new_program", r"folder:d:\program files\acme"),
        ("new_program", r"build:d:\src"),
    ] {
        repo.add_expectation(&expectation(kind, subject), at(10)).unwrap();
    }

    let list = view(&repo, None).expectations;

    let row = |subject: &str| list.iter().find(|e| e.subject == subject).unwrap();
    assert_eq!((row(r"c:\tool.exe").scope, row(r"c:\tool.exe").writable_warning), ("item", false));
    assert_eq!(
        (row(r"folder:c:\program files\acme").scope, row(r"folder:c:\program files\acme").writable_warning),
        ("folder", false)
    );
    let cargo = row(r"folder:c:\users\someone\.cargo");
    assert_eq!(
        (cargo.scope, cargo.writable_warning, cargo.scope_root.as_deref()),
        ("folder", true, Some(r"c:\users\someone\.cargo"))
    );
    assert!(
        row(r"folder:d:\program files\acme").writable_warning,
        "another drive's Program Files follows its own permissions"
    );
    let build = row(r"build:d:\src");
    assert_eq!(
        (build.scope, build.writable_warning),
        ("build", false),
        "chosen on purpose, so labelled rather than warned"
    );
}

// ---- review what is here today (F4)

use syntrac_core::{Executable, FileFacts, IdentityRecord, IdentityRepository, Signature, SignatureStatus};
use syntrac_testkit::identity_contract::seed_executables;
use syntrac_testkit::rollup_contract::t;

const ACME: &str = r"C:\Program Files\Acme\a.exe";
const TOOL: &str = r"C:\Users\someone\Downloads\tool.exe";

fn reviewed_world() -> InMemoryRepository {
    let mut repo = InMemoryRepository::default();
    seed_executables(&mut repo, &[ACME, TOOL]);
    for (path, status, signer) in
        [(ACME, SignatureStatus::Valid, Some("ACME INC")), (TOOL, SignatureStatus::Unsigned, None)]
    {
        repo.save_identity(&IdentityRecord {
            exe: Executable::from_path(path).unwrap(),
            facts: Some(FileFacts {
                signature: Signature { status, signer: signer.map(str::to_owned), ..Signature::default() },
                ..FileFacts::default()
            }),
            inspected_at: Some(t(3, 0)),
            last_attempt: t(3, 0),
            failure: None,
        })
        .unwrap();
    }
    repo
}

#[test]
fn the_review_lists_what_was_here_before_the_switch_on_with_what_deserves_a_look_first() {
    let mut repo = reviewed_world();
    repo.switch_on(t(5, 0)).unwrap();

    let review = review_view(&repo, 60, "c:").unwrap();

    assert_eq!(review.groups.len(), 2);
    assert_eq!(review.groups[0].programs[0].name, "tool.exe", "unsigned in a user folder comes first");
    assert!(review.groups[0].concerns.iter().any(|c| c.contains("not signed")));
    assert_eq!(review.groups[1].root, r"c:\program files\acme");
    assert!(review.groups[1].concerns.is_empty());
    assert_eq!(review.switched_on_ms, Some(t(5, 0).as_micros() / 1000));
    assert_eq!(review.total_groups, 2);
}

#[test]
fn a_program_after_the_switch_on_is_not_in_the_review_and_an_expectation_takes_a_group_out() {
    let mut repo = reviewed_world();
    // Switched on between the two programs: only the first was already here.
    repo.switch_on(t(1, 0)).unwrap();
    assert_eq!(review_view(&repo, 60, "c:").unwrap().groups.len(), 1);

    let mut repo = reviewed_world();
    repo.switch_on(t(5, 0)).unwrap();
    repo.add_expectation(
        &NewExpectation {
            kind: "new_program".into(),
            subject: r"folder:c:\program files\acme".into(),
            note: None,
            source_finding: None,
            ceiling: None,
        },
        t(6, 0),
    )
    .unwrap();
    let review = review_view(&repo, 60, "c:").unwrap();
    assert_eq!(review.groups.len(), 1);
    assert_eq!(review.groups[0].programs[0].name, "tool.exe");
}

#[test]
fn a_group_says_whether_its_whole_folder_may_be_expected_or_only_its_programs_one_by_one() {
    let mut repo = reviewed_world();
    repo.switch_on(t(5, 0)).unwrap();

    let review = review_view(&repo, 60, "c:").unwrap();

    let acme = review.groups.iter().find(|g| g.root == r"c:\program files\acme").unwrap();
    let tool = review.groups.iter().find(|g| g.programs[0].name == "tool.exe").unwrap();
    assert!(acme.whole_folder_allowed);
    assert!(!tool.whole_folder_allowed, "a Downloads folder is somewhere anything gets dropped");
}

// ---- the digest counts what a build folder hides (F6)

#[test]
fn the_digest_counts_new_programs_in_build_folders_so_they_are_never_invisible() {
    let mut repo = InMemoryRepository::default();
    seed_executables(&mut repo, &[r"D:\Src\app\a.exe", r"D:\Src\app\b.exe", r"D:\Elsewhere\c.exe"]);
    repo.add_expectation(&expectation("new_program", r"build:d:\src"), at(1)).unwrap();

    let digest = syntrac_app_lib::history_findings::digest_view(&repo, t(10, 0)).unwrap();
    assert_eq!(digest.new_in_build_folders, 2, "only the programs of the build folder, first seen in the last day");

    // Two days later they are no longer new.
    let later = syntrac_app_lib::history_findings::digest_view(&repo, t(80, 0)).unwrap();
    assert_eq!(later.new_in_build_folders, 0);
}

#[test]
fn the_list_is_limited_and_says_how_many_groups_there_are() {
    let mut repo = reviewed_world();
    repo.switch_on(t(5, 0)).unwrap();

    let review = review_view(&repo, 1, "c:").unwrap();

    assert_eq!((review.groups.len(), review.total_groups), (1, 2));
}
