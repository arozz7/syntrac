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

#[test]
fn marking_a_program_as_known_is_a_request_with_a_path_a_label_and_an_optional_note() {
    let request = Request::KnownProductAdd {
        path: r"C:\Program Files (x86)\Acme\acmeserv.exe".into(),
        label: "Acme".into(),
        note: Some("my cloud backup".into()),
    };

    let line = encode_request(&request);

    let value: serde_json::Value = serde_json::from_str(line.trim_end()).unwrap();
    assert_eq!(value["request"], "known_product_add");
    assert_eq!(value["path"], r"C:\Program Files (x86)\Acme\acmeserv.exe");
    assert_eq!(value["label"], "Acme");
    assert_eq!(decode_request(line.as_bytes()).unwrap(), request);
}

#[test]
fn the_note_may_be_left_out() {
    let decoded = decode_request(br#"{"request":"known_product_add","path":"C:\\a.exe","label":"A"}"#).unwrap();

    assert_eq!(decoded, Request::KnownProductAdd { path: r"C:\a.exe".into(), label: "A".into(), note: None });
}

#[test]
fn removing_a_known_product_names_it_by_id() {
    let request = Request::KnownProductRemove { id: 7 };

    assert_eq!(encode_request(&request), "{\"request\":\"known_product_remove\",\"id\":7}\n");
    assert_eq!(decode_request(encode_request(&request).as_bytes()).unwrap(), request);
}

#[test]
fn a_write_request_without_its_required_fields_is_malformed() {
    assert!(matches!(
        decode_request(br#"{"request":"known_product_add","label":"A"}"#),
        Err(ProtocolError::Malformed(_))
    ));
    assert!(matches!(decode_request(br#"{"request":"known_product_remove"}"#), Err(ProtocolError::Malformed(_))));
    assert!(matches!(
        decode_request(br#"{"request":"known_product_remove","id":"seven"}"#),
        Err(ProtocolError::Malformed(_))
    ));
}

#[test]
fn the_replies_to_the_write_commands_round_trip() {
    for response in [Response::KnownProductSaved { id: 3 }, Response::Done] {
        assert_eq!(decode_response(encode_response(&response).as_bytes()).unwrap(), response);
    }
    assert_eq!(encode_response(&Response::Done), "{\"response\":\"done\"}\n");
}

#[test]
fn a_write_request_over_the_size_cap_is_refused_before_it_is_parsed() {
    let long_note = "x".repeat(MAX_REQUEST_BYTES);
    let line = format!(r#"{{"request":"known_product_add","path":"C:\\a.exe","label":"A","note":"{long_note}"}}"#);

    assert!(matches!(decode_request(line.as_bytes()), Err(ProtocolError::TooLarge { .. })));
}

#[test]
fn the_owner_data_requests_carry_nothing_and_cannot_name_an_address_to_fetch() {
    assert_eq!(encode_request(&Request::OwnerDataStatus), "{\"request\":\"owner_data_status\"}\n");
    assert_eq!(encode_request(&Request::OwnerDataRefresh), "{\"request\":\"owner_data_refresh\"}\n");
    // A client cannot say where to download from: any extra field is ignored, so there is nowhere to put an address.
    let decoded = decode_request(br#"{"request":"owner_data_refresh","url":"https://evil.example/x"}"#).unwrap();
    assert_eq!(decoded, Request::OwnerDataRefresh);
}

#[test]
fn the_owner_data_report_round_trips() {
    use syntrac_ipc::protocol::{OwnerDataReport, OwnerDatasetReport};
    let report = OwnerDataReport {
        dataset: Some(OwnerDatasetReport { source: "iptoasn.com".into(), downloaded_at_micros: 12, rows: 500_000 }),
        refreshing: true,
        last_error: Some("the server answered 503".into()),
        allowed: true,
    };
    let response = Response::OwnerData(report.clone());

    let line = encode_response(&response);

    let value: serde_json::Value = serde_json::from_str(line.trim_end()).unwrap();
    assert_eq!(value["response"], "owner_data");
    assert_eq!(value["dataset"]["rows"], 500_000);
    assert_eq!(decode_response(line.as_bytes()).unwrap(), response);
}
