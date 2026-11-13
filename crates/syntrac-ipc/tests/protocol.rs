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
        etw: Some(EtwStatus {
            received: 9000,
            dropped: 0,
            parse_errors: 0,
            max_lag_ms: 1014,
            future_events: 0,
            silent_secs: 240,
        }),
        storage: None,
    }
}

#[test]
fn the_etw_silence_is_on_the_wire_and_a_reply_from_an_older_service_without_it_still_parses() {
    let value: serde_json::Value =
        serde_json::from_str(encode_response(&Response::Status(sample_status())).trim_end()).expect("valid json");
    assert_eq!(value["etw"]["silent_secs"], 240, "{value}");

    let mut old = value;
    old["etw"].as_object_mut().expect("object").remove("silent_secs");
    let decoded = decode_response(old.to_string().as_bytes()).expect("an older reply must still decode");
    match decoded {
        Response::Status(status) => assert_eq!(status.etw.expect("etw").silent_secs, 0),
        other => panic!("expected a status, got {other:?}"),
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

#[test]
fn storage_state_round_trips_and_an_older_reply_without_it_still_decodes() {
    use syntrac_ipc::protocol::{StorageLevel, StorageStatus};

    let mut report = sample_status();
    report.storage = Some(StorageStatus {
        level: StorageLevel::Trimming,
        database_file_bytes: 3_000_000_000,
        database_live_bytes: 2_900_000_000,
        drive_free_bytes: 18_000_000_000,
        oldest_detail_micros: Some(1_000),
        oldest_rollup_micros: Some(500),
        trimmed_hours_total: 7,
    });
    let line = encode_response(&Response::Status(report.clone()));
    assert_eq!(decode_response(line.trim_end().as_bytes()).unwrap(), Response::Status(report));

    // A reply from a service that predates the storage guard has no "storage" key at all.
    let mut value: serde_json::Value =
        serde_json::from_str(encode_response(&Response::Status(sample_status())).trim_end()).unwrap();
    value.as_object_mut().unwrap().remove("storage");
    match decode_response(value.to_string().as_bytes()).unwrap() {
        Response::Status(status) => assert_eq!(status.storage, None),
        other => panic!("expected a status, got {other:?}"),
    }
}

#[test]
fn the_storage_level_uses_stable_lowercase_names() {
    use syntrac_ipc::protocol::StorageLevel;
    assert_eq!(serde_json::to_string(&StorageLevel::Ok).unwrap(), "\"ok\"");
    assert_eq!(serde_json::to_string(&StorageLevel::Trimming).unwrap(), "\"trimming\"");
    assert_eq!(serde_json::to_string(&StorageLevel::Paused).unwrap(), "\"paused\"");
}
