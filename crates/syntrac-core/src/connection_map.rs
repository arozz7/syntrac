//! The connection map (docs/PHASE5-PLAN.md, A2): which programs talked to which networks, and how much. Pure: the per-program traffic sums
//! the store produced and a way to look an address up go in, a bounded graph (programs on one side, owner networks on the other) comes out.
//!
//! Addresses are grouped by the network that owns them, as in `owner_totals`: an owner is the organisation that runs a range of addresses
//! (often a cloud or hosting company), not necessarily the service that was reached. Addresses with no owner form ONE node, never a guess.
//! The graph is cut to the busiest programs and networks; everything beyond the cut is folded into one "others" node per side, so the links
//! always add up to the traffic that was given.

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap};
use std::net::IpAddr;

use crate::addr_class::classify_addr;
use crate::destination_trend::{AddressScope, TrafficPair};
use crate::owner_dataset::Owner;
use crate::rollup::Executable;

/// How many of a network's addresses a node names, so a click can open one.
pub const ADDRESSES_PER_NETWORK: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapLimits {
    /// How many programs are drawn (at least one); the rest are folded into one node.
    pub programs: usize,
    /// How many networks are drawn (at least one); the rest are folded into one node.
    pub networks: usize,
}

impl Default for MapLimits {
    fn default() -> Self {
        Self { programs: 12, networks: 12 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgramKind {
    Program(Executable),
    /// The traffic of processes whose program could not be identified.
    Unidentified,
    /// The programs beyond the limit.
    Others,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkKind {
    Owner(Owner),
    /// Addresses no owner was found for (no dataset, outside every range, or not on the internet).
    NoOwner,
    /// The networks beyond the limit.
    Others,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramNode {
    pub kind: ProgramKind,
    /// How many programs the node stands for: one, or the number folded into `Others`.
    pub programs: u32,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkNode {
    pub kind: NetworkKind,
    /// How many networks the node stands for: one, or the number folded into `Others`.
    pub networks: u32,
    /// How many different addresses.
    pub addresses: u32,
    /// The biggest addresses of the network (at most `ADDRESSES_PER_NETWORK`), biggest first; none for `Others`.
    pub top: Vec<IpAddr>,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

/// Traffic of one program node to one network node. Indexes into `ConnectionMap::programs` and `networks`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapLink {
    pub program: usize,
    pub network: usize,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionMap {
    /// Biggest first; the `Others` node, when there is one, is last.
    pub programs: Vec<ProgramNode>,
    pub networks: Vec<NetworkNode>,
    /// Biggest first.
    pub links: Vec<MapLink>,
    /// How many programs and networks there were before the cut.
    pub total_programs: u32,
    pub total_networks: u32,
}

/// Which program a pair belongs to: its path key, or `None` for the unidentified bucket.
type ProgramKey = Option<String>;
/// Which network: the AS number, or 0 for "no owner" (a real AS number is never 0).
type NetworkKey = u32;

#[derive(Default)]
struct Totals {
    sent: u64,
    received: u64,
}

impl Totals {
    fn add(&mut self, sent: u64, received: u64) {
        self.sent = self.sent.saturating_add(sent);
        self.received = self.received.saturating_add(received);
    }

    fn bytes(&self) -> u64 {
        self.sent.saturating_add(self.received)
    }
}

/// A node's share before the cut: who it is, and what it carried.
struct Candidate<K> {
    key: K,
    totals: Totals,
}

/// The map of `pairs`, cut to `limits`. Only addresses inside `scope` count, and only addresses on the internet are looked up (each at most
/// once). A pair with no bytes is left out. `lookup` may fail (a store read); the first failure is returned.
pub fn build_map<E>(
    pairs: Vec<TrafficPair>,
    scope: AddressScope,
    limits: MapLimits,
    mut lookup: impl FnMut(IpAddr) -> Result<Option<Owner>, E>,
) -> Result<ConnectionMap, E> {
    let mut owners: HashMap<IpAddr, Option<Owner>> = HashMap::new();
    let mut owner_of_asn: BTreeMap<NetworkKey, Owner> = BTreeMap::new();
    let mut paths: BTreeMap<String, Executable> = BTreeMap::new();
    let mut addresses: BTreeMap<NetworkKey, BTreeMap<IpAddr, u64>> = BTreeMap::new();
    let mut edges: BTreeMap<(ProgramKey, NetworkKey), Totals> = BTreeMap::new();

    for pair in pairs {
        let bytes = pair.bytes_sent.saturating_add(pair.bytes_received);
        if !scope.includes(pair.addr) || bytes == 0 {
            continue;
        }
        if let Entry::Vacant(slot) = owners.entry(pair.addr) {
            slot.insert(if classify_addr(pair.addr).is_internet() { lookup(pair.addr)? } else { None });
        }
        let network = match &owners[&pair.addr] {
            Some(owner) => {
                owner_of_asn.entry(owner.asn).or_insert_with(|| owner.clone());
                owner.asn
            }
            None => 0,
        };
        let seen = addresses.entry(network).or_default().entry(pair.addr).or_default();
        *seen = seen.saturating_add(bytes);
        let program = pair.exe.map(|exe| {
            let key = exe.key.clone();
            paths.entry(key.clone()).or_insert(exe);
            key
        });
        edges.entry((program, network)).or_default().add(pair.bytes_sent, pair.bytes_received);
    }

    let mut by_program: BTreeMap<ProgramKey, Totals> = BTreeMap::new();
    let mut by_network: BTreeMap<NetworkKey, Totals> = BTreeMap::new();
    for ((program, network), totals) in &edges {
        by_program.entry(program.clone()).or_default().add(totals.sent, totals.received);
        by_network.entry(*network).or_default().add(totals.sent, totals.received);
    }
    let total_programs = u32::try_from(by_program.len()).unwrap_or(u32::MAX);
    let total_networks = u32::try_from(by_network.len()).unwrap_or(u32::MAX);

    let (kept_programs, other_programs) = rank(by_program, limits.programs);
    let (kept_networks, other_networks) = rank(by_network, limits.networks);

    let mut program_index: HashMap<ProgramKey, usize> = HashMap::new();
    let mut programs: Vec<ProgramNode> = Vec::new();
    for candidate in kept_programs {
        program_index.insert(candidate.key.clone(), programs.len());
        let kind = match candidate.key.as_ref().and_then(|key| paths.get(key)) {
            Some(exe) => ProgramKind::Program(exe.clone()),
            None => ProgramKind::Unidentified,
        };
        programs.push(ProgramNode {
            kind,
            programs: 1,
            bytes_sent: candidate.totals.sent,
            bytes_received: candidate.totals.received,
        });
    }
    let others_program = (!other_programs.is_empty()).then(|| {
        let mut totals = Totals::default();
        for candidate in &other_programs {
            totals.add(candidate.totals.sent, candidate.totals.received);
        }
        programs.push(ProgramNode {
            kind: ProgramKind::Others,
            programs: u32::try_from(other_programs.len()).unwrap_or(u32::MAX),
            bytes_sent: totals.sent,
            bytes_received: totals.received,
        });
        programs.len() - 1
    });

    let mut network_index: HashMap<NetworkKey, usize> = HashMap::new();
    let mut networks: Vec<NetworkNode> = Vec::new();
    for candidate in kept_networks {
        network_index.insert(candidate.key, networks.len());
        let kind = match owner_of_asn.get(&candidate.key) {
            Some(owner) => NetworkKind::Owner(owner.clone()),
            None => NetworkKind::NoOwner,
        };
        networks.push(NetworkNode {
            kind,
            networks: 1,
            addresses: count(addresses.get(&candidate.key).map_or(0, BTreeMap::len)),
            top: addresses.get(&candidate.key).map_or_else(Vec::new, biggest),
            bytes_sent: candidate.totals.sent,
            bytes_received: candidate.totals.received,
        });
    }
    let others_network = (!other_networks.is_empty()).then(|| {
        let mut totals = Totals::default();
        let mut address_count = 0usize;
        for candidate in &other_networks {
            totals.add(candidate.totals.sent, candidate.totals.received);
            address_count += addresses.get(&candidate.key).map_or(0, BTreeMap::len);
        }
        networks.push(NetworkNode {
            kind: NetworkKind::Others,
            networks: count(other_networks.len()),
            addresses: count(address_count),
            top: Vec::new(),
            bytes_sent: totals.sent,
            bytes_received: totals.received,
        });
        networks.len() - 1
    });

    let mut merged: BTreeMap<(usize, usize), Totals> = BTreeMap::new();
    for ((program, network), totals) in edges {
        let from = program_index.get(&program).copied().or(others_program);
        let to = network_index.get(&network).copied().or(others_network);
        if let (Some(from), Some(to)) = (from, to) {
            merged.entry((from, to)).or_default().add(totals.sent, totals.received);
        }
    }
    let mut links: Vec<MapLink> = merged
        .into_iter()
        .map(|((program, network), totals)| MapLink {
            program,
            network,
            bytes_sent: totals.sent,
            bytes_received: totals.received,
        })
        .collect();
    links.sort_by(|a, b| {
        (b.bytes_sent.saturating_add(b.bytes_received))
            .cmp(&a.bytes_sent.saturating_add(a.bytes_received))
            .then(a.program.cmp(&b.program))
            .then(a.network.cmp(&b.network))
    });

    Ok(ConnectionMap { programs, networks, links, total_programs, total_networks })
}

/// The biggest addresses by bytes, ties by address.
fn biggest(bytes: &BTreeMap<IpAddr, u64>) -> Vec<IpAddr> {
    let mut ranked: Vec<(&IpAddr, &u64)> = bytes.iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    ranked.into_iter().take(ADDRESSES_PER_NETWORK).map(|(addr, _)| *addr).collect()
}

fn count(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}

/// The `limit` biggest (at least one; ties by key) and the rest. Whatever is not kept is folded into an "others" node by the caller.
fn rank<K: Ord>(totals: BTreeMap<K, Totals>, limit: usize) -> (Vec<Candidate<K>>, Vec<Candidate<K>>) {
    let mut ranked: Vec<Candidate<K>> = totals.into_iter().map(|(key, totals)| Candidate { key, totals }).collect();
    ranked.sort_by(|a, b| b.totals.bytes().cmp(&a.totals.bytes()).then_with(|| a.key.cmp(&b.key)));
    let rest = ranked.split_off(limit.max(1).min(ranked.len()));
    (ranked, rest)
}
