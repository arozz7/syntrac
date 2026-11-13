//! Process identity must survive PID reuse: the id is derived from (pid, creation time).

use syntrac_core::{ProcessInstanceId, Timestamp};

#[test]
fn same_pid_and_creation_time_give_the_same_id() {
    let a = ProcessInstanceId::from_pid_and_creation(4242, Timestamp::from_micros(1_000));
    let b = ProcessInstanceId::from_pid_and_creation(4242, Timestamp::from_micros(1_000));
    assert_eq!(a, b);
}

#[test]
fn reused_pid_with_a_new_creation_time_gives_a_different_id() {
    let first = ProcessInstanceId::from_pid_and_creation(4242, Timestamp::from_micros(1_000));
    let second = ProcessInstanceId::from_pid_and_creation(4242, Timestamp::from_micros(9_000));
    assert_ne!(first, second);
}

#[test]
fn different_pids_started_at_the_same_time_give_different_ids() {
    let a = ProcessInstanceId::from_pid_and_creation(100, Timestamp::from_micros(1_000));
    let b = ProcessInstanceId::from_pid_and_creation(101, Timestamp::from_micros(1_000));
    assert_ne!(a, b);
}

#[test]
fn id_round_trips_through_json_as_a_plain_string() {
    let id = ProcessInstanceId::from_pid_and_creation(7, Timestamp::from_micros(5));
    let json = serde_json::to_string(&id).expect("serialize");
    let back: ProcessInstanceId = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(id, back);
    assert!(json.starts_with('"'), "expected a JSON string, got {json}");
}
