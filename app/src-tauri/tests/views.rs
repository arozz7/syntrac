//! The view models the UI renders, built from the repository traits and tested against the in-memory store.

use std::net::{IpAddr, Ipv4Addr};

use syntrac_app_lib::views::{
    clamp_limit, clamp_range_secs, connection_list, overview, process_list, ProcessListParams, MAX_RANGE_SECS,
    MIN_RANGE_SECS,
};
use syntrac_core::{EventRepository, MetricsRepository, ProcessInstanceId, ProcessRepository, Timestamp};
use syntrac_testkit::fixtures::{
    at_secs, gpu_process_metric, metrics_sample, net_flow, process_metric, running_process, with_gpu_reading,
    EventFactory, BEACON_TARGET,
};
use syntrac_testkit::InMemoryRepository;

fn now() -> Timestamp {
    at_secs(10_000)
}

fn named(pid: u32, started: i64, path: &str) -> syntrac_core::ProcessRecord {
    let mut record = running_process(pid, at_secs(started));
    record.image_path = path.into();
    record
}

fn flow_for(
    f: &mut EventFactory,
    at: i64,
    owner: Option<ProcessInstanceId>,
    pid: u32,
    sent: u64,
    received: u64,
) -> syntrac_core::EventEnvelope {
    let mut event = f.make(at_secs(at), net_flow(pid, BEACON_TARGET, sent, received, at));
    event.process_instance_id = owner;
    event
}

#[test]
fn the_cpu_breakdown_names_each_process_and_shows_what_no_process_explains() {
    let mut repo = InMemoryRepository::default();
    let chrome = named(1, 100, r"C:\Program Files\Google\Chrome\chrome.exe");
    repo.record_start(chrome.clone()).unwrap();
    // system 40%, chrome 20%, long tail 4% -> 16% unexplained
    repo.append_metrics(&metrics_sample(9_900, 40.0, vec![process_metric(1, Some(chrome.id), 20.0)], 4.0, 50)).unwrap();

    let view = overview(&repo, now(), 3600).unwrap();

    let top = &view.cpu.processes[0];
    assert_eq!((top.name.as_str(), top.pid), ("chrome.exe", 1));
    assert!((top.avg_percent - 20.0).abs() < 1e-3);
    assert!((view.cpu.unaccounted_avg_percent - 16.0).abs() < 1e-3);
    assert!((view.cpu.others_avg_percent - 4.0).abs() < 1e-3);
    assert_eq!(top.instance.as_deref(), Some(chrome.id.as_uuid().to_string().as_str()));
}

#[test]
fn a_process_the_collector_could_not_identify_is_labelled_by_its_pid() {
    let mut repo = InMemoryRepository::default();
    repo.append_metrics(&metrics_sample(9_900, 10.0, vec![process_metric(555, None, 5.0)], 0.0, 0)).unwrap();
    let view = overview(&repo, now(), 3600).unwrap();
    assert_eq!(view.cpu.processes[0].name, "pid 555");
    assert!(view.cpu.processes[0].instance.is_none());
}

#[test]
fn the_series_carries_memory_as_a_percentage_and_stays_within_the_point_limit() {
    let mut repo = InMemoryRepository::default();
    for i in 0..1000 {
        repo.append_metrics(&metrics_sample(9_000 + i, 10.0, vec![], 0.0, 0)).unwrap();
    }
    let view = overview(&repo, now(), 7200).unwrap();
    assert!(view.system.len() <= 240, "{} points", view.system.len());
    // the fixture uses 8 GB of 32 GB
    assert!((view.system[0].memory_percent - 25.0).abs() < 1e-3);
    assert!(view.system.windows(2).all(|pair| pair[0].ts_ms <= pair[1].ts_ms), "oldest first");
}

#[test]
fn destinations_are_listed_busiest_first_with_text_addresses() {
    let mut repo = InMemoryRepository::default();
    let mut f = EventFactory::default();
    repo.append(&[flow_for(&mut f, 9_950, None, 7, 100, 9_000)]).unwrap();
    let view = overview(&repo, now(), 3600).unwrap();
    let top = &view.destinations[0];
    assert_eq!((top.remote_addr.as_str(), top.remote_port), ("203.0.113.9", 443));
    assert_eq!((top.bytes_sent, top.bytes_received, top.flow_count), (100, 9_000, 1));
    let _ = IpAddr::V4(Ipv4Addr::LOCALHOST);
}

#[test]
fn traffic_to_this_machine_itself_is_not_a_network_destination() {
    // Real data showed 16 MB of loopback IPC outranking every real server.
    let mut repo = InMemoryRepository::default();
    let mut f = EventFactory::default();
    let mut local = f.make(at_secs(9_950), net_flow(7, Ipv4Addr::LOCALHOST, 50_000_000, 50_000_000, 9_950));
    local.process_instance_id = None;
    repo.append(&[local, flow_for(&mut f, 9_951, None, 7, 100, 200)]).unwrap();

    let view = overview(&repo, now(), 3600).unwrap();

    assert_eq!(view.destinations.len(), 1, "{:?}", view.destinations);
    assert_eq!(view.destinations[0].remote_addr, "203.0.113.9");
}

#[test]
fn loopback_does_not_crowd_real_destinations_out_of_the_limited_list() {
    let mut repo = InMemoryRepository::default();
    let mut f = EventFactory::default();
    let mut events = Vec::new();
    for port_offset in 0..20u8 {
        // twenty loopback endpoints, each far busier than the one real server
        events.push(f.make(
            at_secs(9_900),
            net_flow(7, Ipv4Addr::new(127, 0, 0, 1 + port_offset), 900_000_000, 900_000_000, 9_900),
        ));
    }
    events.push(flow_for(&mut f, 9_950, None, 7, 1, 1));
    repo.append(&events).unwrap();

    let view = overview(&repo, now(), 3600).unwrap();
    assert!(view.destinations.iter().any(|d| d.remote_addr == "203.0.113.9"), "{:?}", view.destinations);
}

#[test]
fn an_empty_store_gives_an_empty_but_valid_overview() {
    let view = overview(&InMemoryRepository::default(), now(), 3600).unwrap();
    assert!(view.system.is_empty() && view.cpu.processes.is_empty() && view.destinations.is_empty());
    assert_eq!(view.cpu.window_samples, 0);
}

#[test]
fn the_requested_range_is_clamped_to_something_sensible() {
    assert_eq!(clamp_range_secs(5), MIN_RANGE_SECS);
    assert_eq!(clamp_range_secs(u64::MAX), MAX_RANGE_SECS);
    assert_eq!(clamp_range_secs(900), 900);
    let view = overview(&InMemoryRepository::default(), now(), 1).unwrap();
    assert_eq!(view.to_ms - view.from_ms, (MIN_RANGE_SECS * 1000) as i64);
}

#[test]
fn row_limits_are_clamped_with_a_default() {
    assert_eq!(clamp_limit(None, 500, 2000), 500);
    assert_eq!(clamp_limit(Some(0), 500, 2000), 1);
    assert_eq!(clamp_limit(Some(10_000_000), 500, 2000), 2000);
    assert_eq!(clamp_limit(Some(50), 500, 2000), 50);
}

#[test]
fn processes_are_listed_with_display_names_and_lifetime_flags() {
    let mut repo = InMemoryRepository::default();
    let parent = named(1, 100, r"C:\apps\launcher.exe");
    let mut child = named(2, 200, r"C:\apps\worker.exe");
    child.parent_id = Some(parent.id);
    let gone = named(3, 300, r"C:\apps\gone.exe");
    for p in [&parent, &child, &gone] {
        repo.record_start((*p).clone()).unwrap();
    }
    repo.reconcile_running(&[parent.id, child.id], at_secs(400)).unwrap(); // `gone` exited while we were down

    let rows = process_list(&repo, &ProcessListParams { limit: Some(100), ..ProcessListParams::default() }).unwrap();

    let by_name = |name: &str| rows.iter().find(|r| r.name == name).unwrap();
    assert!(by_name("launcher.exe").running && by_name("worker.exe").running);
    let gone_row = by_name("gone.exe");
    assert!(!gone_row.running && gone_row.ended_approximate && gone_row.ended_ms.is_some());
    assert_eq!(by_name("worker.exe").parent_id.as_deref(), Some(parent.id.as_uuid().to_string().as_str()));
    assert_eq!(rows[0].name, "gone.exe", "newest start first");
}

#[test]
fn the_process_list_can_show_only_running_processes_and_search_by_name() {
    let mut repo = InMemoryRepository::default();
    let alive = named(1, 100, r"C:\apps\Browser.exe");
    let dead = named(2, 200, r"C:\apps\browser-helper.exe");
    repo.record_start(alive.clone()).unwrap();
    repo.record_start(dead.clone()).unwrap();
    repo.record_exit(dead.id, at_secs(300), Some(0)).unwrap();

    let running =
        process_list(&repo, &ProcessListParams { running_only: true, ..ProcessListParams::default() }).unwrap();
    assert_eq!(running.len(), 1);

    let found =
        process_list(&repo, &ProcessListParams { search: Some("BROWSER".into()), ..ProcessListParams::default() })
            .unwrap();
    assert_eq!(found.len(), 2, "case-insensitive substring match");
}

#[test]
fn connections_name_their_process_and_come_newest_first() {
    let mut repo = InMemoryRepository::default();
    let browser = named(1, 100, r"C:\apps\browser.exe");
    repo.record_start(browser.clone()).unwrap();
    let mut f = EventFactory::default();
    repo.append(&[flow_for(&mut f, 9_900, Some(browser.id), 1, 10, 20), flow_for(&mut f, 9_950, None, 99, 1, 1)])
        .unwrap();

    let rows = connection_list(&repo, now(), 3600, Some(10)).unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(
        (rows[0].name.as_str(), rows[0].pid),
        ("pid 99", 99),
        "newest first; unattributed flows are named by pid"
    );
    assert_eq!(rows[1].name, "browser.exe");
    assert_eq!((rows[1].protocol.as_str(), rows[1].remote_port, rows[1].bytes_received), ("tcp", 443, 20));
}

// ---- the GPU (docs/ALERTS-PLAN.md, live views L1)

#[test]
fn the_gpu_breakdown_names_each_program_with_its_average_peak_and_memory() {
    let mut repo = InMemoryRepository::default();
    let chrome = named(1, 100, r"C:\Program Files\Google\Chrome\chrome.exe");
    repo.record_start(chrome.clone()).unwrap();
    for (at, gpu, memory) in [(9_800, 60.0, 2_000), (9_900, 20.0, 8_000)] {
        repo.append_metrics(&with_gpu_reading(
            metrics_sample(at, 10.0, vec![gpu_process_metric(1, Some(chrome.id), 1.0, gpu, memory)], 0.0, 0),
            Some(70.0),
        ))
        .unwrap();
    }

    let view = overview(&repo, now(), 3600).unwrap();

    assert_eq!(view.gpu.reading_samples, 2);
    let top = &view.gpu.processes[0];
    assert_eq!((top.name.as_str(), top.pid), ("chrome.exe", 1));
    assert!((top.avg_percent - 40.0).abs() < 1e-3 && (top.peak_percent - 60.0).abs() < 1e-3);
    assert_eq!(top.memory_peak_bytes, 8_000);
    assert_eq!(top.instance.as_deref(), Some(chrome.id.as_uuid().to_string().as_str()));
    assert!((view.gpu.system_avg_percent - 70.0).abs() < 1e-3 && (view.gpu.system_peak_percent - 70.0).abs() < 1e-3);
}

#[test]
fn a_period_without_gpu_readings_says_so_instead_of_showing_an_idle_gpu() {
    let mut repo = InMemoryRepository::default();
    repo.append_metrics(&metrics_sample(9_900, 10.0, vec![process_metric(555, None, 5.0)], 0.0, 0)).unwrap();

    let view = overview(&repo, now(), 3600).unwrap();

    assert_eq!(view.gpu.reading_samples, 0);
    assert!(view.gpu.processes.is_empty());
    assert_eq!(view.system[0].gpu_percent, None, "no reading is not 0%");
}

#[test]
fn the_series_carries_the_gpu_and_the_downsampling_does_not_dilute_it_with_samples_that_had_no_reading() {
    let mut repo = InMemoryRepository::default();
    for i in 0..1000 {
        // The first 200 samples have no GPU counters; after that every other sample has a reading of 40%.
        let reading = (i >= 200 && i % 2 == 0).then_some(40.0);
        repo.append_metrics(&with_gpu_reading(metrics_sample(9_000 + i, 10.0, vec![], 0.0, 0), reading)).unwrap();
    }

    let view = overview(&repo, now(), 7200).unwrap();

    assert!(view.system.len() <= 240);
    assert_eq!(view.system[0].gpu_percent, None, "a point made only of samples without a reading has none");
    let last = view.system.last().unwrap().gpu_percent.expect("a reading at the end");
    assert!((last - 40.0).abs() < 1e-3, "40% of the samples that had a reading, not 20% of all of them: {last}");
}
