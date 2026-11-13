//! Startup snapshot: what was already running (and connected) when monitoring began.
//!
//! This is a thin Windows adapter. What the raw values mean, and in what order they are emitted,
//! is decided by the tested functions in `snapshot_map`.

use std::mem::size_of;
use std::net::{IpAddr, Ipv6Addr};

use syntrac_core::{Clock, EventEnvelope, SourceError, TelemetrySource};
use tracing::{info, warn};
use windows::core::PWSTR;
use windows::Win32::Foundation::FILETIME;
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCP6TABLE_OWNER_PID, MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_ALL,
};
use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};

use crate::snapshot_map::{
    established_connection, ipv4_from_table_dword, observed_processes, table_port, SnapshotProcess,
};
use crate::system::{filetime_to_u64, OwnedHandle};

const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
const NO_ERROR: u32 = 0;
const TABLE_RETRIES: usize = 4;

/// Emits the startup snapshot once, then nothing.
pub struct SnapshotSource {
    clock: Box<dyn Clock>,
    taken: bool,
}

impl SnapshotSource {
    pub fn new(clock: impl Clock + 'static) -> Self {
        Self { clock: Box::new(clock), taken: false }
    }
}

impl TelemetrySource for SnapshotSource {
    fn name(&self) -> &str {
        "startup-snapshot"
    }

    fn drain(&mut self) -> Result<Vec<EventEnvelope>, SourceError> {
        if self.taken {
            return Ok(Vec::new());
        }
        self.taken = true;

        // Stamp with the time enumeration BEGINS, so exits during or after it sort after the snapshot.
        let began = self.clock.now();
        let result = observed_processes(enumerate_processes(), began);
        let mut events = result.events;
        let processes = events.len();

        let connections = established_tcp_connections(began);
        let connection_count = connections.len();
        events.extend(connections);

        info!(processes, connections = connection_count, skipped = result.skipped, "Startup snapshot taken");
        Ok(events)
    }
}

fn enumerate_processes() -> Vec<SnapshotProcess> {
    // SAFETY: standard Toolhelp enumeration; the snapshot handle is closed by OwnedHandle, and `entry`
    // is a live local with dwSize set as the API requires.
    unsafe {
        let snapshot = match CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            Ok(handle) => OwnedHandle(handle),
            Err(error) => {
                warn!(%error, "Could not list processes for the startup snapshot");
                return Vec::new();
            }
        };

        let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut processes = Vec::new();
        let mut next = Process32FirstW(snapshot.0, &mut entry);
        while next.is_ok() {
            let (create_time, image_path) = inspect(entry.th32ProcessID);
            processes.push(SnapshotProcess {
                pid: entry.th32ProcessID,
                parent_pid: entry.th32ParentProcessID,
                create_time,
                image_path,
            });
            next = Process32NextW(snapshot.0, &mut entry);
        }
        processes
    }
}

/// Creation time (FILETIME) and Win32 image path of a process, or `None`s when it cannot be opened
/// (protected processes). The creation time comes from `GetProcessTimes`, the same clock ETW's
/// `CreateTime` uses, so both paths give the same process identity.
fn inspect(pid: u32) -> (Option<u64>, Option<String>) {
    // SAFETY: out-pointers reference live locals; the process handle is closed by OwnedHandle.
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return (None, None);
        };
        let handle = OwnedHandle(handle);

        let (mut created, mut exited, mut kernel, mut user) =
            (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
        let create_time = GetProcessTimes(handle.0, &mut created, &mut exited, &mut kernel, &mut user)
            .ok()
            .map(|()| filetime_to_u64(created));

        let mut buffer = [0u16; 1024];
        let mut length = buffer.len() as u32;
        let image_path =
            QueryFullProcessImageNameW(handle.0, PROCESS_NAME_WIN32, PWSTR(buffer.as_mut_ptr()), &mut length)
                .ok()
                .map(|()| String::from_utf16_lossy(&buffer[..length as usize]));

        (create_time, image_path)
    }
}

fn established_tcp_connections(observed_at: syntrac_core::Timestamp) -> Vec<EventEnvelope> {
    let mut events = Vec::new();

    if let Some(buffer) = read_tcp_table(AF_INET.0.into()) {
        // SAFETY: the buffer was filled by GetExtendedTcpTable with this table class, is 4-byte aligned
        // (Vec<u32>), and `dwNumEntries` rows follow the header inside the buffer.
        let rows = unsafe {
            let table = &*buffer.as_ptr().cast::<MIB_TCPTABLE_OWNER_PID>();
            std::slice::from_raw_parts(table.table.as_ptr(), table.dwNumEntries as usize)
        };
        for row in rows {
            let remote = IpAddr::V4(ipv4_from_table_dword(row.dwRemoteAddr));
            events.extend(established_connection(
                row.dwState,
                row.dwOwningPid,
                remote,
                table_port(row.dwRemotePort),
                observed_at,
            ));
        }
    }

    if let Some(buffer) = read_tcp_table(AF_INET6.0.into()) {
        // SAFETY: as above, for the IPv6 table class.
        let rows = unsafe {
            let table = &*buffer.as_ptr().cast::<MIB_TCP6TABLE_OWNER_PID>();
            std::slice::from_raw_parts(table.table.as_ptr(), table.dwNumEntries as usize)
        };
        for row in rows {
            let remote = IpAddr::V6(Ipv6Addr::from(row.ucRemoteAddr));
            events.extend(established_connection(
                row.dwState,
                row.dwOwningPid,
                remote,
                table_port(row.dwRemotePort),
                observed_at,
            ));
        }
    }

    events
}

/// Reads the owner-PID TCP table for one address family into a 4-byte-aligned buffer.
fn read_tcp_table(family: u32) -> Option<Vec<u32>> {
    let mut size = 0u32;
    for _ in 0..TABLE_RETRIES {
        // SAFETY: first call passes no buffer to learn the required size; later calls pass a buffer of
        // exactly `size` bytes. The table can grow between calls, which is why this retries.
        let status = unsafe {
            let words = (size as usize).div_ceil(4);
            let mut buffer = vec![0u32; words.max(1)];
            let pointer = if size == 0 { None } else { Some(buffer.as_mut_ptr().cast::<core::ffi::c_void>()) };
            let status = GetExtendedTcpTable(pointer, &mut size, false, family, TCP_TABLE_OWNER_PID_ALL, 0);
            if status == NO_ERROR {
                return Some(buffer);
            }
            status
        };
        if status != ERROR_INSUFFICIENT_BUFFER {
            warn!(status, family, "Could not read the TCP table for the startup snapshot");
            return None;
        }
    }
    warn!(family, "TCP table kept changing size; skipping connections in the startup snapshot");
    None
}
