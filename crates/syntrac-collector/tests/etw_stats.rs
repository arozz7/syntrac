//! Diagnostics counters: they must be able to tell WHICH events failed to parse, and whether ETW timestamps
//! share the wall clock's time base (FILETIME) or not (QPC would look like a huge, constant offset).

use syntrac_collector::etw_stats::EtwStats;
use syntrac_core::Timestamp;

const SEC: i64 = 1_000_000;

fn at(secs: i64) -> Timestamp {
    Timestamp::from_micros(secs * SEC)
}

#[test]
fn parse_errors_are_bucketed_by_provider_and_event_id_and_totalled() {
    let stats = EtwStats::default();
    stats.record_parse_error("network", 17);
    stats.record_parse_error("network", 17);
    stats.record_parse_error("network", 11);
    stats.record_parse_error("process", 1);

    let report = stats.report();
    assert_eq!(report.parse_errors_total, 4);
    let count_for = |provider: &str, event_id: u16| {
        report.parse_errors.iter().find(|e| e.provider == provider && e.event_id == event_id).map_or(0, |e| e.count)
    };
    assert_eq!(count_for("network", 17), 2);
    assert_eq!(count_for("network", 11), 1);
    assert_eq!(count_for("process", 1), 1);
    assert_eq!(count_for("dns", 3008), 0);
}

#[test]
fn the_noisiest_failures_are_listed_first() {
    let stats = EtwStats::default();
    stats.record_parse_error("dns", 3008);
    for _ in 0..5 {
        stats.record_parse_error("network", 43);
    }
    let report = stats.report();
    assert_eq!((report.parse_errors[0].provider, report.parse_errors[0].event_id), ("network", 43));
}

#[test]
fn received_and_dropped_events_are_counted() {
    let stats = EtwStats::default();
    stats.record_received();
    stats.record_received();
    stats.record_dropped();
    let report = stats.report();
    assert_eq!((report.received, report.dropped), (2, 1));
}

#[test]
fn the_largest_delivery_lag_is_tracked() {
    let stats = EtwStats::default();
    stats.observe_event_time(at(98), at(100)); // 2 s behind the wall clock
    stats.observe_event_time(at(99), at(100)); // 1 s behind
    let report = stats.report();
    assert_eq!(report.max_lag_ms, 2000);
    assert_eq!(report.future_events, 0);
}

#[test]
fn events_stamped_well_into_the_future_are_counted_and_their_distance_recorded() {
    // A QPC-based timestamp interpreted as FILETIME would land far from "now"; this is how we would notice.
    let stats = EtwStats::default();
    stats.observe_event_time(at(105), at(100));
    stats.observe_event_time(at(100), at(100));
    let report = stats.report();
    assert_eq!(report.future_events, 1);
    assert_eq!(report.max_ahead_ms, 5000);
}

#[test]
fn small_clock_jitter_ahead_is_not_a_future_event() {
    let stats = EtwStats::default();
    stats.observe_event_time(Timestamp::from_micros(100 * SEC + 400_000), at(100)); // 0.4 s ahead
    assert_eq!(stats.report().future_events, 0);
}
