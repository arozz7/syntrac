//! The wire protocol: one JSON object per line. The UI depends on these exact field names.

use syntrac_ipc::protocol::{
    decode_request, decode_response, encode_request, encode_response, CollectorTotals, EtwStatus, ProtocolError,
    Request, Response, StatusReport, MAX_REQUEST_BYTES,
};

fn sample_status() -> StatusReport {
    StatusReport {
        version: "0.1.0".into(),
        pid: 4242,
        started_at_micros: 1_794_000_000_000_000,
        uptime_secs: 61,
        database_path: r"C:\ProgramData\Syntrac\data\syntrac.db".into(),
        etw_enabled: true,
        last_tick_at_micros: Some(1_794_000_061_000_000),
        totals: CollectorTotals {
            ticks: 120,
            events_stored: 900,
            flows_stored: 300,
            metrics_samples: 12,
            source_errors: 0,
            metrics_errors: 0,
            storage_errors: 0,
        },
        etw: Some(EtwStatus { received: 9000, dropped: 0, parse_errors: 0, max_lag_ms: 1014, future_events: 0 }),
    }
}

#[test]
fn requests_round_trip_as_single_lines() {
    for request in [Request::Ping, Request::Status] {
        let line = encode_request(&request);
        assert!(line.ends_with('\n'));
        assert_eq!(line.matches('\n').count(), 1, "exactly one newline, at the end: {line:?}");
        assert_eq!(decode_request(line.trim_end().as_bytes()).unwrap(), request);
    }
}

#[test]
fn responses_round_trip_as_single_lines() {
    let responses = [
        Response::Pong { version: "0.1.0".into() },
        Response::Status(sample_status()),
        Response::Error { message: "nope".into() },
    ];
    for response in responses {
        let line = encode_response(&response);
        assert_eq!(line.matches('\n').count(), 1);
        assert_eq!(decode_response(line.trim_end().as_bytes()).unwrap(), response);
    }
}

#[test]
fn the_wire_format_is_stable_because_the_ui_is_written_against_it() {
    assert_eq!(encode_request(&Request::Ping), "{\"request\":\"ping\"}\n");
    assert_eq!(encode_request(&Request::Status), "{\"request\":\"status\"}\n");
    let pong = encode_response(&Response::Pong { version: "1.2.3".into() });
    assert_eq!(pong, "{\"response\":\"pong\",\"version\":\"1.2.3\"}\n");

    let status = serde_json::to_value(Response::Status(sample_status())).unwrap();
    assert_eq!(status["response"], "status");
    assert_eq!(status["totals"]["events_stored"], 900);
    assert_eq!(status["etw"]["dropped"], 0);
    assert_eq!(status["database_path"], r"C:\ProgramData\Syntrac\data\syntrac.db");
}

#[test]
fn a_status_without_etw_serializes_with_a_null_etw_section() {
    let mut status = sample_status();
    status.etw = None;
    status.etw_enabled = false;
    let value = serde_json::to_value(Response::Status(status)).unwrap();
    assert!(value["etw"].is_null());
}

#[test]
fn unknown_requests_and_garbage_are_protocol_errors_not_panics() {
    assert!(matches!(decode_request(b"{\"request\":\"format_disk\"}"), Err(ProtocolError::Malformed(_))));
    assert!(matches!(decode_request(b"not json at all"), Err(ProtocolError::Malformed(_))));
    assert!(matches!(decode_request(b""), Err(ProtocolError::Empty)));
    assert!(matches!(decode_request(b"   "), Err(ProtocolError::Empty)));
    assert!(matches!(decode_request(&[0xff, 0xfe, 0x00]), Err(ProtocolError::Malformed(_))));
}

#[test]
fn a_request_over_the_size_cap_is_rejected_before_parsing() {
    let huge = vec![b'a'; MAX_REQUEST_BYTES + 1];
    assert!(matches!(decode_request(&huge), Err(ProtocolError::TooLarge { .. })));
}

#[test]
fn extra_fields_in_a_request_are_ignored_so_newer_clients_still_work_with_older_services() {
    assert_eq!(decode_request(b"{\"request\":\"ping\",\"client\":\"ui\"}").unwrap(), Request::Ping);
}
