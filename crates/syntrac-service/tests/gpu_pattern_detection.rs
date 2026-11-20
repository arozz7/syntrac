//! The GPU pattern as the detector runs it (docs/ALERTS-PLAN.md, slice G2): samples with GPU use go through the rollups and the hourly rows
//! into the pattern detector, which raises an unsigned program in a user folder that kept the GPU busy for three hours in a row.

use std::time::Duration;

use syntrac_core::{
    built_in_pack, roll_up_due, Detector, Executable, FileFacts, IdentityRecord, IdentityRepository, MetricsRepository,
    PatternDetector, ProcessRecord, ProcessRepository, RollupSettings, Signature, SignatureStatus, Timestamp,
};
use syntrac_testkit::fixtures::{at_secs, gpu_process_metric, metrics_sample, running_process};
use syntrac_testkit::InMemoryRepository;

const MINER: &str = r"C:\Users\alice\AppData\Roaming\toolx\toolx.exe";
const KIND: &str = "pattern_unsigned_program_keeps_gpu_busy";
/// An exact hour (2026-11-13 12:00 UTC).
const BASE: i64 = 1_794_571_200 / 3600 * 3600;

fn secs(hour: i64, minutes: i64) -> i64 {
    BASE + hour * 3600 + minutes * 60
}

fn t(hour: i64, minutes: i64) -> Timestamp {
    at_secs(secs(hour, minutes))
}

fn start_process(repo: &mut InMemoryRepository, pid: u32, started: Timestamp, path: &str) -> ProcessRecord {
    let mut record = running_process(pid, started);
    record.image_path = path.to_owned();
    repo.record_start(record.clone()).unwrap();
    record
}

/// The program runs in the given hours at the given GPU share; its file was read and is unsigned. Everything before hour 5 is rolled up.
fn world(busy_hours: &[(i64, f32)]) -> InMemoryRepository {
    let mut repo = InMemoryRepository::default();
    let process = start_process(&mut repo, 10, t(0, 0), MINER);
    for (hour, gpu) in busy_hours {
        repo.append_metrics(&metrics_sample(
            secs(*hour, 10),
            10.0,
            vec![gpu_process_metric(10, Some(process.id), 1.0, *gpu, 0)],
            0.0,
            0,
        ))
        .unwrap();
    }
    let settings = RollupSettings { settle: Duration::from_secs(300), max_hours_per_pass: 500 };
    roll_up_due(&mut repo, t(5, 0), &settings).unwrap();
    let exe = Executable::from_path(MINER).unwrap();
    repo.save_identity(&IdentityRecord {
        exe,
        facts: Some(FileFacts {
            signature: Signature { status: SignatureStatus::Unsigned, ..Signature::default() },
            ..FileFacts::default()
        }),
        inspected_at: Some(t(4, 0)),
        last_attempt: t(4, 0),
        failure: None,
    })
    .unwrap();
    repo
}

fn found(repo: InMemoryRepository) -> Vec<String> {
    let detector = PatternDetector::new(repo, built_in_pack());
    // Detection was switched on at the start; "now" is shortly after hour 4 began, so hour 3 is the last completed hour with rows.
    detector.detect(t(0, 0), t(4, 10)).unwrap().into_iter().map(|candidate| candidate.kind).collect()
}

#[test]
fn three_busy_hours_in_a_row_raise_the_pattern() {
    let kinds = found(world(&[(1, 60.0), (2, 55.0), (3, 62.0)]));
    assert_eq!(kinds.iter().filter(|kind| kind.as_str() == KIND).count(), 1, "{kinds:?}");
}

#[test]
fn two_busy_hours_or_a_quiet_one_in_the_middle_raise_nothing() {
    assert!(!found(world(&[(2, 55.0), (3, 62.0)])).contains(&KIND.to_owned()));
    assert!(!found(world(&[(1, 60.0), (2, 4.0), (3, 62.0)])).contains(&KIND.to_owned()));
}
