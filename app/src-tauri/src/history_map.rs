//! The connection map view (docs/PHASE5-PLAN.md, A2): which programs talked to which networks over a range, as the History screen draws
//! it. The grouping and the cut are `syntrac_core::connection_map`; this adds the names the screen shows.
//! Same conventions as `history.rs`: camelCase on the wire, times in milliseconds since the Unix epoch.

use serde::Serialize;
use syntrac_core::connection_map::{
    build_map, ConnectionMap, MapLimits, NetworkKind, NetworkNode, ProgramKind, ProgramNode,
};
use syntrac_core::storage_guard::StorageStats;
use syntrac_core::{AddressScope, DestinationTrendRepository, OwnerRepository, RepoError, RollupRepository, Timestamp};

use crate::history::{clamp_history_secs, ms, range_start};
use crate::history_network::{names_summary, owner_data, OwnerDataView, OwnerView};
use crate::history_programs::UNKNOWN_PROGRAM;
use crate::names::display_name;

/// The most programs, and the most networks, one map draws (the rest are folded into one node each).
pub const MAX_MAP_NODES: usize = 30;
const DEFAULT_MAP_NODES: usize = 12;
const NO_OWNER_LABEL: &str = "No owner found";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MapProgramView {
    /// `program`, `unidentified` or `others`.
    pub kind: &'static str,
    pub name: String,
    /// The program's image path, which opens its details; none for the other two kinds.
    pub path: Option<String>,
    /// How many programs the node stands for.
    pub programs: u32,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MapAddressView {
    pub addr: String,
    /// The name asked for most recently, when any was recorded.
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MapNetworkView {
    /// `owner`, `no_owner` or `others`.
    pub kind: &'static str,
    pub label: String,
    pub owner: Option<OwnerView>,
    /// How many networks the node stands for.
    pub networks: u32,
    pub addresses: u32,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    /// The biggest addresses of the network, which open their details.
    pub top: Vec<MapAddressView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MapLinkView {
    /// Indexes into `programs` and `networks`.
    pub program: usize,
    pub network: usize,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionMapView {
    pub from_ms: i64,
    pub to_ms: i64,
    pub history_starts_ms: Option<i64>,
    pub programs: Vec<MapProgramView>,
    pub networks: Vec<MapNetworkView>,
    pub links: Vec<MapLinkView>,
    /// How many programs and networks there were before the cut.
    pub total_programs: u32,
    pub total_networks: u32,
    pub owner_data: Option<OwnerDataView>,
}

fn plural(count: u32, singular: &str) -> String {
    if count == 1 {
        format!("1 other {singular}")
    } else {
        format!("{count} other {singular}s")
    }
}

fn program_view(node: ProgramNode) -> MapProgramView {
    let (kind, name, path) = match node.kind {
        ProgramKind::Program(exe) => ("program", display_name(&exe.path, 0), Some(exe.path)),
        ProgramKind::Unidentified => ("unidentified", UNKNOWN_PROGRAM.to_owned(), None),
        ProgramKind::Others => ("others", plural(node.programs, "program"), None),
    };
    MapProgramView {
        kind,
        name,
        path,
        programs: node.programs,
        bytes_sent: node.bytes_sent,
        bytes_received: node.bytes_received,
    }
}

fn network_view<R: RollupRepository>(repo: &R, node: NetworkNode) -> Result<MapNetworkView, RepoError> {
    let (kind, label, owner) = match node.kind {
        NetworkKind::Owner(owner) => {
            ("owner", owner.org.clone(), Some(OwnerView { asn: owner.asn, org: owner.org, country: owner.country }))
        }
        NetworkKind::NoOwner => ("no_owner", NO_OWNER_LABEL.to_owned(), None),
        NetworkKind::Others => ("others", plural(node.networks, "network"), None),
    };
    let mut top = Vec::with_capacity(node.top.len());
    for addr in node.top {
        top.push(MapAddressView { addr: addr.to_string(), name: names_summary(repo, addr)?.0 });
    }
    Ok(MapNetworkView {
        kind,
        label,
        owner,
        networks: node.networks,
        addresses: node.addresses,
        bytes_sent: node.bytes_sent,
        bytes_received: node.bytes_received,
        top,
    })
}

fn map_view<R: RollupRepository + StorageStats + OwnerRepository>(
    repo: &R,
    map: ConnectionMap,
    from: Timestamp,
    to: Timestamp,
) -> Result<ConnectionMapView, RepoError> {
    let mut networks = Vec::with_capacity(map.networks.len());
    for node in map.networks {
        networks.push(network_view(repo, node)?);
    }
    Ok(ConnectionMapView {
        from_ms: ms(from),
        to_ms: ms(to),
        history_starts_ms: StorageStats::usage(repo)?.oldest_rollup.map(ms),
        programs: map.programs.into_iter().map(program_view).collect(),
        networks,
        links: map
            .links
            .into_iter()
            .map(|link| MapLinkView {
                program: link.program,
                network: link.network,
                bytes_sent: link.bytes_sent,
                bytes_received: link.bytes_received,
            })
            .collect(),
        total_programs: map.total_programs,
        total_networks: map.total_networks,
        owner_data: owner_data(repo)?,
    })
}

/// The map of the traffic over the last `range_secs`. `limits` is how many programs and networks to draw (each kept within
/// `1..=MAX_MAP_NODES`); the rest are folded into one node each.
pub fn connection_map<R>(
    repo: &R,
    now: Timestamp,
    range_secs: u64,
    scope: AddressScope,
    limits: Option<(usize, usize)>,
) -> Result<ConnectionMapView, RepoError>
where
    R: RollupRepository + DestinationTrendRepository + OwnerRepository + StorageStats,
{
    let from = syntrac_core::rollup::hour_start(range_start(now, clamp_history_secs(range_secs)));
    let (programs, networks) = limits.unwrap_or((DEFAULT_MAP_NODES, DEFAULT_MAP_NODES));
    let limits = MapLimits { programs: programs.clamp(1, MAX_MAP_NODES), networks: networks.clamp(1, MAX_MAP_NODES) };
    let map = build_map(repo.traffic_pairs(from, now)?, scope, limits, |addr| repo.lookup_owner(addr))?;
    map_view(repo, map, from, now)
}
