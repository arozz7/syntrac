//! The banner has to tell apart: running, running-but-silent, not running, and "cannot tell".

use syntrac_app_lib::health::{assess_health, HealthState, ETW_SILENT_DEGRADED_AFTER_SECS};
use syntrac_core::Timestamp;
use syntrac_ipc::pipe::ClientError;
use syntrac_ipc::protocol::{CollectorTotals, EtwStatus, StatusReport};

const SEC: i64 = 1_000_000;

fn now() -> Timestamp {
    Timestamp::from_micros(10_000 * SEC)
}

fn report(dropped: u64) -> StatusReport {
    report_silent(dropped, 0)
}

fn report_silent(dropped: u64, silent_secs: u64) -> StatusReport {
    StatusReport {
        version: "0.1.0".into(),
        pid: 99,
        started_at_micros: 0,
        uptime_secs: 3600,
        database_path: "db".into(),
        etw_enabled: true,
        last_tick_at_micros: Some(10_000 * SEC),
        totals: CollectorTotals::default(),
        etw: Some(EtwStatus { received: 100, dropped, parse_errors: 0, max_lag_ms: 5, future_events: 0, silent_secs }),
    }
}

fn data_age(secs: i64) -> Option<Timestamp> {
    Some(Timestamp::from_micros(10_000 * SEC - secs * SEC))
}

#[test]
fn a_collector_that_answers_with_fresh_data_is_running() {
    let health = assess_health(Ok(report(0)), data_age(3), now());
    assert_eq!(health.state, HealthState::Running);
    assert_eq!((health.pid, health.uptime_secs, health.data_age_secs), (Some(99), Some(3600), Some(3)));
    assert_eq!(health.etw_dropped, Some(0));
}

#[test]
fn a_collector_that_answers_but_whose_data_is_old_is_stale_not_healthy() {
    let health = assess_health(Ok(report(0)), data_age(300), now());
    assert_eq!(health.state, HealthState::Stale);
    assert!(health.message.contains("300"), "{}", health.message);
}

#[test]
fn dropped_etw_events_are_called_out_even_when_everything_else_is_fine() {
    let health = assess_health(Ok(report(12)), data_age(2), now());
    assert_eq!(health.state, HealthState::Running);
    assert!(health.message.contains("12") && health.message.to_lowercase().contains("dropped"), "{}", health.message);
}

#[test]
fn no_pipe_means_the_collector_is_offline_and_says_how_old_the_data_is() {
    let health = assess_health(Err(ClientError::NotRunning), data_age(90), now());
    assert_eq!(health.state, HealthState::Offline);
    assert_eq!(health.data_age_secs, Some(90));
}

#[test]
fn offline_with_no_data_at_all_says_so() {
    let health = assess_health(Err(ClientError::NotRunning), None, now());
    assert_eq!(health.state, HealthState::Offline);
    assert_eq!(health.data_age_secs, None);
}

#[test]
fn access_denied_and_other_pipe_problems_are_unknown_not_offline() {
    for error in [ClientError::AccessDenied, ClientError::Timeout, ClientError::Io("boom".into())] {
        let health = assess_health(Err(error), data_age(1), now());
        assert_eq!(health.state, HealthState::Unknown);
    }
}

#[test]
fn a_future_dated_sample_never_gives_a_negative_age() {
    let health = assess_health(Ok(report(0)), Some(Timestamp::from_micros(10_100 * SEC)), now());
    assert_eq!(health.data_age_secs, Some(0));
}

// ---- a trace that is "started" but delivers nothing

#[test]
fn a_few_quiet_seconds_from_etw_are_normal_and_do_not_change_the_state() {
    let health = assess_health(Ok(report_silent(0, 15)), data_age(2), now());
    assert_eq!(health.state, HealthState::Running);
}

#[test]
fn etw_that_has_delivered_nothing_for_a_long_time_is_degraded_even_though_the_collector_looks_alive() {
    let silent = ETW_SILENT_DEGRADED_AFTER_SECS + 60;
    let health = assess_health(Ok(report_silent(0, silent)), data_age(2), now());
    assert_eq!(health.state, HealthState::Degraded);
    assert_eq!(health.etw_silent_secs, Some(silent));
    let message = health.message.to_lowercase();
    assert!(message.contains("etw") && message.contains(&silent.to_string()), "{}", health.message);
    assert!(message.contains("not being recorded"), "say what is lost: {}", health.message);
}

#[test]
fn the_degraded_threshold_is_exact() {
    let just_under = assess_health(Ok(report_silent(0, ETW_SILENT_DEGRADED_AFTER_SECS - 1)), data_age(2), now());
    let at = assess_health(Ok(report_silent(0, ETW_SILENT_DEGRADED_AFTER_SECS)), data_age(2), now());
    assert_eq!((just_under.state, at.state), (HealthState::Running, HealthState::Degraded));
}

#[test]
fn old_data_is_still_reported_as_stale_before_a_silent_trace() {
    // Nothing is being written at all: that is the bigger problem and the banner must say so.
    let health = assess_health(Ok(report_silent(0, 1000)), data_age(300), now());
    assert_eq!(health.state, HealthState::Stale);
}

#[test]
fn a_silent_trace_outranks_dropped_events() {
    let health = assess_health(Ok(report_silent(40, 1000)), data_age(2), now());
    assert_eq!(health.state, HealthState::Degraded);
}

#[test]
fn a_collector_running_without_etw_is_not_called_degraded() {
    let mut no_etw = report(0);
    no_etw.etw = None;
    no_etw.etw_enabled = false;
    assert_eq!(assess_health(Ok(no_etw), data_age(2), now()).state, HealthState::Running);
}
