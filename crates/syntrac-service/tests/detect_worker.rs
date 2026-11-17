//! The detect worker: runs the detectors on a timer with its own repository, and one failure never stops it.

use std::time::Duration;

use syntrac_core::findings::{Candidate, ReconcileSettings, Tier};
use syntrac_core::{DetectError, Detector, FindingRepository, Timestamp};
use syntrac_service::{DetectTick, DetectWorker, DetectWorkerSettings};
use syntrac_testkit::{InMemoryRepository, ManualClock};

fn at(secs: i64) -> Timestamp {
    Timestamp::from_micros(secs * 1_000_000)
}

fn settings() -> DetectWorkerSettings {
    DetectWorkerSettings { interval: Duration::from_secs(300), reconcile: ReconcileSettings { max_new: 50 } }
}

struct Finds(&'static str);
impl Detector for Finds {
    fn name(&self) -> &'static str {
        "finds"
    }
    fn detect(&self, _since: Timestamp, now: Timestamp) -> Result<Vec<Candidate>, DetectError> {
        Ok(vec![Candidate {
            kind: "new_program".into(),
            subject: self.0.into(),
            subject_label: self.0.into(),
            tier: Tier::Unusual,
            title: format!("New program {}", self.0),
            why: "Because.".into(),
            evidence: vec![],
            dedupe_key: format!("new_program:{}", self.0),
            at: now,
        }])
    }
}

struct Fails;
impl Detector for Fails {
    fn name(&self) -> &'static str {
        "fails"
    }
    fn detect(&self, _since: Timestamp, _now: Timestamp) -> Result<Vec<Candidate>, DetectError> {
        Err(DetectError("no data".into()))
    }
}

#[test]
fn the_first_tick_runs_and_the_next_waits_for_the_interval() {
    let clock = ManualClock::new(at(1_000));
    let mut worker =
        DetectWorker::new(InMemoryRepository::default(), vec![Box::new(Finds("a.exe"))], clock.clone(), settings());

    let first = worker.tick();

    let DetectTick::Ran(report) = first else { panic!("{first:?}") };
    assert_eq!(report.inserted, 1);
    assert_eq!(worker.tick(), DetectTick::NotYet);
    clock.set(at(1_299));
    assert_eq!(worker.tick(), DetectTick::NotYet, "still inside the five minutes");
    clock.set(at(1_301));
    let DetectTick::Ran(again) = worker.tick() else { panic!("ran again") };
    assert_eq!((again.inserted, again.touched), (0, 1), "the same thing found again is one more hit");
}

#[test]
fn a_failing_detector_is_in_the_report_and_the_others_still_run() {
    let mut worker = DetectWorker::new(
        InMemoryRepository::default(),
        vec![Box::new(Fails), Box::new(Finds("b.exe"))],
        ManualClock::new(at(0)),
        settings(),
    );

    let DetectTick::Ran(report) = worker.tick() else { panic!("ran") };

    assert_eq!(report.inserted, 1);
    assert_eq!(report.failed, vec![("fails".to_owned(), "no data".to_owned())]);
}

#[test]
fn with_no_detectors_it_still_switches_detection_on_and_stores_nothing() {
    let mut worker =
        DetectWorker::new(InMemoryRepository::default(), Vec::new(), ManualClock::new(at(500)), settings());

    let DetectTick::Ran(report) = worker.tick() else { panic!("ran") };

    assert_eq!((report.candidates, report.inserted), (0, 0));
    assert!(worker.repository().findings(None, 10).unwrap().is_empty());
}
