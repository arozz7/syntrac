//! Deterministic builders and replayable scenarios for tests.

use std::net::{IpAddr, Ipv4Addr};

use syntrac_core::{
    EventEnvelope, EventPayload, MetricsSample, NetAction, NetProtocol, OtherProcesses, ProcessInstanceId,
    ProcessMetric, ProcessRecord, Severity, SystemMetrics, Timestamp,
};
use uuid::Uuid;

/// One second in microseconds, to keep scenarios readable.
pub const SEC: i64 = 1_000_000;

pub fn at_secs(secs: i64) -> Timestamp {
    Timestamp::from_micros(secs * SEC)
}

/// Builds events with predictable ids (1, 2, 3, ...).
#[derive(Debug, Default)]
pub struct EventFactory {
    next_id: u128,
}

impl EventFactory {
    /// A factory whose ids start after `last_used`, to combine events from several factories safely.
    pub fn after(last_used: u128) -> Self {
        Self { next_id: last_used }
    }

    pub fn make(&mut self, ts: Timestamp, payload: EventPayload) -> EventEnvelope {
        self.next_id += 1;
        EventEnvelope { id: Uuid::from_u128(self.next_id), ts, process_instance_id: None, severity: None, payload }
    }
}

/// A live process start; `created_secs` is the OS creation time (the event itself happens at the same moment).
pub fn process_started(pid: u32, parent_pid: u32, created_secs: i64, image: &str) -> EventPayload {
    EventPayload::ProcessStarted {
        pid,
        parent_pid,
        created: at_secs(created_secs),
        image_path: image.to_owned(),
        command_line: None,
    }
}

/// A process found by the startup snapshot.
pub fn process_observed(pid: u32, parent_pid: u32, created_secs: i64, image: &str) -> EventPayload {
    EventPayload::ProcessObserved {
        pid,
        parent_pid,
        created: at_secs(created_secs),
        image_path: image.to_owned(),
        command_line: None,
    }
}

pub fn process_exited(pid: u32, created_secs: i64) -> EventPayload {
    EventPayload::ProcessExited { pid, created: at_secs(created_secs), exit_code: Some(0) }
}

pub fn net_activity(pid: u32, action: NetAction, remote: Ipv4Addr, bytes: u32) -> EventPayload {
    EventPayload::NetworkActivity {
        pid,
        protocol: NetProtocol::Tcp,
        action,
        remote_addr: IpAddr::V4(remote),
        remote_port: 443,
        bytes,
    }
}

/// An already-aggregated flow, for storage tests.
pub fn net_flow(pid: u32, remote: Ipv4Addr, sent: u64, received: u64, window_start_secs: i64) -> EventPayload {
    EventPayload::NetworkFlow {
        pid,
        protocol: NetProtocol::Tcp,
        remote_addr: IpAddr::V4(remote),
        remote_port: 443,
        bytes_sent: sent,
        bytes_received: received,
        opened: 1,
        closed: 0,
        failed: 0,
        window_start: at_secs(window_start_secs),
        window_end: at_secs(window_start_secs + 10),
    }
}

pub fn dns_query(pid: u32, query: &str, addresses: &[Ipv4Addr]) -> EventPayload {
    EventPayload::DnsQuery {
        pid,
        query: query.to_owned(),
        addresses: addresses.iter().copied().map(IpAddr::V4).collect(),
    }
}

/// A process record that started at `start` and is still running.
pub fn running_process(pid: u32, start: Timestamp) -> ProcessRecord {
    ProcessRecord {
        id: ProcessInstanceId::from_pid_and_creation(pid, start),
        pid,
        parent_pid: 4,
        parent_id: None,
        image_path: format!(r"C:\test\proc{pid}.exe"),
        command_line: None,
        started: start,
        ended: None,
        exit_code: None,
    }
}

pub fn with_severity(mut event: EventEnvelope, severity: Severity) -> EventEnvelope {
    event.severity = Some(severity);
    event
}

pub fn system_metrics(cpu_percent: f32) -> SystemMetrics {
    SystemMetrics {
        cpu_percent,
        memory_used_bytes: 8_000_000_000,
        memory_total_bytes: 32_000_000_000,
        disk_read_bytes: 1000,
        disk_write_bytes: 2000,
        net_received_bytes: 3000,
        net_sent_bytes: 400,
    }
}

pub fn process_metric(pid: u32, instance: Option<ProcessInstanceId>, cpu_percent: f32) -> ProcessMetric {
    ProcessMetric { pid, instance, cpu_percent, memory_bytes: 50_000_000 }
}

/// A metrics sample at `at` seconds with the given heaviest processes and a long-tail remainder.
pub fn metrics_sample(
    at: i64,
    system_cpu: f32,
    processes: Vec<ProcessMetric>,
    others_cpu: f32,
    others_count: u32,
) -> MetricsSample {
    MetricsSample {
        ts: at_secs(at),
        system: system_metrics(system_cpu),
        processes,
        others: OtherProcesses { process_count: others_count, cpu_percent: others_cpu, memory_bytes: 1_000_000_000 },
    }
}

pub const BEACON_TARGET: Ipv4Addr = Ipv4Addr::new(203, 0, 113, 9);

pub const EXPLORER_PID: u32 = 1000;
pub const DROPPER_PID: u32 = 2000;
pub const POWERSHELL_PID: u32 = 3000;

/// The headline scenario from the product vision: an unfamiliar executable launches PowerShell,
/// which beacons to one address every 30 seconds. Afterwards the PID of the PowerShell process
/// is reused by an unrelated process that also connects out.
pub fn beacon_scenario() -> Vec<EventEnvelope> {
    let mut f = EventFactory::default();
    let mut events = vec![
        f.make(at_secs(1), process_started(EXPLORER_PID, 4, 1, r"C:\Windows\explorer.exe")),
        f.make(at_secs(2), process_started(DROPPER_PID, EXPLORER_PID, 2, r"C:\Users\me\AppData\Local\Temp\update.exe")),
        f.make(at_secs(3), process_started(POWERSHELL_PID, DROPPER_PID, 3, r"C:\Windows\System32\powershell.exe")),
    ];
    for beat in 0..3 {
        let at = 4 + beat * 30;
        events.push(f.make(at_secs(at), net_activity(POWERSHELL_PID, NetAction::Connect, BEACON_TARGET, 0)));
        events.push(f.make(at_secs(at + 1), net_activity(POWERSHELL_PID, NetAction::Send, BEACON_TARGET, 200)));
    }
    events.push(f.make(at_secs(100), process_exited(POWERSHELL_PID, 3)));
    // PID reuse: same PID, different process (new creation time), later.
    events.push(
        f.make(at_secs(200), process_started(POWERSHELL_PID, EXPLORER_PID, 200, r"C:\Program Files\App\app.exe")),
    );
    events.push(
        f.make(at_secs(210), net_activity(POWERSHELL_PID, NetAction::Connect, Ipv4Addr::new(198, 51, 100, 7), 0)),
    );
    events
}
