//! The status board: what the collector publishes each pass, and what the pipe answers.

use syntrac_collector::etw_stats::{EtwReport, ParseErrorCount};
use syntrac_core::Timestamp;
use syntrac_ipc::protocol::{Request, Response};
use syntrac_ipc::serve::Handler;
use syntrac_service::{BoardInfo, StatusBoard, Totals};
use syntrac_testkit::ManualClock;

const SEC: i64 = 1_000_000;

fn board(clock: &ManualClock, etw_enabled: bool) -> StatusBoard<ManualClock> {
    StatusBoard::new(
        BoardInfo {
            version: "9.9.9".into(),
            pid: 4242,
            started_at: Timestamp::from_micros(100 * SEC),
            database_path: r"C:\data\syntrac.db".into(),
            etw_enabled,
        },
        clock.clone(),
    )
}

fn status(board: &StatusBoard<ManualClock>) -> syntrac_ipc::protocol::StatusReport {
    match board.handle(Request::Status) {
        Response::Status(report) => report,
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn a_ping_reports_the_version() {
    let clock = ManualClock::new(Timestamp::from_micros(100 * SEC));
    assert_eq!(board(&clock, false).handle(Request::Ping), Response::Pong { version: "9.9.9".into() });
}

#[test]
fn before_the_first_pass_the_board_reports_zeros_and_no_last_tick() {
    let clock = ManualClock::new(Timestamp::from_micros(100 * SEC));
    let report = status(&board(&clock, false));
    assert_eq!((report.pid, report.totals.ticks, report.last_tick_at_micros), (4242, 0, None));
    assert_eq!(report.database_path, r"C:\data\syntrac.db");
    assert!(report.etw.is_none());
}

#[test]
fn published_totals_and_etw_health_appear_in_the_status() {
    let clock = ManualClock::new(Timestamp::from_micros(100 * SEC));
    let board = board(&clock, true);
    let totals = Totals { ticks: 10, events_stored: 55, flows_stored: 7, metrics_samples: 2, ..Totals::default() };
    let etw = EtwReport {
        received: 900,
        dropped: 3,
        parse_errors: vec![ParseErrorCount { provider: "network", event_id: 11, count: 2 }],
        parse_errors_total: 2,
        max_lag_ms: 1014,
        max_ahead_ms: 0,
        future_events: 0,
    };
    board.publish(totals, Some(&etw), 240, Timestamp::from_micros(130 * SEC));

    let report = status(&board);
    assert_eq!((report.totals.ticks, report.totals.events_stored, report.totals.flows_stored), (10, 55, 7));
    assert_eq!(report.last_tick_at_micros, Some(130 * SEC));
    let etw = report.etw.expect("etw health is published when ETW is on");
    assert_eq!((etw.received, etw.dropped, etw.parse_errors, etw.max_lag_ms), (900, 3, 2, 1014));
    assert_eq!(etw.silent_secs, 240);
    assert!(report.etw_enabled);
}

#[test]
fn uptime_is_measured_from_the_start_time_at_the_moment_of_the_request() {
    let clock = ManualClock::new(Timestamp::from_micros(100 * SEC));
    let board = board(&clock, false);
    assert_eq!(status(&board).uptime_secs, 0);
    clock.set(Timestamp::from_micros(161 * SEC));
    assert_eq!(status(&board).uptime_secs, 61);
}

#[test]
fn a_clock_that_moves_backwards_never_gives_a_negative_uptime() {
    let clock = ManualClock::new(Timestamp::from_micros(50 * SEC)); // before the start time
    assert_eq!(status(&board(&clock, false)).uptime_secs, 0);
}

#[test]
fn the_text_rendering_makes_dropped_events_impossible_to_miss() {
    let clock = ManualClock::new(Timestamp::from_micros(100 * SEC));
    let board = board(&clock, true);
    let etw = EtwReport {
        received: 10,
        dropped: 4,
        parse_errors: vec![],
        parse_errors_total: 0,
        max_lag_ms: 5,
        max_ahead_ms: 0,
        future_events: 0,
    };
    board.publish(Totals::default(), Some(&etw), 0, Timestamp::from_micros(101 * SEC));
    let text = syntrac_service::format_status(&status(&board));
    assert!(text.contains("4 DROPPED"), "{text}");
    assert!(text.contains("9.9.9") && text.contains("4242"), "{text}");
}

#[test]
fn the_text_rendering_says_when_etw_is_not_running() {
    let clock = ManualClock::new(Timestamp::from_micros(100 * SEC));
    let text = syntrac_service::format_status(&status(&board(&clock, false)));
    assert!(text.contains("not running"), "{text}");
}

#[test]
fn the_text_rendering_says_when_etw_has_gone_quiet_and_stays_calm_when_it_has_not() {
    let clock = ManualClock::new(Timestamp::from_micros(100 * SEC));
    let board = board(&clock, true);
    let etw = EtwReport {
        received: 0,
        dropped: 0,
        parse_errors: vec![],
        parse_errors_total: 0,
        max_lag_ms: 0,
        max_ahead_ms: 0,
        future_events: 0,
    };

    board.publish(Totals::default(), Some(&etw), 12, Timestamp::from_micros(101 * SEC));
    let calm = syntrac_service::format_status(&status(&board));
    assert!(!calm.to_uppercase().contains("SILENT"), "a few quiet seconds are normal: {calm}");

    board.publish(Totals::default(), Some(&etw), 600, Timestamp::from_micros(102 * SEC));
    let quiet = syntrac_service::format_status(&status(&board));
    assert!(quiet.contains("SILENT for 600 s"), "{quiet}");
}
