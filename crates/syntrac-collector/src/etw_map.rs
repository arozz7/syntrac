//! Pure mapping from raw ETW field values to SYNTRAC events.
//!
//! The thin adapter in `etw.rs` pulls raw fields out of an `EventRecord`; everything that decides
//! what they mean lives here so it can be tested with values captured from real traces.
//!
//! Event semantics come from the provider manifests (`wevtutil gp <provider> /ge /gm`) and the
//! Phase 0 spike (`aiChangeLog/etw-spike-result-kw10.txt`).

use std::net::IpAddr;

use syntrac_core::{EventEnvelope, EventPayload, NetAction, NetProtocol, Timestamp};

use crate::paths::NtPathConverter;

/// Kernel-Process: process start. Fields as reported by ETW.
pub struct ProcessStartFields {
    pub pid: u32,
    pub parent_pid: u32,
    /// Windows FILETIME of process creation.
    pub create_time: u64,
    /// NT device path.
    pub image_name: String,
}

/// Kernel-Process: process stop. Repeats the creation time, so identity needs no lookup.
pub struct ProcessStopFields {
    pub pid: u32,
    pub create_time: u64,
    pub exit_code: Option<u32>,
}

/// Kernel-Network: one data or connection event. Ports are RAW (network byte order read as little-endian).
pub struct NetworkFields {
    pub event_id: u16,
    pub pid: u32,
    pub size: u32,
    pub daddr: IpAddr,
    pub saddr: IpAddr,
    pub dport: u16,
    pub sport: u16,
}

/// DNS-Client event. `caller_pid` is the event header's PID.
pub struct DnsFields {
    pub event_id: u16,
    pub caller_pid: u32,
    pub query_name: String,
    pub results: Option<String>,
}

/// ETW delivers ports with their two bytes swapped relative to host order.
pub fn swap_port(raw: u16) -> u16 {
    raw.swap_bytes()
}

pub fn process_started(ts: Timestamp, fields: ProcessStartFields, paths: &NtPathConverter) -> EventEnvelope {
    EventEnvelope::from_source(
        ts,
        EventPayload::ProcessStarted {
            pid: fields.pid,
            parent_pid: fields.parent_pid,
            created: Timestamp::from_filetime(fields.create_time),
            image_path: paths.convert(&fields.image_name),
            command_line: None,
        },
    )
}

pub fn process_stopped(ts: Timestamp, fields: ProcessStopFields) -> EventEnvelope {
    EventEnvelope::from_source(
        ts,
        EventPayload::ProcessExited {
            pid: fields.pid,
            created: Timestamp::from_filetime(fields.create_time),
            exit_code: fields.exit_code,
        },
    )
}

/// Event ids that matter, for TCP/UDP over IPv4/IPv6. Everything else is ignored on purpose:
/// 14/30 retransmits and 16/32 reconnects add nothing, and 18/34 ("bytes copied on behalf of
/// user") would count the same bytes a second time.
fn classify(event_id: u16) -> Option<(NetProtocol, NetAction)> {
    use NetAction::{Accept, Close, Connect, Fail, Receive, Send};
    use NetProtocol::{Tcp, Udp};
    Some(match event_id {
        10 | 26 => (Tcp, Send),
        11 | 27 => (Tcp, Receive),
        12 | 28 => (Tcp, Connect),
        13 | 29 => (Tcp, Close),
        15 | 31 => (Tcp, Accept),
        17 => (Tcp, Fail),
        42 | 58 => (Udp, Send),
        43 | 59 => (Udp, Receive),
        49 => (Udp, Fail),
        _ => return None,
    })
}

/// Which fields an event must provide to be mapped. Lets the adapter tell a genuine parse failure
/// from an event that simply does not carry the field (failure events hold only an error code).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetworkFieldNeeds {
    /// Addresses and ports.
    pub endpoint: bool,
    /// Byte count.
    pub size: bool,
}

pub fn network_field_needs(event_id: u16) -> NetworkFieldNeeds {
    match classify(event_id) {
        Some((_, NetAction::Send | NetAction::Receive)) => NetworkFieldNeeds { endpoint: true, size: true },
        Some((_, NetAction::Fail)) | None => NetworkFieldNeeds { endpoint: false, size: false },
        Some(_) => NetworkFieldNeeds { endpoint: true, size: false },
    }
}

/// Cheap pre-check so the adapter can skip parsing events that would be ignored anyway.
pub fn is_tracked_network_event(event_id: u16) -> bool {
    classify(event_id).is_some()
}

/// Cheap pre-check, see [`is_tracked_network_event`].
pub fn is_tracked_dns_event(event_id: u16) -> bool {
    event_id == DNS_CALLER_RESPONSE
}

pub fn network_activity(ts: Timestamp, fields: NetworkFields) -> Option<EventEnvelope> {
    let (protocol, action) = classify(fields.event_id)?;

    // TCP events always describe the connection as (local saddr:sport, remote daddr:dport).
    // For a UDP datagram that was RECEIVED, daddr is this machine (or a multicast group) and the
    // remote end is the sender.
    let (remote_addr, remote_port) = if protocol == NetProtocol::Udp && action == NetAction::Receive {
        (fields.saddr, fields.sport)
    } else {
        (fields.daddr, fields.dport)
    };

    // For failure events `size` is an error code, and lifecycle events move no data.
    let bytes = match action {
        NetAction::Send | NetAction::Receive => fields.size,
        _ => 0,
    };

    Some(EventEnvelope::from_source(
        ts,
        EventPayload::NetworkActivity {
            pid: fields.pid,
            protocol,
            action,
            remote_addr: remote_addr.to_canonical(),
            remote_port: swap_port(remote_port),
            bytes,
        },
    ))
}

/// DNS-Client event 3008 is the caller-side response: it carries the requesting process, the name
/// and the answers. The query event (3006) and the service-side events are ignored so one lookup
/// is stored once.
const DNS_CALLER_RESPONSE: u16 = 3008;

pub fn dns_resolution(ts: Timestamp, fields: DnsFields) -> Option<EventEnvelope> {
    if fields.event_id != DNS_CALLER_RESPONSE {
        return None;
    }
    let query = fields.query_name.trim().trim_end_matches('.').to_ascii_lowercase();
    if query.is_empty() {
        return None;
    }
    Some(EventEnvelope::from_source(
        ts,
        EventPayload::DnsQuery {
            pid: fields.caller_pid,
            query,
            addresses: parse_dns_results(fields.results.as_deref().unwrap_or_default()),
        },
    ))
}

/// `QueryResults` is a `;`-separated list that ends with a separator and may hold CNAME entries
/// such as `type: 5 name.example;`. Only IP addresses are kept, canonicalised (IPv4-mapped IPv6
/// becomes IPv4) so they join with flow remote addresses, without duplicates.
fn parse_dns_results(results: &str) -> Vec<IpAddr> {
    let mut addresses: Vec<IpAddr> = Vec::new();
    for entry in results.split(';') {
        if let Ok(address) = entry.trim().parse::<IpAddr>() {
            let address = address.to_canonical();
            if !addresses.contains(&address) {
                addresses.push(address);
            }
        }
    }
    addresses
}
