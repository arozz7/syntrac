//! The normalized event envelope. Every telemetry source maps into this shape.

use std::net::IpAddr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::identity::ProcessInstanceId;
use crate::time::Timestamp;

/// Detection tier. An anomaly is not a threat, so the tiers stay distinct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Deviates from the learned baseline.
    Unusual,
    /// Matches a detection rule.
    Suspicious,
    /// Matches threat intelligence.
    Confirmed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventCategory {
    Process,
    Network,
    Dns,
}

impl EventCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Process => "process",
            Self::Network => "network",
            Self::Dns => "dns",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetProtocol {
    Tcp,
    Udp,
}

/// What a raw network event reported. Byte counts apply to `Send` and `Receive` only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetAction {
    /// A connection attempt was made.
    Connect,
    /// A connection was established or accepted.
    Accept,
    Send,
    Receive,
    Close,
    /// A connection attempt failed.
    Fail,
}

/// What happened. Sources report raw PIDs; the correlator resolves them to instances.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventPayload {
    /// A process was created while monitoring.
    ProcessStarted {
        pid: u32,
        parent_pid: u32,
        /// Real creation time reported by the OS (not the event time).
        created: Timestamp,
        image_path: String,
        command_line: Option<String>,
    },
    ProcessExited {
        pid: u32,
        created: Timestamp,
        exit_code: Option<u32>,
    },
    /// A process that was already running when monitoring began (startup snapshot).
    /// Distinct from `ProcessStarted` so restarts do not flood the timeline or trip "new process" rules.
    ProcessObserved {
        pid: u32,
        parent_pid: u32,
        created: Timestamp,
        image_path: String,
        command_line: Option<String>,
    },
    /// Raw, per-packet-ish network activity. Aggregated into `NetworkFlow`; never persisted.
    NetworkActivity {
        pid: u32,
        protocol: NetProtocol,
        action: NetAction,
        remote_addr: IpAddr,
        remote_port: u16,
        bytes: u32,
    },
    /// Network activity of one process instance with one remote endpoint over one time window.
    NetworkFlow {
        pid: u32,
        protocol: NetProtocol,
        remote_addr: IpAddr,
        remote_port: u16,
        bytes_sent: u64,
        bytes_received: u64,
        opened: u32,
        closed: u32,
        failed: u32,
        window_start: Timestamp,
        window_end: Timestamp,
    },
    /// A DNS lookup. `addresses` is empty when the lookup returned nothing.
    DnsQuery {
        pid: u32,
        query: String,
        addresses: Vec<IpAddr>,
    },
}

impl EventPayload {
    pub fn category(&self) -> EventCategory {
        match self {
            Self::ProcessStarted { .. } | Self::ProcessExited { .. } | Self::ProcessObserved { .. } => {
                EventCategory::Process
            }
            Self::NetworkActivity { .. } | Self::NetworkFlow { .. } => EventCategory::Network,
            Self::DnsQuery { .. } => EventCategory::Dns,
        }
    }

    /// Stable string matching the serialized `kind` tag.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::ProcessStarted { .. } => "process_started",
            Self::ProcessExited { .. } => "process_exited",
            Self::ProcessObserved { .. } => "process_observed",
            Self::NetworkActivity { .. } => "network_activity",
            Self::NetworkFlow { .. } => "network_flow",
            Self::DnsQuery { .. } => "dns_query",
        }
    }

    /// The PID the source attributed this event to.
    pub fn pid(&self) -> u32 {
        match self {
            Self::ProcessStarted { pid, .. }
            | Self::ProcessExited { pid, .. }
            | Self::ProcessObserved { pid, .. }
            | Self::NetworkActivity { pid, .. }
            | Self::NetworkFlow { pid, .. }
            | Self::DnsQuery { pid, .. } => *pid,
        }
    }

    /// Raw network activity is an intermediate form; only its aggregated flows are stored.
    pub fn is_persisted(&self) -> bool {
        !matches!(self, Self::NetworkActivity { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub id: Uuid,
    pub ts: Timestamp,
    /// Filled in by the correlator; sources leave this empty.
    pub process_instance_id: Option<ProcessInstanceId>,
    pub severity: Option<Severity>,
    pub payload: EventPayload,
}

impl EventEnvelope {
    /// An uncorrelated event as produced by a telemetry source.
    pub fn from_source(ts: Timestamp, payload: EventPayload) -> Self {
        Self { id: Uuid::new_v4(), ts, process_instance_id: None, severity: None, payload }
    }
}
