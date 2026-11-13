//! The normalized event envelope is the contract between every source and the brain.

use std::net::{IpAddr, Ipv4Addr};

use syntrac_core::{EventCategory, EventEnvelope, EventPayload, NetProtocol, ProcessInstanceId, Severity, Timestamp};
use uuid::Uuid;

fn sample_payloads() -> Vec<EventPayload> {
    vec![
        EventPayload::ProcessStarted {
            pid: 10,
            parent_pid: 4,
            image_path: r"C:\Windows\System32\cmd.exe".into(),
            command_line: Some("cmd.exe /c whoami".into()),
        },
        EventPayload::ProcessExited { pid: 10, exit_code: Some(0) },
        EventPayload::NetworkConnection {
            pid: 10,
            protocol: NetProtocol::Tcp,
            local_addr: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 5)),
            local_port: 50_000,
            remote_addr: IpAddr::V4(Ipv4Addr::new(203, 0, 113, 9)),
            remote_port: 443,
        },
        EventPayload::DnsQuery { pid: 10, query: "example.org".into() },
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
        vec![EventCategory::Process, EventCategory::Process, EventCategory::Network, EventCategory::Dns]
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
