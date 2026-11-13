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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetProtocol {
    Tcp,
    Udp,
}

/// What happened. Sources report raw PIDs; the correlator resolves them to instances.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventPayload {
    ProcessStarted {
        pid: u32,
        parent_pid: u32,
        image_path: String,
        command_line: Option<String>,
    },
    ProcessExited {
        pid: u32,
        exit_code: Option<u32>,
    },
    NetworkConnection {
        pid: u32,
        protocol: NetProtocol,
        local_addr: IpAddr,
        local_port: u16,
        remote_addr: IpAddr,
        remote_port: u16,
    },
    DnsQuery {
        pid: u32,
        query: String,
    },
}

impl EventPayload {
    pub fn category(&self) -> EventCategory {
        match self {
            Self::ProcessStarted { .. } | Self::ProcessExited { .. } => EventCategory::Process,
            Self::NetworkConnection { .. } => EventCategory::Network,
            Self::DnsQuery { .. } => EventCategory::Dns,
        }
    }

    /// Stable string matching the serialized `kind` tag.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::ProcessStarted { .. } => "process_started",
            Self::ProcessExited { .. } => "process_exited",
            Self::NetworkConnection { .. } => "network_connection",
            Self::DnsQuery { .. } => "dns_query",
        }
    }

    /// The PID the source attributed this event to.
    pub fn pid(&self) -> u32 {
        match self {
            Self::ProcessStarted { pid, .. }
            | Self::ProcessExited { pid, .. }
            | Self::NetworkConnection { pid, .. }
            | Self::DnsQuery { pid, .. } => *pid,
        }
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
