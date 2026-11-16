//! One read of the Uninstall keys: what is recorded, what is counted, and that a failing read cannot make anything look removed.

use syntrac_core::{sweep_installs, InstallRepository, InstallSweepError, RawInstall, Timestamp};
use syntrac_testkit::{FakeInstallSource, InMemoryRepository};

fn entry(key: &str, name: &str, folder: &str) -> RawInstall {
    RawInstall {
        hive: "machine".to_owned(),
        key_name: key.to_owned(),
        display_name: name.to_owned(),
        publisher: "Example, Inc.".to_owned(),
        install_location: folder.to_owned(),
        ..RawInstall::default()
    }
}

fn at(micros: i64) -> Timestamp {
    Timestamp::from_micros(micros)
}

#[test]
fn entries_become_records_and_entries_with_no_name_are_left_out() {
    let mut repo = InMemoryRepository::default();
    let source = FakeInstallSource::new(vec![
        entry("{A}", "Acme", r"C:\Program Files (x86)\Acme"),
        entry("{B}", "A patch with no folder", ""),
        entry("{C}", "", r"C:\Apps\Nameless"),
    ]);

    let sweep = sweep_installs(&source, &mut repo, at(10)).unwrap();

    assert_eq!((sweep.entries, sweep.recorded, sweep.with_folder, sweep.new), (3, 2, 1, 2));
    assert_eq!(repo.installs().unwrap().len(), 2);
}

#[test]
fn a_second_read_adds_nothing_new_and_a_product_that_was_uninstalled_stops_being_current() {
    let mut repo = InMemoryRepository::default();
    sweep_installs(
        &FakeInstallSource::new(vec![entry("{A}", "Keep", r"C:\Apps\Keep"), entry("{B}", "Remove", r"C:\Apps\Remove")]),
        &mut repo,
        at(10),
    )
    .unwrap();

    let second =
        sweep_installs(&FakeInstallSource::new(vec![entry("{A}", "Keep", r"C:\Apps\Keep")]), &mut repo, at(20))
            .unwrap();

    assert_eq!(second.new, 0);
    let all = repo.installs().unwrap();
    assert!(all.iter().find(|s| s.record.display_name == "Keep").unwrap().current);
    assert!(!all.iter().find(|s| s.record.display_name == "Remove").unwrap().current);
}

#[test]
fn a_read_that_fails_changes_nothing_and_marks_nothing_as_removed() {
    let mut repo = InMemoryRepository::default();
    sweep_installs(&FakeInstallSource::new(vec![entry("{A}", "Keep", r"C:\Apps\Keep")]), &mut repo, at(10)).unwrap();

    let failed = sweep_installs(&FakeInstallSource::failing("the registry is busy"), &mut repo, at(20));

    assert!(matches!(failed, Err(InstallSweepError::Source(ref why)) if why.contains("busy")));
    assert!(repo.installs().unwrap()[0].current, "not read, so not known to be gone");
}

#[test]
fn the_same_entry_twice_is_one_record_and_a_runaway_source_is_bounded() {
    let mut repo = InMemoryRepository::default();
    let twice =
        FakeInstallSource::new(vec![entry("{A}", "Once", r"C:\Apps\Once"), entry("{A}", "Once", r"C:\Apps\Once")]);
    assert_eq!(sweep_installs(&twice, &mut repo, at(10)).unwrap().recorded, 1);

    let flood: Vec<RawInstall> =
        (0..6_000).map(|n| entry(&format!("{{{n}}}"), &format!("Product {n}"), r"C:\Apps\X")).collect();
    let sweep = sweep_installs(&FakeInstallSource::new(flood), &mut InMemoryRepository::default(), at(20)).unwrap();

    assert_eq!(sweep.recorded, 5_000, "bounded");
}
