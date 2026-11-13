//! Deterministic builders and replayable scenarios for tests.

use std::net::{IpAddr, Ipv4Addr};

use syntrac_core::{EventEnvelope, EventPayload, NetProtocol, ProcessInstanceId, ProcessRecord, Severity, Timestamp};
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
    pub fn make(&mut self, ts: Timestamp, payload: EventPayload) -> EventEnvelope {
        self.next_id += 1;
        EventEnvelope { id: Uuid::from_u128(self.next_id), ts, process_instance_id: None, severity: None, payload }
    }
}

pub fn process_started(pid: u32, parent_pid: u32, image: &str) -> EventPayload {
    EventPayload::ProcessStarted { pid, parent_pid, image_path: image.to_owned(), command_line: None }
}

pub fn process_exited(pid: u32) -> EventPayload {
    EventPayload::ProcessExited { pid, exit_code: Some(0) }
}

pub fn tcp_connect(pid: u32, remote: Ipv4Addr, remote_port: u16) -> EventPayload {
    EventPayload::NetworkConnection {
        pid,
        protocol: NetProtocol::Tcp,
        local_addr: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 5)),
        local_port: 50_000,
        remote_addr: IpAddr::V4(remote),
        remote_port,
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

pub const BEACON_TARGET: Ipv4Addr = Ipv4Addr::new(203, 0, 113, 9);

pub const EXPLORER_PID: u32 = 1000;
pub const DROPPER_PID: u32 = 2000;
pub const POWERSHELL_PID: u32 = 3000;

/// The headline scenario from the product vision: an unfamiliar executable launches
/// PowerShell, which beacons to one address every 30 seconds. Afterwards the PID of
/// the PowerShell process is reused by an unrelated process that also connects out.
pub fn beacon_scenario() -> Vec<EventEnvelope> {
    let mut f = EventFactory::default();
    let mut events = vec![
        f.make(at_secs(1), process_started(EXPLORER_PID, 4, r"C:\Windows\explorer.exe")),
        f.make(at_secs(2), process_started(DROPPER_PID, EXPLORER_PID, r"C:\Users\me\AppData\Local\Temp\update.exe")),
        f.make(at_secs(3), process_started(POWERSHELL_PID, DROPPER_PID, r"C:\Windows\System32\powershell.exe")),
    ];
    for beat in 0..3 {
        events.push(f.make(at_secs(4 + beat * 30), tcp_connect(POWERSHELL_PID, BEACON_TARGET, 443)));
    }
    events.push(f.make(at_secs(100), process_exited(POWERSHELL_PID)));
    // PID reuse: same PID, different process, later.
    events.push(f.make(at_secs(200), process_started(POWERSHELL_PID, EXPLORER_PID, r"C:\Program Files\App\app.exe")));
    events.push(f.make(at_secs(210), tcp_connect(POWERSHELL_PID, Ipv4Addr::new(198, 51, 100, 7), 443)));
    events
}
