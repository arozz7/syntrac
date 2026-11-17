//! "Review what is here today" (docs/ALERTS-PLAN.md, F4): the programs that were already here when detection was switched on, grouped
//! by product, so the user can look at them once and say what is expected. All pure.

use syntrac_core::review::review_groups;
use syntrac_core::{
    Executable, Expectation, FileFacts, IdentityEntry, IdentityRecord, KnownProduct, LedgerEntry, Signature,
    SignatureStatus, Timestamp, VersionInfo,
};

const DAY: i64 = 86_400 * 1_000_000;

fn at(day: i64) -> Timestamp {
    Timestamp::from_micros(day * DAY)
}

fn facts(status: SignatureStatus, signer: Option<&str>, product: Option<&str>) -> FileFacts {
    FileFacts {
        signature: Signature { status, signer: signer.map(str::to_owned), ..Signature::default() },
        version: VersionInfo { product: product.map(str::to_owned), ..VersionInfo::default() },
        ..FileFacts::default()
    }
}

fn entry(path: &str, first_seen_day: Option<i64>, facts: Option<FileFacts>) -> IdentityEntry {
    let exe = Executable::from_path(path).unwrap();
    IdentityEntry {
        exe: exe.clone(),
        seen: first_seen_day.map(|day| LedgerEntry { first_seen: at(day), last_seen: at(day + 1) }),
        record: facts.map(|facts| IdentityRecord {
            exe,
            facts: Some(facts),
            inspected_at: Some(at(1)),
            last_attempt: at(1),
            failure: None,
        }),
    }
}

fn expectation(subject: &str) -> Expectation {
    Expectation {
        id: 1,
        kind: "new_program".into(),
        subject: subject.into(),
        note: None,
        created_at: at(1),
        source_finding: None,
        ceiling: None,
    }
}

const ON: Option<Timestamp> = Some(Timestamp::from_micros(10 * DAY));

fn valid(signer: &str, product: &str) -> FileFacts {
    facts(SignatureStatus::Valid, Some(signer), Some(product))
}

#[test]
fn programs_of_one_maker_in_one_install_folder_are_one_group_and_other_folders_are_other_groups() {
    let entries = vec![
        entry(r"C:\Program Files\Vendor\a.exe", Some(2), Some(valid("VENDOR INC", "Vendor Suite"))),
        entry(r"C:\Program Files\Vendor\sub\b.exe", Some(3), Some(valid("VENDOR INC", "Vendor Suite"))),
        entry(r"C:\Program Files\Other\c.exe", Some(3), Some(valid("OTHER INC", "Other"))),
    ];

    let groups = review_groups(&entries, &[], &[], ON);

    assert_eq!(groups.len(), 2);
    let vendor = groups.iter().find(|g| g.root == r"c:\program files\vendor").unwrap();
    assert_eq!(vendor.programs.len(), 2);
    assert_eq!(vendor.publisher.as_deref(), Some("vendor inc"));
    assert_eq!(vendor.label, "Vendor Suite", "the product name the files give themselves, when there is one");
}

#[test]
fn only_programs_already_here_when_detection_was_switched_on_are_reviewed() {
    let entries = vec![
        entry(r"C:\Program Files\Old\old.exe", Some(5), Some(valid("OLD INC", "Old"))),
        entry(r"C:\Program Files\New\new.exe", Some(12), Some(valid("NEW INC", "New"))),
        entry(r"C:\Program Files\Never\never.exe", None, Some(valid("NEVER INC", "Never"))),
    ];

    let groups = review_groups(&entries, &[], &[], ON);

    assert_eq!(groups.len(), 1, "a program after the switch-on is a finding, and one never seen running is not here");
    assert_eq!(groups[0].programs[0].name, "old.exe");
}

#[test]
fn before_detection_is_switched_on_every_program_seen_running_is_offered() {
    let entries = vec![
        entry(r"C:\Program Files\A\a.exe", Some(5), Some(valid("A INC", "A"))),
        entry(r"C:\Program Files\B\b.exe", Some(50), Some(valid("B INC", "B"))),
    ];

    assert_eq!(review_groups(&entries, &[], &[], None).len(), 2);
}

#[test]
fn a_known_product_and_a_folder_or_program_expectation_take_a_group_out_of_the_review() {
    let entries = vec![
        entry(r"C:\Program Files\Vendor\a.exe", Some(2), Some(valid("VENDOR INC", "Vendor Suite"))),
        entry(r"C:\Users\someone\.cargo\bin\cargo.exe", Some(2), Some(facts(SignatureStatus::Unsigned, None, None))),
        entry(r"C:\Tools\single.exe", Some(2), Some(facts(SignatureStatus::Unsigned, None, None))),
        entry(r"C:\Program Files\Left\left.exe", Some(2), Some(valid("LEFT INC", "Left"))),
    ];
    let known = KnownProduct {
        id: 1,
        label: "Vendor".into(),
        publisher: Some("vendor inc".into()),
        root: r"c:\program files\vendor".into(),
        product_name: None,
        note: None,
        created_at: at(1),
    };
    let expectations = [expectation(r"folder:c:\users\someone\.cargo"), expectation(r"c:\tools\single.exe")];

    let groups = review_groups(&entries, &[known], &expectations, ON);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].programs[0].name, "left.exe");
}

#[test]
fn groups_with_something_to_look_at_come_first_and_say_what() {
    let entries = vec![
        entry(r"C:\Program Files\Many\a.exe", Some(2), Some(valid("MANY INC", "Many"))),
        entry(r"C:\Program Files\Many\b.exe", Some(2), Some(valid("MANY INC", "Many"))),
        entry(r"C:\Users\someone\Downloads\setup.exe", Some(2), Some(facts(SignatureStatus::Unsigned, None, None))),
    ];

    let groups = review_groups(&entries, &[], &[], ON);

    assert_eq!(
        groups[0].programs[0].name, "setup.exe",
        "an unsigned program in a user folder is listed before a bigger quiet group"
    );
    assert!(groups[0].concerns.iter().any(|c| c.contains("not signed")), "{:?}", groups[0].concerns);
    assert!(groups[1].concerns.is_empty());
}

#[test]
fn the_label_is_the_product_then_the_signer_then_the_folder() {
    let entries = vec![
        entry(r"C:\Program Files\P\p.exe", Some(2), Some(valid("MAKER INC", "Named Product"))),
        entry(r"C:\Program Files\S\s.exe", Some(2), Some(facts(SignatureStatus::Valid, Some("Signer Only"), None))),
        entry(r"C:\Tools\mytool\t.exe", Some(2), Some(facts(SignatureStatus::Unsigned, None, None))),
    ];

    let groups = review_groups(&entries, &[], &[], ON);

    let label = |root: &str| groups.iter().find(|g| g.root == root).unwrap().label.clone();
    assert_eq!(label(r"c:\program files\p"), "Named Product");
    assert_eq!(label(r"c:\program files\s"), "Signer Only");
    assert_eq!(label(r"c:\tools"), "tools");
}

#[test]
fn a_path_with_no_folder_to_group_by_is_left_out() {
    let entries = vec![entry("bare.exe", Some(2), Some(facts(SignatureStatus::Unsigned, None, None)))];

    assert!(review_groups(&entries, &[], &[], ON).is_empty());
}
