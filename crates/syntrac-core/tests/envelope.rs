//! The normalized event envelope is the contract between every source and the brain.

use std::net::{IpAddr, Ipv4Addr};

use syntrac_core::{
    EventCategory, EventEnvelope, EventPayload, NetAction, NetProtocol, ProcessInstanceId, Severity, Timestamp,
};
use uuid::Uuid;

const REMOTE: IpAddr = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 9));

fn sample_payloads() -> Vec<EventPayload> {
    let created = Timestamp::from_micros(100);
    vec![
        EventPayload::ProcessStarted {
            pid: 10,
            parent_pid: 4,
            created,
            image_path: r"C:\Windows\System32\cmd.exe".into(),
            command_line: Some("cmd.exe /c whoami".into()),
        },
        EventPayload::ProcessExited { pid: 10, created, exit_code: Some(0) },
        EventPayload::ProcessObserved {
            pid: 10,
            parent_pid: 4,
            created,
            image_path: r"C:\Windows\System32\cmd.exe".into(),
            command_line: None,
        },
        EventPayload::NetworkActivity {
            pid: 10,
            protocol: NetProtocol::Tcp,
            action: NetAction::Send,
            remote_addr: REMOTE,
            remote_port: 443,
            bytes: 1200,
        },
        EventPayload::NetworkFlow {
            pid: 10,
            protocol: NetProtocol::Tcp,
            remote_addr: REMOTE,
            remote_port: 443,
            bytes_sent: 1200,
            bytes_received: 90_000,
            opened: 1,
            closed: 0,
            failed: 0,
            window_start: Timestamp::from_micros(0),
            window_end: Timestamp::from_micros(10_000_000),
        },
        EventPayload::DnsQuery { pid: 10, query: "example.org".into(), addresses: vec![REMOTE] },
    ]
}

#[test]
fn every_payload_round_trips_through_json() {
    for payload in sample_payloads() {
        let envelope = EventEnvelope {
            id: Uuid::from_u128(1),
            ts: Timestamp::from_micros(123),
            process_instance_id: Some(ProcessInstanceId::from_pid_and_creation(10, Timestamp::from_micros(100))),
            severity: Some(Severity::Unusual),
            payload,
        };
        let json = serde_json::to_string(&envelope).expect("serialize");
        let back: EventEnvelope = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(envelope, back);
    }
}

#[test]
fn category_is_derived_from_the_payload() {
    let categories: Vec<EventCategory> = sample_payloads().iter().map(EventPayload::category).collect();
    assert_eq!(
        categories,
        vec![
            EventCategory::Process,
            EventCategory::Process,
            EventCategory::Process,
            EventCategory::Network,
            EventCategory::Network,
            EventCategory::Dns
        ]
    );
}

#[test]
fn kind_string_matches_the_serialized_tag() {
    for payload in sample_payloads() {
        let value = serde_json::to_value(&payload).expect("serialize");
        assert_eq!(value["kind"], payload.kind());
    }
}

#[test]
fn severity_tiers_are_ordered_from_unusual_to_confirmed() {
    assert!(Severity::Unusual < Severity::Suspicious);
    assert!(Severity::Suspicious < Severity::Confirmed);
}

#[test]
fn pid_is_exposed_for_every_payload() {
    for payload in sample_payloads() {
        assert_eq!(payload.pid(), 10);
    }
}

#[test]
fn raw_network_activity_is_never_persisted_but_everything_else_is() {
    for payload in sample_payloads() {
        let expected = !matches!(payload, EventPayload::NetworkActivity { .. });
        assert_eq!(payload.is_persisted(), expected, "{}", payload.kind());
    }
}

#[test]
fn a_snapshotted_process_has_a_distinct_kind_from_a_live_start() {
    let kinds: Vec<&str> = sample_payloads().iter().map(EventPayload::kind).collect();
    assert!(kinds.contains(&"process_started"));
    assert!(kinds.contains(&"process_observed"));
}
