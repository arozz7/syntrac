//! The "new thing" rules as the detect worker runs them: switch-on, what counts as new, and that a repeat is a hit, not a new finding.

use std::time::Duration;

use syntrac_core::findings::ReconcileSettings;
use syntrac_core::{FindingRepository, NewThingsDetector};
use syntrac_service::{DetectTick, DetectWorker, DetectWorkerSettings};
use syntrac_testkit::identity_contract::seed_executables;
use syntrac_testkit::rollup_contract::t;
use syntrac_testkit::{InMemoryRepository, ManualClock};

const OLD: &str = r"C:\Program Files\Old\old.exe";
const NEW: &str = r"C:\Users\someone\Downloads\fresh.exe";

fn settings() -> DetectWorkerSettings {
    DetectWorkerSettings { interval: Duration::from_secs(300), reconcile: ReconcileSettings { max_new: 50 } }
}

/// OLD ran in hour 0, NEW in hour 1.
fn data() -> InMemoryRepository {
    let mut repo = InMemoryRepository::default();
    seed_executables(&mut repo, &[OLD, NEW]);
    repo
}

#[test]
fn the_first_run_counts_what_exists_as_already_here_and_a_program_after_it_is_raised_once() {
    // Switched on at the start of hour 2: both programs were seen before, so nothing is new.
    let clock = ManualClock::new(t(2, 0));
    let mut worker = DetectWorker::new(
        InMemoryRepository::default(),
        vec![Box::new(NewThingsDetector::new(data()))],
        clock,
        settings(),
    );

    let DetectTick::Ran(first) = worker.tick() else { panic!("ran") };

    assert_eq!((first.candidates, first.inserted), (0, 0));
    assert!(worker.repository().findings(None, 10).unwrap().is_empty());
}

#[test]
fn a_program_first_seen_after_the_switch_on_is_raised_and_found_again_it_is_a_hit() {
    // Switched on at the start of hour 1: OLD (hour 0) is already here, NEW (hour 1) is new.
    let clock = ManualClock::new(t(1, 0));
    let mut worker = DetectWorker::new(
        InMemoryRepository::default(),
        vec![Box::new(NewThingsDetector::new(data()))],
        clock.clone(),
        settings(),
    );

    let DetectTick::Ran(first) = worker.tick() else { panic!("ran") };
    assert_eq!((first.candidates, first.inserted), (1, 1));
    let found = worker.repository().findings(None, 10).unwrap();
    assert_eq!((found[0].kind.as_str(), found[0].subject.as_str()), ("new_program", NEW.to_lowercase().as_str()));
    assert!(found[0].why.contains("not been read") || found[0].evidence.iter().any(|l| l.contains("not been read")));

    clock.set(t(1, 20));
    let DetectTick::Ran(again) = worker.tick() else { panic!("ran again") };
    assert_eq!((again.inserted, again.touched), (0, 1));
    assert_eq!(worker.repository().findings(None, 10).unwrap()[0].hits, 2);
}
