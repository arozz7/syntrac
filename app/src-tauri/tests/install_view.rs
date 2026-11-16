//! What the app says about where a program came from according to the Uninstall registry keys: the entries in the identity panel,
//! and a fallback label for a product group that has neither a product name nor a publisher. A clue, never a standing.

use syntrac_app_lib::history_programs::{program_history, program_ranking, Grouping, ProgramMetricParam};
use syntrac_core::{
    roll_up_due, Executable, FileFacts, IdentityRecord, IdentityRepository, InstallRecord, InstallRepository,
    MetricsRepository, ProcessRepository, RollupSettings, Signature, SignatureStatus, Timestamp, VersionInfo,
};
use syntrac_testkit::fixtures::{at_secs, metrics_sample, process_metric, running_process};
use syntrac_testkit::InMemoryRepository;

const HOUR: i64 = 3600;
const BASE: i64 = 1_794_571_200 / 3600 * 3600;
const ONE: &str = r"C:\Program Files\Acme\one.exe";
const TWO: &str = r"C:\Program Files\Acme\sub\two.exe";
const OTHER: &str = r"C:\Program Files\Other\other.exe";

fn secs(hour: i64, minutes: i64) -> i64 {
    BASE + hour * HOUR + minutes * 60
}

fn now() -> Timestamp {
    at_secs(BASE + 20 * HOUR)
}

fn record(name: &str, folder: Option<&str>) -> InstallRecord {
    InstallRecord {
        hive: "machine".to_owned(),
        key_name: format!("{{{name}}}"),
        display_name: name.to_owned(),
        publisher: Some("Acme, Inc.".to_owned()),
        version: Some("3.1".to_owned()),
        install_date: Some("20260504".to_owned()),
        location: folder.map(str::to_owned),
        derived: false,
        system_component: false,
    }
}

fn facts(signer: Option<&str>, product: Option<&str>) -> FileFacts {
    FileFacts {
        signature: Signature {
            status: if signer.is_some() { SignatureStatus::Valid } else { SignatureStatus::Unsigned },
            signer: signer.map(str::to_owned),
            ..Signature::default()
        },
        version: VersionInfo { product: product.map(str::to_owned), ..VersionInfo::default() },
        ..FileFacts::default()
    }
}

/// Three programs that were running, with identity records that say whatever `signer` and `product` say.
fn scenario(signer: Option<&str>, product: Option<&str>) -> InMemoryRepository {
    let mut repo = InMemoryRepository::default();
    for (index, path) in [ONE, TWO, OTHER].iter().enumerate() {
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
    for path in [ONE, TWO, OTHER] {
        repo.save_identity(&IdentityRecord {
            exe: Executable::from_path(path).unwrap(),
            facts: Some(facts(signer, product)),
            inspected_at: Some(at_secs(BASE)),
            last_attempt: at_secs(BASE),
            failure: None,
        })
        .unwrap();
    }
    repo
}

fn group_names(repo: &InMemoryRepository) -> Vec<String> {
    program_ranking(repo, now(), 7 * 24 * 3600, ProgramMetricParam::Cpu, 20, Grouping::ByProduct)
        .unwrap()
        .programs
        .into_iter()
        .map(|row| row.name)
        .collect()
}

#[test]
fn the_panel_of_a_program_shows_the_product_an_installer_registered_for_its_folder() {
    let mut repo = scenario(None, None);
    repo.record_installs(&[record("Acme Suite", Some(r"c:\program files\acme"))], at_secs(secs(6, 0))).unwrap();

    let view = program_history(&repo, now(), 7 * 24 * 3600, TWO).unwrap();

    let registered = &view.identity.registered;
    assert_eq!(registered.len(), 1);
    assert_eq!(
        (registered[0].name.as_str(), registered[0].publisher.as_deref(), registered[0].version.as_deref()),
        ("Acme Suite", Some("Acme, Inc."), Some("3.1"))
    );
    assert_eq!(
        (registered[0].folder.as_str(), registered[0].folder_derived, registered[0].current),
        (r"c:\program files\acme", false, true)
    );
    assert_eq!(registered[0].installed.as_deref(), Some("20260504"));
    assert_eq!(view.identity.badge.registered_as.as_deref(), Some("Acme Suite"));
}

#[test]
fn a_program_outside_every_registered_folder_has_nothing_registered_not_a_claim() {
    let mut repo = scenario(None, None);
    repo.record_installs(&[record("Acme Suite", Some(r"c:\program files\acme"))], at_secs(secs(6, 0))).unwrap();

    let view = program_history(&repo, now(), 7 * 24 * 3600, OTHER).unwrap();

    assert!(view.identity.registered.is_empty());
    assert_eq!(view.identity.badge.registered_as, None);
}

#[test]
fn a_product_that_was_uninstalled_is_still_listed_but_says_so_and_does_not_name_the_group() {
    let mut repo = scenario(None, None);
    repo.record_installs(&[record("Acme Suite", Some(r"c:\program files\acme"))], at_secs(secs(6, 0))).unwrap();
    repo.record_installs(&[], at_secs(secs(8, 0))).unwrap();

    let view = program_history(&repo, now(), 7 * 24 * 3600, ONE).unwrap();

    assert!(!view.identity.registered[0].current);
    assert_eq!(view.identity.badge.registered_as, None, "a product that is gone is not offered as a name");
}

#[test]
fn a_group_with_no_product_name_and_no_publisher_takes_the_registered_name_before_the_folder_name() {
    let mut repo = scenario(None, None);
    assert!(group_names(&repo).contains(&"Acme".to_owned()), "without a record: the folder name");

    repo.record_installs(&[record("Acme Suite", Some(r"c:\program files\acme"))], at_secs(secs(6, 0))).unwrap();

    let names = group_names(&repo);
    assert!(names.contains(&"Acme Suite".to_owned()), "{names:?}");
    assert!(names.contains(&"other.exe".to_owned()), "a program outside the folder is untouched: {names:?}");
}

#[test]
fn the_registered_name_is_below_the_product_name_and_the_signer_and_never_merges_groups() {
    let mut with_product = scenario(None, Some("Acme Product"));
    with_product.record_installs(&[record("Acme Suite", Some(r"c:\program files\acme"))], at_secs(secs(6, 0))).unwrap();
    assert!(group_names(&with_product).contains(&"Acme Product".to_owned()));

    let mut signed = scenario(Some("ACME, INC."), None);
    signed.record_installs(&[record("Acme Suite", Some(r"c:\program files\acme"))], at_secs(secs(6, 0))).unwrap();
    assert!(group_names(&signed).contains(&"ACME, INC.".to_owned()));

    // Two groups stay two groups: a record never merges anything.
    let mut separate = scenario(None, None);
    separate.record_installs(&[record("Everything", Some(r"c:\program files"))], at_secs(secs(6, 0))).unwrap();
    assert_eq!(group_names(&separate).len(), 2);
}
