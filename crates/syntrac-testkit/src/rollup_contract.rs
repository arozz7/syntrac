//! Contract suite for rollups: every `RollupRepository` (in-memory, SQLite, ...) must pass these, so the two cannot drift apart.

use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use syntrac_core::rollup::{roll_up_hour, HourInput};
use syntrac_core::{
    roll_up_due, EventRepository, MetricsRepository, ProcessInstanceId, ProcessRecord, ProcessRepository, Retention,
    RollupRepository, RollupSettings, Timestamp,
};

use crate::fixtures::{
    metrics_sample, net_flow, process_metric, process_started, running_process, EventFactory, BEACON_TARGET, SEC,
};

const T0: i64 = 1_794_571_200 / 3600 * 3600; // an exact UTC hour, in seconds
pub(crate) const A_EXE: &str = r"C:\Apps\A.exe";
pub(crate) const B_EXE: &str = r"C:\Apps\B.exe";
pub(crate) const OTHER_TARGET: Ipv4Addr = Ipv4Addr::new(198, 51, 100, 7);

/// Hour `hour` (0 = the base hour) plus `minutes`, as a timestamp.
pub(crate) fn t(hour: i64, minutes: i64) -> Timestamp {
    Timestamp::from_micros((T0 + hour * 3600 + minutes * 60) * SEC)
}

/// The same instant in seconds, for the fixtures that take seconds.
pub(crate) fn secs(hour: i64, minutes: i64) -> i64 {
    T0 + hour * 3600 + minutes * 60
}

pub(crate) fn settings(max_hours: usize) -> RollupSettings {
    RollupSettings { settle: Duration::from_secs(5 * 60), max_hours_per_pass: max_hours }
}

pub trait RollupSubject:
    RollupRepository + EventRepository + ProcessRepository + MetricsRepository + Retention
{
}
impl<T> RollupSubject for T where
    T: RollupRepository + EventRepository + ProcessRepository + MetricsRepository + Retention
{
}

pub fn rollup_contract<R: RollupSubject>(make: impl Fn() -> R) {
    nothing_is_due_inside_the_first_hour(make());
    only_completed_hours_are_rolled_and_the_watermark_follows(make());
    an_hour_is_not_rolled_until_it_has_settled(make());
    rolling_again_changes_nothing(make());
    a_backlog_is_worked_off_in_capped_passes(make());
    hours_without_data_store_nothing_but_the_watermark_passes_them(make());
    the_first_pass_starts_at_the_earliest_detail(make());
    processes_are_attributed_by_instance_not_pid(make());
    starts_come_from_process_started_events(make());
    traffic_is_rolled_by_executable_and_destination(make());
    ledgers_record_first_and_last_seen_and_survive_a_purge(make());
    a_rolled_up_hour_cannot_be_rolled_again(make());
    earliest_detail_covers_every_kind_of_detail(make());
    history_ranges_are_inclusive_and_ordered(make());
}

pub(crate) fn sample_at<R: MetricsRepository>(repo: &mut R, hour: i64, minutes: i64, cpu: f32) {
    repo.append_metrics(&metrics_sample(secs(hour, minutes), cpu, vec![], 0.0, 0)).unwrap();
}

pub(crate) fn start_process<R: ProcessRepository>(
    repo: &mut R,
    pid: u32,
    started: Timestamp,
    exe: &str,
) -> ProcessRecord {
    let mut record = running_process(pid, started);
    record.image_path = exe.to_owned();
    repo.record_start(record.clone()).unwrap();
    record
}

fn nothing_is_due_inside_the_first_hour<R: RollupSubject>(mut repo: R) {
    sample_at(&mut repo, 0, 10, 5.0);
    let pass = roll_up_due(&mut repo, t(0, 40), &settings(4)).unwrap();
    assert_eq!((pass.hours_rolled, pass.backlog), (0, false));
    assert_eq!(repo.rolled_up_before().unwrap(), None, "no rollup yet: the watermark is not invented");
}

fn only_completed_hours_are_rolled_and_the_watermark_follows<R: RollupSubject>(mut repo: R) {
    for hour in 0..3 {
        sample_at(&mut repo, hour, 10, 10.0 * (hour + 1) as f32);
    }
    let pass = roll_up_due(&mut repo, t(2, 30), &settings(10)).unwrap();
    assert_eq!(pass.hours_rolled, 2, "hours 0 and 1 are complete; hour 2 is still running");
    assert_eq!(repo.rolled_up_before().unwrap(), Some(t(2, 0)));

    let hours = repo.system_hours(t(0, 0), t(5, 0)).unwrap();
    assert_eq!(hours.iter().map(|h| h.hour).collect::<Vec<_>>(), vec![t(0, 0), t(1, 0)]);
    assert!((hours[0].cpu_avg - 10.0).abs() < 1e-6 && (hours[1].cpu_avg - 20.0).abs() < 1e-6);
}

fn an_hour_is_not_rolled_until_it_has_settled<R: RollupSubject>(mut repo: R) {
    sample_at(&mut repo, 0, 10, 5.0);
    assert_eq!(
        roll_up_due(&mut repo, t(1, 2), &settings(4)).unwrap().hours_rolled,
        0,
        "ended 2 minutes ago: late events may still come"
    );
    assert_eq!(roll_up_due(&mut repo, t(1, 6), &settings(4)).unwrap().hours_rolled, 1);
}

fn rolling_again_changes_nothing<R: RollupSubject>(mut repo: R) {
    sample_at(&mut repo, 0, 10, 5.0);
    roll_up_due(&mut repo, t(2, 0), &settings(4)).unwrap();
    let before = repo.system_hours(t(0, 0), t(9, 0)).unwrap();

    let again = roll_up_due(&mut repo, t(2, 0), &settings(4)).unwrap();

    assert_eq!(again.hours_rolled, 0, "nothing new is complete: a second pass has nothing to do");
    assert_eq!(repo.system_hours(t(0, 0), t(9, 0)).unwrap(), before);
}

fn a_backlog_is_worked_off_in_capped_passes<R: RollupSubject>(mut repo: R) {
    for hour in 0..10 {
        sample_at(&mut repo, hour, 1, 1.0);
    }
    let now = t(10, 30);
    let first = roll_up_due(&mut repo, now, &settings(4)).unwrap();
    let second = roll_up_due(&mut repo, now, &settings(4)).unwrap();
    let third = roll_up_due(&mut repo, now, &settings(4)).unwrap();
    assert_eq!((first.hours_rolled, first.backlog), (4, true));
    assert_eq!((second.hours_rolled, second.backlog), (4, true));
    assert_eq!((third.hours_rolled, third.backlog), (2, false));
    assert_eq!(repo.system_hours(t(0, 0), t(20, 0)).unwrap().len(), 10);
    assert_eq!(roll_up_due(&mut repo, now, &settings(4)).unwrap().hours_rolled, 0);
}

fn hours_without_data_store_nothing_but_the_watermark_passes_them<R: RollupSubject>(mut repo: R) {
    sample_at(&mut repo, 0, 1, 1.0);
    sample_at(&mut repo, 5, 1, 1.0);
    let pass = roll_up_due(&mut repo, t(7, 0), &settings(20)).unwrap();
    assert_eq!(pass.hours_rolled, 6, "hours 0 to 5");
    assert_eq!(repo.rolled_up_before().unwrap(), Some(t(6, 0)));
    let rows = repo.system_hours(t(0, 0), t(20, 0)).unwrap();
    assert_eq!(rows.iter().map(|h| h.hour).collect::<Vec<_>>(), vec![t(0, 0), t(5, 0)], "the gap has no rows");
}

fn the_first_pass_starts_at_the_earliest_detail<R: RollupSubject>(mut repo: R) {
    sample_at(&mut repo, 7, 10, 3.0);
    let pass = roll_up_due(&mut repo, t(9, 0), &settings(50)).unwrap();
    assert_eq!(pass.hours_rolled, 1, "hour 7 only; it does not walk back from the beginning of time");
    assert_eq!(repo.system_hours(t(0, 0), t(20, 0)).unwrap().len(), 1);
}

fn processes_are_attributed_by_instance_not_pid<R: RollupSubject>(mut repo: R) {
    // PID 10 belongs to A.exe, exits, and is then reused by B.exe.
    let a = start_process(&mut repo, 10, t(0, 0), A_EXE);
    repo.record_exit(a.id, t(0, 30), Some(0)).unwrap();
    let b = start_process(&mut repo, 10, t(0, 31), B_EXE);

    repo.append_metrics(&metrics_sample(secs(0, 10), 10.0, vec![process_metric(10, Some(a.id), 4.0)], 0.0, 0)).unwrap();
    repo.append_metrics(&metrics_sample(secs(0, 40), 10.0, vec![process_metric(10, Some(b.id), 6.0)], 0.0, 0)).unwrap();
    repo.append_metrics(&metrics_sample(secs(0, 50), 10.0, vec![process_metric(99, None, 1.0)], 0.0, 0)).unwrap();
    roll_up_due(&mut repo, t(2, 0), &settings(4)).unwrap();

    let a_rows = repo.executable_hours(r"c:\APPS\a.EXE", t(0, 0), t(0, 0)).unwrap();
    let b_rows = repo.executable_hours(B_EXE, t(0, 0), t(0, 0)).unwrap();
    assert_eq!(a_rows.len(), 1, "the path is matched case-insensitively");
    assert!(
        (a_rows[0].cpu_peak - 4.0).abs() < 1e-6 && (b_rows[0].cpu_peak - 6.0).abs() < 1e-6,
        "each program keeps its own samples"
    );
    assert!(
        (a_rows[0].cpu_avg - 4.0 / 3.0).abs() < 1e-6,
        "averaged over all 3 samples of the hour: {}",
        a_rows[0].cpu_avg
    );
    assert_eq!(a_rows[0].samples_present, 1);
}

fn starts_come_from_process_started_events<R: RollupSubject>(mut repo: R) {
    // A has a process record and a sample but NO process_started event; B has the event too.
    let a = start_process(&mut repo, 10, t(0, 1), A_EXE);
    let b = start_process(&mut repo, 11, t(0, 2), B_EXE);
    let mut f = EventFactory::default();
    repo.append(&[f.make(t(0, 2), process_started(11, 4, secs(0, 2), B_EXE))]).unwrap();
    repo.append_metrics(&metrics_sample(
        secs(0, 10),
        5.0,
        vec![process_metric(10, Some(a.id), 1.0), process_metric(11, Some(b.id), 1.0)],
        0.0,
        0,
    ))
    .unwrap();
    roll_up_due(&mut repo, t(2, 0), &settings(4)).unwrap();

    assert_eq!(repo.executable_hours(A_EXE, t(0, 0), t(0, 0)).unwrap()[0].starts, 0);
    assert_eq!(repo.executable_hours(B_EXE, t(0, 0), t(0, 0)).unwrap()[0].starts, 1);
}

pub(crate) fn flow_event(
    f: &mut EventFactory,
    owner: Option<ProcessInstanceId>,
    target: Ipv4Addr,
    sent: u64,
    received: u64,
    at: i64,
) -> syntrac_core::EventEnvelope {
    let mut event = f.make(Timestamp::from_micros(at * SEC), net_flow(10, target, sent, received, at));
    event.process_instance_id = owner;
    event
}

fn traffic_is_rolled_by_executable_and_destination<R: RollupSubject>(mut repo: R) {
    let a = start_process(&mut repo, 10, t(0, 0), A_EXE);
    let stranger = ProcessInstanceId::from_pid_and_creation(777, t(0, 0)); // no process record
    let mut f = EventFactory::default();
    repo.append(&[
        flow_event(&mut f, Some(a.id), BEACON_TARGET, 100, 200, secs(0, 10)),
        flow_event(&mut f, Some(a.id), BEACON_TARGET, 50, 50, secs(0, 20)),
        flow_event(&mut f, Some(a.id), OTHER_TARGET, 1, 1, secs(0, 30)),
        flow_event(&mut f, Some(stranger), BEACON_TARGET, 7, 0, secs(0, 40)),
        flow_event(&mut f, None, BEACON_TARGET, 3, 0, secs(0, 45)),
    ])
    .unwrap();
    sample_at(&mut repo, 0, 5, 1.0);
    roll_up_due(&mut repo, t(2, 0), &settings(4)).unwrap();

    let rows = repo.destination_hours(IpAddr::V4(BEACON_TARGET), t(0, 0), t(0, 0)).unwrap();
    assert_eq!(rows.len(), 2, "A.exe, and the unattributed flows together");
    assert!(rows[0].exe.is_none(), "unattributed sorts first");
    assert_eq!((rows[0].bytes_sent, rows[0].flows), (10, 2), "an owner with no process record is unattributed");
    let a_row = &rows[1];
    assert_eq!(a_row.exe.as_ref().unwrap().path, A_EXE);
    assert_eq!((a_row.bytes_sent, a_row.bytes_received, a_row.flows), (150, 250, 2));
    assert_eq!((a_row.first_seen, a_row.last_seen), (t(0, 10), t(0, 20)));
    assert_eq!((a_row.distinct_ports, a_row.top_port), (1, 443));
    assert_eq!(repo.destination_hours(IpAddr::V4(OTHER_TARGET), t(0, 0), t(5, 0)).unwrap().len(), 1);
}

fn ledgers_record_first_and_last_seen_and_survive_a_purge<R: RollupSubject>(mut repo: R) {
    let a = start_process(&mut repo, 10, t(0, 0), A_EXE);
    let mut f = EventFactory::default();
    repo.append(&[
        flow_event(&mut f, Some(a.id), BEACON_TARGET, 1, 1, secs(0, 10)),
        flow_event(&mut f, Some(a.id), BEACON_TARGET, 1, 1, secs(2, 20)),
    ])
    .unwrap();
    repo.append_metrics(&metrics_sample(secs(0, 5), 1.0, vec![process_metric(10, Some(a.id), 2.0)], 0.0, 0)).unwrap();
    repo.append_metrics(&metrics_sample(secs(2, 25), 1.0, vec![process_metric(10, Some(a.id), 2.0)], 0.0, 0)).unwrap();
    roll_up_due(&mut repo, t(4, 0), &settings(10)).unwrap();

    let target = IpAddr::V4(BEACON_TARGET);
    let exe = repo.executable_ledger(r"c:\apps\A.EXE").unwrap().unwrap();
    assert_eq!((exe.first_seen, exe.last_seen), (t(0, 5), t(2, 25)));
    let destination = repo.destination_ledger(target).unwrap().unwrap();
    assert_eq!((destination.first_seen, destination.last_seen), (t(0, 10), t(2, 20)));
    assert_eq!(repo.pair_ledger(A_EXE, target).unwrap(), Some(destination));
    assert_eq!(repo.executable_ledger(B_EXE).unwrap(), None, "never seen");
    assert_eq!(repo.pair_ledger(B_EXE, target).unwrap(), None);

    // The detail ages out; the history and the ledgers must not.
    let hours_before = repo.system_hours(t(0, 0), t(9, 0)).unwrap();
    repo.purge_before(t(100, 0)).unwrap();
    assert_eq!(repo.system_hours(t(0, 0), t(9, 0)).unwrap(), hours_before, "purging detail leaves the rollups alone");
    assert_eq!(repo.executable_ledger(A_EXE).unwrap(), Some(exe));
    assert_eq!(repo.destination_ledger(target).unwrap(), Some(destination));
    assert_eq!(repo.destination_hours(target, t(0, 0), t(9, 0)).unwrap().len(), 2);
}

fn a_rolled_up_hour_cannot_be_rolled_again<R: RollupSubject>(mut repo: R) {
    sample_at(&mut repo, 0, 10, 50.0);
    roll_up_due(&mut repo, t(2, 0), &settings(4)).unwrap();
    let before = repo.system_hours(t(0, 0), t(9, 0)).unwrap();
    let watermark = repo.rolled_up_before().unwrap();

    // A re-roll of hour 0 (for example after its detail was purged) would replace good rows with thinner ones: refused.
    let thinner = roll_up_hour(&HourInput::empty(t(0, 0)));
    assert!(repo.store_hour(&thinner).is_err());

    assert_eq!(repo.system_hours(t(0, 0), t(9, 0)).unwrap(), before);
    assert_eq!(repo.rolled_up_before().unwrap(), watermark, "and the watermark never moves backwards");
}

fn earliest_detail_covers_every_kind_of_detail<R: RollupSubject>(mut repo: R) {
    assert_eq!(repo.earliest_detail().unwrap(), None);
    let mut f = EventFactory::default();
    repo.append(&[f.make(t(3, 30), process_started(10, 4, secs(3, 30), A_EXE))]).unwrap();
    assert_eq!(repo.earliest_detail().unwrap(), Some(t(3, 30)));
    sample_at(&mut repo, 2, 15, 1.0);
    assert_eq!(repo.earliest_detail().unwrap(), Some(t(2, 15)));
}

fn history_ranges_are_inclusive_and_ordered<R: RollupSubject>(mut repo: R) {
    let a = start_process(&mut repo, 10, t(0, 0), A_EXE);
    for hour in 0..4 {
        repo.append_metrics(&metrics_sample(secs(hour, 10), 1.0, vec![process_metric(10, Some(a.id), 1.0)], 0.0, 0))
            .unwrap();
    }
    roll_up_due(&mut repo, t(5, 0), &settings(10)).unwrap();

    let system: Vec<_> = repo.system_hours(t(1, 0), t(2, 0)).unwrap().iter().map(|h| h.hour).collect();
    assert_eq!(system, vec![t(1, 0), t(2, 0)], "both ends are included");
    let exe: Vec<_> = repo.executable_hours(A_EXE, t(1, 0), t(3, 0)).unwrap().iter().map(|h| h.hour).collect();
    assert_eq!(exe, vec![t(1, 0), t(2, 0), t(3, 0)]);
    assert!(repo.executable_hours(A_EXE, t(10, 0), t(11, 0)).unwrap().is_empty());
}
