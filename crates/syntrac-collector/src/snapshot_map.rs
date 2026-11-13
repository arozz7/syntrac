//! Maps the startup snapshot (processes and connections already present) to events.
//!
//! Ordering rules that keep the process table consistent:
//! * snapshot events carry the time the snapshot BEGAN, so any exit that happens during or after
//!   enumeration sorts after them and closes the process correctly;
//! * processes are emitted oldest first so a parent is registered before its children;
//! * they are `ProcessObserved`, not `ProcessStarted`, so restarts do not flood the timeline.

use std::net::{IpAddr, Ipv4Addr};

use syntrac_core::{EventEnvelope, EventPayload, NetAction, NetProtocol, Timestamp};

use crate::etw_map::swap_port;

pub const TCP_STATE_ESTABLISHED: u32 = 5;

/// A running process as enumerated by the OS. `create_time` is a FILETIME; `None` when the process
/// cannot be inspected (protected processes), which means we cannot give it a safe identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotProcess {
    pub pid: u32,
    pub parent_pid: u32,
    pub create_time: Option<u64>,
    pub image_path: Option<String>,
}

#[derive(Debug)]
pub struct SnapshotResult {
    pub events: Vec<EventEnvelope>,
    /// Processes that could not be identified and were left out.
    pub skipped: usize,
}

pub fn observed_processes(entries: Vec<SnapshotProcess>, snapshot_began: Timestamp) -> SnapshotResult {
    let (mut known, unknown): (Vec<_>, Vec<_>) = entries.into_iter().partition(|entry| entry.create_time.is_some());
    known.sort_by_key(|entry| (entry.create_time, entry.pid));

    let events = known
        .into_iter()
        .filter_map(|entry| {
            let created = Timestamp::from_filetime(entry.create_time?);
            Some(EventEnvelope::from_source(
                snapshot_began,
                EventPayload::ProcessObserved {
                    pid: entry.pid,
                    parent_pid: entry.parent_pid,
                    created,
                    image_path: entry.image_path.unwrap_or_default(),
                    command_line: None,
                },
            ))
        })
        .collect();

    SnapshotResult { events, skipped: unknown.len() }
}

/// Address column of the OS TCP table: the four address bytes in network order, read as a
/// little-endian DWORD.
pub fn ipv4_from_table_dword(dword: u32) -> Ipv4Addr {
    Ipv4Addr::from(dword.to_le_bytes())
}

/// Port column of the OS TCP table: network order in the low 16 bits; the high bits are padding.
pub fn table_port(dword: u32) -> u16 {
    swap_port((dword & 0xFFFF) as u16)
}

/// An already-open connection, reported as an `Accept` (established) with no byte count.
pub fn established_connection(
    state: u32,
    owner_pid: u32,
    remote_addr: IpAddr,
    remote_port: u16,
    observed_at: Timestamp,
) -> Option<EventEnvelope> {
    if state != TCP_STATE_ESTABLISHED || remote_addr.is_unspecified() {
        return None;
    }
    Some(EventEnvelope::from_source(
        observed_at,
        EventPayload::NetworkActivity {
            pid: owner_pid,
            protocol: NetProtocol::Tcp,
            action: NetAction::Accept,
            remote_addr: remote_addr.to_canonical(),
            remote_port,
            bytes: 0,
        },
    ))
}
