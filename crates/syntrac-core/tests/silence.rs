//! A telemetry source that is "running" but delivers nothing is the worst kind of failure, because every other
//! number still looks healthy. The watchdog turns "the count stopped moving" into a number of seconds and a decision.

use syntrac_core::silence::{SilenceAction, SilencePolicy, SilenceWatchdog};
use syntrac_core::Timestamp;

const SEC: i64 = 1_000_000;

fn at(secs: i64) -> Timestamp {
    Timestamp::from_micros(secs * SEC)
}

fn policy() -> SilencePolicy {
    SilencePolicy { restart_after_secs: 180, max_restarts: 2 }
}

#[test]
fn a_count_that_keeps_growing_is_never_silent() {
    let mut dog = SilenceWatchdog::new(policy(), at(0));
    for (i, secs) in (1..=1000).step_by(10).enumerate() {
        let verdict = dog.observe(100 * (i as u64 + 1), at(secs));
        assert_eq!(verdict.silent_secs, 0);
        assert_eq!(verdict.action, SilenceAction::None);
    }
}

#[test]
fn silence_is_measured_from_the_last_time_the_count_moved() {
    let mut dog = SilenceWatchdog::new(policy(), at(0));
    dog.observe(50, at(10));
    assert_eq!(dog.observe(50, at(40)).silent_secs, 30);
    assert_eq!(dog.observe(50, at(100)).silent_secs, 90);
}

#[test]
fn a_source_that_never_delivers_is_silent_from_the_moment_the_watchdog_started() {
    // This is today's failure: the trace "started" and then received nothing, ever.
    let mut dog = SilenceWatchdog::new(policy(), at(1000));
    assert_eq!(dog.observe(0, at(1100)).silent_secs, 100);
}

#[test]
fn a_restart_is_requested_once_the_silence_reaches_the_threshold_and_not_before() {
    let mut dog = SilenceWatchdog::new(policy(), at(0));
    assert_eq!(dog.observe(0, at(179)).action, SilenceAction::None);
    assert_eq!(dog.observe(0, at(180)).action, SilenceAction::Restart { attempt: 1 });
}

#[test]
fn after_a_restart_the_source_gets_a_full_threshold_to_prove_itself_before_the_next_one() {
    let mut dog = SilenceWatchdog::new(policy(), at(0));
    assert_eq!(dog.observe(0, at(180)).action, SilenceAction::Restart { attempt: 1 });
    let after = dog.observe(0, at(181));
    assert_eq!(after.action, SilenceAction::None);
    assert_eq!(after.silent_secs, 181, "a restart must not make the reported silence look shorter than it is");
    assert_eq!(dog.observe(0, at(359)).action, SilenceAction::None);
    assert_eq!(dog.observe(0, at(360)).action, SilenceAction::Restart { attempt: 2 });
}

#[test]
fn restarts_are_limited_and_giving_up_is_announced_exactly_once() {
    let mut dog = SilenceWatchdog::new(policy(), at(0));
    assert_eq!(dog.observe(0, at(180)).action, SilenceAction::Restart { attempt: 1 });
    assert_eq!(dog.observe(0, at(360)).action, SilenceAction::Restart { attempt: 2 });
    assert_eq!(dog.observe(0, at(540)).action, SilenceAction::GiveUp);
    assert_eq!(dog.observe(0, at(900)).action, SilenceAction::None, "no repeated announcements or restarts");
    assert_eq!(dog.observe(0, at(900)).silent_secs, 900, "but the silence keeps being reported honestly");
}

#[test]
fn a_restart_that_works_resets_the_attempt_budget() {
    let mut dog = SilenceWatchdog::new(policy(), at(0));
    assert_eq!(dog.observe(0, at(180)).action, SilenceAction::Restart { attempt: 1 });
    // events flow again
    assert_eq!(dog.observe(10, at(200)).silent_secs, 0);
    // a much later, separate stall is treated as a new problem with a fresh budget
    assert_eq!(dog.observe(10, at(380)).action, SilenceAction::Restart { attempt: 1 });
}

#[test]
fn a_counter_that_goes_backwards_still_counts_as_activity() {
    // A restarted trace may report from zero again; that is a change, not silence.
    let mut dog = SilenceWatchdog::new(policy(), at(0));
    dog.observe(500, at(10));
    assert_eq!(dog.observe(3, at(500)).silent_secs, 0);
}

#[test]
fn a_clock_that_moves_backwards_never_gives_negative_silence_or_a_restart() {
    let mut dog = SilenceWatchdog::new(policy(), at(1000));
    let verdict = dog.observe(0, at(900));
    assert_eq!(verdict.silent_secs, 0);
    assert_eq!(verdict.action, SilenceAction::None);
}
