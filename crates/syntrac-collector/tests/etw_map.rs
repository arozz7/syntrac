//! Pure mapping from raw ETW field values to SYNTRAC events, using values captured in the Phase 0 spike.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use syntrac_collector::etw_map::{
    dns_resolution, network_activity, process_started, process_stopped, swap_port, DnsFields, NetworkFields,
    ProcessStartFields, ProcessStopFields,
};
use syntrac_collector::paths::NtPathConverter;
use syntrac_core::{EventPayload, NetAction, NetProtocol, ProcessInstanceId, Timestamp};

/// CreateTime of pid 5616 (jq.exe) from aiChangeLog/etw-spike-result-kw10.txt
const SPIKE_CREATE_TIME: u64 = 134_390_031_692_131_159;
const SPIKE_CREATED_MICROS: i64 = 1_794_529_569_213_115;

fn paths() -> NtPathConverter {
    NtPathConverter::new(vec![(r"\Device\HarddiskVolume7".to_owned(), "C:".to_owned())])
}

fn now() -> Timestamp {
    Timestamp::from_micros(SPIKE_CREATED_MICROS + 1_000)
}

fn v4(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(a, b, c, d))
}

fn tcp_fields(event_id: u16, size: u32) -> NetworkFields {
    NetworkFields {
        event_id,
        pid: 13632,
        size,
        daddr: v4(146, 143, 202, 83),
        saddr: v4(192, 168, 0, 132),
        dport: 47873, // 443 as it appears on the wire (network byte order read as little-endian)
        sport: 61934,
    }
}

fn activity(fields: NetworkFields) -> Option<(NetProtocol, NetAction, IpAddr, u16, u32)> {
    network_activity(now(), fields).map(|event| match event.payload {
        EventPayload::NetworkActivity { protocol, action, remote_addr, remote_port, bytes, .. } => {
            (protocol, action, remote_addr, remote_port, bytes)
        }
        other => panic!("unexpected {}", other.kind()),
    })
}

#[test]
fn process_start_uses_the_real_creation_time_and_a_drive_letter_path() {
    let fields = ProcessStartFields {
        pid: 5616,
        parent_pid: 32620,
        create_time: SPIKE_CREATE_TIME,
        image_name: r"\Device\HarddiskVolume7\Users\alice\anaconda3\Library\mingw-w64\bin\jq.exe".into(),
    };
    let event = process_started(now(), fields, &paths());

    assert_eq!(event.ts, now(), "event time stays the time we heard about it");
    match event.payload {
        EventPayload::ProcessStarted { pid, parent_pid, created, image_path, .. } => {
            assert_eq!((pid, parent_pid), (5616, 32620));
            assert_eq!(created.as_micros(), SPIKE_CREATED_MICROS);
            assert_eq!(image_path, r"C:\Users\alice\anaconda3\Library\mingw-w64\bin\jq.exe");
        }
        other => panic!("unexpected {}", other.kind()),
    }
}

#[test]
fn start_and_stop_of_the_same_process_yield_the_same_identity() {
    let start = process_started(
        now(),
        ProcessStartFields { pid: 5616, parent_pid: 1, create_time: SPIKE_CREATE_TIME, image_name: "x".into() },
        &paths(),
    );
    let stop =
        process_stopped(now(), ProcessStopFields { pid: 5616, create_time: SPIKE_CREATE_TIME, exit_code: Some(0) });

    let identity = |payload: &EventPayload| match payload {
        EventPayload::ProcessStarted { pid, created, .. } | EventPayload::ProcessExited { pid, created, .. } => {
            ProcessInstanceId::from_pid_and_creation(*pid, *created)
        }
        other => panic!("unexpected {}", other.kind()),
    };
    assert_eq!(identity(&start.payload), identity(&stop.payload));
}

#[test]
fn process_stop_carries_the_exit_code() {
    let stop = process_stopped(now(), ProcessStopFields { pid: 1, create_time: SPIKE_CREATE_TIME, exit_code: Some(3) });
    assert!(matches!(stop.payload, EventPayload::ProcessExited { exit_code: Some(3), .. }));
}

#[test]
fn ports_are_byte_swapped_from_network_order() {
    assert_eq!(swap_port(47873), 443);
    assert_eq!(swap_port(27655), 1900); // SSDP
    assert_eq!(swap_port(59668), 5353); // mDNS
    assert_eq!(swap_port(swap_port(1234)), 1234);
}

#[test]
fn tcp_send_reports_the_destination_as_remote_with_a_swapped_port_and_the_byte_count() {
    let (protocol, action, remote, port, bytes) = activity(tcp_fields(10, 1500)).unwrap();
    assert_eq!(
        (protocol, action, remote, port, bytes),
        (NetProtocol::Tcp, NetAction::Send, v4(146, 143, 202, 83), 443, 1500)
    );
}

#[test]
fn tcp_receive_also_uses_the_destination_fields_as_the_remote_endpoint() {
    // Observed in the spike: TCP receive events carry the REMOTE server in daddr/dport.
    let (_, action, remote, port, bytes) = activity(tcp_fields(11, 900)).unwrap();
    assert_eq!((action, remote, port, bytes), (NetAction::Receive, v4(146, 143, 202, 83), 443, 900));
}

#[test]
fn udp_send_uses_the_destination_as_remote() {
    let (protocol, action, remote, port, _) = activity(tcp_fields(42, 60)).unwrap();
    assert_eq!((protocol, action, remote, port), (NetProtocol::Udp, NetAction::Send, v4(146, 143, 202, 83), 443));
}

#[test]
fn udp_receive_uses_the_source_as_remote_because_the_destination_is_this_machine() {
    // Observed in the spike: for UDP receive, daddr was this machine (192.168.1.50) or a multicast group.
    let fields = NetworkFields {
        event_id: 43,
        pid: 12448,
        size: 120,
        daddr: v4(192, 168, 0, 132),
        saddr: v4(10, 0, 0, 5),
        dport: 61934,
        sport: 47873,
    };
    let (protocol, action, remote, port, bytes) = activity(fields).unwrap();
    assert_eq!(
        (protocol, action, remote, port, bytes),
        (NetProtocol::Udp, NetAction::Receive, v4(10, 0, 0, 5), 443, 120)
    );
}

#[test]
fn connection_lifecycle_events_have_no_byte_count() {
    let cases = [(12, NetAction::Connect), (15, NetAction::Accept), (13, NetAction::Close)];
    for (event_id, expected) in cases {
        let (_, action, _, _, bytes) = activity(tcp_fields(event_id, 777)).unwrap();
        assert_eq!((action, bytes), (expected, 0), "event {event_id}");
    }
}

#[test]
fn a_failure_event_never_reports_its_error_code_as_bytes() {
    let (_, action, _, _, bytes) = activity(tcp_fields(17, 10060)).unwrap();
    assert_eq!((action, bytes), (NetAction::Fail, 0));
}

#[test]
fn retransmits_reconnects_and_user_copy_events_are_ignored_to_avoid_double_counting() {
    // 18 and 34 ("bytes copied on behalf of user") duplicate the bytes already seen in 10/11.
    for ignored in [14, 16, 18, 30, 32, 34, 999] {
        assert!(network_activity(now(), tcp_fields(ignored, 100)).is_none(), "event {ignored} must be ignored");
    }
}

#[test]
fn failure_events_need_no_endpoint_because_their_payload_is_only_an_error_code() {
    use syntrac_collector::etw_map::network_field_needs;
    // Manifest: "TCPv4: Connection attempt failed with error code %2." has no addresses or ports.
    for fail in [17, 49] {
        let needs = network_field_needs(fail);
        assert!(!needs.endpoint && !needs.size, "event {fail}");
    }
}

#[test]
fn data_events_need_endpoint_and_size_and_lifecycle_events_only_the_endpoint() {
    use syntrac_collector::etw_map::network_field_needs;
    for data in [10, 11, 26, 27, 42, 43, 58, 59] {
        let needs = network_field_needs(data);
        assert!(needs.endpoint && needs.size, "event {data}");
    }
    for lifecycle in [12, 13, 15, 28, 29, 31] {
        let needs = network_field_needs(lifecycle);
        assert!(needs.endpoint && !needs.size, "event {lifecycle}");
    }
    let ignored = network_field_needs(18);
    assert!(!ignored.endpoint && !ignored.size);
}

#[test]
fn the_pre_checks_agree_with_what_the_mappers_would_accept() {
    for event_id in 0..=3100u16 {
        assert_eq!(
            syntrac_collector::etw_map::is_tracked_network_event(event_id),
            network_activity(now(), tcp_fields(event_id, 1)).is_some(),
            "network event {event_id}"
        );
        assert_eq!(
            syntrac_collector::etw_map::is_tracked_dns_event(event_id),
            dns_resolution(now(), dns(event_id, "example.org", Some(""))).is_some(),
            "dns event {event_id}"
        );
    }
}

#[test]
fn ipv6_events_map_like_their_ipv4_counterparts() {
    let multicast = IpAddr::V6(Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 0xfb));
    let fields = NetworkFields {
        event_id: 58, // UDPv6 send
        pid: 12620,
        size: 80,
        daddr: multicast,
        saddr: IpAddr::V6(Ipv6Addr::LOCALHOST),
        dport: 59668,
        sport: 59668,
    };
    let (protocol, action, remote, port, _) = activity(fields).unwrap();
    assert_eq!((protocol, action, remote, port), (NetProtocol::Udp, NetAction::Send, multicast, 5353));
}

#[test]
fn ipv4_mapped_ipv6_addresses_are_canonicalised_to_ipv4() {
    let mapped = IpAddr::V6("::ffff:34.149.66.165".parse().unwrap());
    let mut fields = tcp_fields(26, 10); // TCPv6 send
    fields.daddr = mapped;
    let (_, _, remote, _, _) = activity(fields).unwrap();
    assert_eq!(remote, v4(34, 149, 66, 165));
}

fn dns(event_id: u16, query: &str, results: Option<&str>) -> DnsFields {
    DnsFields { event_id, caller_pid: 68428, query_name: query.to_owned(), results: results.map(str::to_owned) }
}

fn resolved(fields: DnsFields) -> Option<(u32, String, Vec<IpAddr>)> {
    dns_resolution(now(), fields).map(|event| match event.payload {
        EventPayload::DnsQuery { pid, query, addresses } => (pid, query, addresses),
        other => panic!("unexpected {}", other.kind()),
    })
}

#[test]
fn a_dns_response_with_the_spike_results_string_yields_canonical_addresses() {
    let fields = dns(3008, "http-intake.logs.us5.datadoghq.com", Some("2600:1901:0:9e23::;::ffff:34.149.66.165;"));
    let (pid, query, addresses) = resolved(fields).unwrap();

    assert_eq!(pid, 68428, "the caller, taken from the event header");
    assert_eq!(query, "http-intake.logs.us5.datadoghq.com");
    assert_eq!(addresses, vec![IpAddr::V6("2600:1901:0:9e23::".parse().unwrap()), v4(34, 149, 66, 165)]);
}

#[test]
fn a_dns_response_with_no_results_is_still_recorded_with_no_addresses() {
    let (_, _, addresses) = resolved(dns(3008, "nothing.example", Some(""))).unwrap();
    assert!(addresses.is_empty());
}

#[test]
fn only_the_caller_side_response_event_is_recorded_so_a_query_is_not_stored_twice() {
    for ignored in [1001, 1015, 3006, 3009, 3010, 3011, 3016, 3018, 3019, 3020] {
        assert!(dns_resolution(now(), dns(ignored, "example.org", Some("1.2.3.4;"))).is_none(), "event {ignored}");
    }
}

#[test]
fn non_address_entries_and_duplicates_in_the_results_are_dropped() {
    let (_, _, addresses) =
        resolved(dns(3008, "alias.example", Some("type: 5 real.example.com;1.2.3.4;1.2.3.4;not-an-ip;;"))).unwrap();
    assert_eq!(addresses, vec![v4(1, 2, 3, 4)]);
}

#[test]
fn query_names_are_lowercased_and_lose_a_trailing_dot() {
    let (_, query, _) = resolved(dns(3008, "Example.ORG.", Some(""))).unwrap();
    assert_eq!(query, "example.org");
}

#[test]
fn a_response_without_a_query_name_is_dropped() {
    assert!(dns_resolution(now(), dns(3008, "", Some("1.2.3.4;"))).is_none());
}
