//! Startup snapshot mapping: existing processes and connections become events, in a safe order.

use std::net::{IpAddr, Ipv4Addr};

use syntrac_collector::etw_map::{process_started, ProcessStartFields};
use syntrac_collector::paths::NtPathConverter;
use syntrac_collector::snapshot_map::{
    established_connection, ipv4_from_table_dword, observed_processes, table_port, SnapshotProcess,
    TCP_STATE_ESTABLISHED,
};
use syntrac_core::{EventPayload, NetAction, NetProtocol, ProcessInstanceId, Timestamp};

const SPIKE_CREATE_TIME: u64 = 134_390_031_692_131_159;

fn entry(pid: u32, parent_pid: u32, created: Option<u64>, image: Option<&str>) -> SnapshotProcess {
    SnapshotProcess { pid, parent_pid, create_time: created, image_path: image.map(str::to_owned) }
}

fn taken_at() -> Timestamp {
    Timestamp::from_micros(1_794_529_600_000_000)
}

fn identity(payload: &EventPayload) -> ProcessInstanceId {
    match payload {
        EventPayload::ProcessStarted { pid, created, .. } | EventPayload::ProcessObserved { pid, created, .. } => {
            ProcessInstanceId::from_pid_and_creation(*pid, *created)
        }
        other => panic!("unexpected {}", other.kind()),
    }
}

#[test]
fn snapshotted_processes_are_marked_observed_not_started() {
    let result = observed_processes(vec![entry(10, 4, Some(SPIKE_CREATE_TIME), Some(r"C:\a.exe"))], taken_at());
    assert!(matches!(result.events[0].payload, EventPayload::ProcessObserved { .. }));
}

#[test]
fn the_snapshot_and_etw_agree_on_the_identity_of_the_same_process() {
    let snapshot =
        observed_processes(vec![entry(5616, 32620, Some(SPIKE_CREATE_TIME), Some(r"C:\jq.exe"))], taken_at());
    let live = process_started(
        taken_at(),
        ProcessStartFields { pid: 5616, parent_pid: 32620, create_time: SPIKE_CREATE_TIME, image_name: "x".into() },
        &NtPathConverter::new(vec![]),
    );
    assert_eq!(identity(&snapshot.events[0].payload), identity(&live.payload));
}

#[test]
fn parents_come_before_children_regardless_of_enumeration_order() {
    let parent_created = SPIKE_CREATE_TIME;
    let child_created = SPIKE_CREATE_TIME + 10_000_000; // one second later
    let result = observed_processes(
        vec![
            entry(200, 100, Some(child_created), Some(r"C:\child.exe")),
            entry(100, 4, Some(parent_created), Some(r"C:\parent.exe")),
        ],
        taken_at(),
    );
    let pids: Vec<u32> = result.events.iter().map(|e| e.payload.pid()).collect();
    assert_eq!(pids, vec![100, 200]);
}

#[test]
fn every_snapshot_event_carries_the_time_the_snapshot_began() {
    let result = observed_processes(
        vec![entry(1, 0, Some(SPIKE_CREATE_TIME), None), entry(2, 0, Some(SPIKE_CREATE_TIME + 5), None)],
        taken_at(),
    );
    assert!(result.events.iter().all(|e| e.ts == taken_at()));
}

#[test]
fn processes_we_cannot_inspect_are_skipped_and_counted_not_fatal() {
    let result = observed_processes(
        vec![entry(4, 0, None, None), entry(10, 4, Some(SPIKE_CREATE_TIME), Some(r"C:\ok.exe"))],
        taken_at(),
    );
    assert_eq!(result.events.len(), 1);
    assert_eq!(result.skipped, 1);
}

#[test]
fn a_missing_image_path_becomes_an_empty_string() {
    let result = observed_processes(vec![entry(10, 4, Some(SPIKE_CREATE_TIME), None)], taken_at());
    match &result.events[0].payload {
        EventPayload::ProcessObserved { image_path, .. } => assert!(image_path.is_empty()),
        other => panic!("unexpected {}", other.kind()),
    }
}

#[test]
fn the_os_connection_table_stores_addresses_and_ports_in_network_order() {
    // The 4 address bytes 146.143.202.83 read as a little-endian DWORD.
    let dword = u32::from_le_bytes([146, 143, 202, 83]);
    assert_eq!(ipv4_from_table_dword(dword), Ipv4Addr::new(146, 143, 202, 83));
    // Port 443 appears as 47873 (0xBB01) in the low 16 bits; the high bits are padding.
    assert_eq!(table_port(47873), 443);
    assert_eq!(table_port(0xABCD_BB01), 443);
}

#[test]
fn an_established_tcp_row_becomes_an_accept_activity_with_the_remote_endpoint() {
    let event = established_connection(
        TCP_STATE_ESTABLISHED,
        12448,
        IpAddr::V4(Ipv4Addr::new(146, 143, 202, 83)),
        443,
        taken_at(),
    )
    .expect("established rows are reported");

    match event.payload {
        EventPayload::NetworkActivity { pid, protocol, action, remote_addr, remote_port, bytes } => {
            assert_eq!(
                (pid, protocol, action, remote_port, bytes),
                (12448, NetProtocol::Tcp, NetAction::Accept, 443, 0)
            );
            assert_eq!(remote_addr, IpAddr::V4(Ipv4Addr::new(146, 143, 202, 83)));
        }
        other => panic!("unexpected {}", other.kind()),
    }
}

#[test]
fn only_established_rows_are_reported() {
    let remote = IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4));
    for state in [1u32, 2, 3, 4, 6, 7, 8, 9, 10, 11, 12] {
        assert!(established_connection(state, 1, remote, 443, taken_at()).is_none(), "state {state}");
    }
}

#[test]
fn unspecified_remote_addresses_are_not_connections() {
    let unspecified = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
    assert!(established_connection(TCP_STATE_ESTABLISHED, 1, unspecified, 0, taken_at()).is_none());
}
