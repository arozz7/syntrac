//! "Top owners": the destinations of a range grouped by the network that owns them (docs/ALERTS-PLAN.md, N1).
//!
//! Pure: it takes the sums the store already produced for the range (`DestinationTotals`) and a way to look an address up, so the
//! in-memory and SQLite stores cannot disagree and nothing here touches the network or a database.
//!
//! An owner is the organisation that runs a range of addresses, often a cloud or hosting company, not necessarily the service that
//! was reached. Addresses with no owner (no dataset, outside every range, or not on the internet) form ONE group with `owner: None`;
//! it is shown as "no owner found", never guessed.

use std::collections::BTreeMap;
use std::net::IpAddr;

use crate::addr_class::classify_addr;
use crate::destination_trend::{AddressScope, DestinationTotals};
use crate::owner_dataset::Owner;

/// How many of an owner's destinations a row keeps, biggest first, so the row can say where without a second read.
pub const DESTINATIONS_PER_OWNER: usize = 5;

/// One owner (or the group with none), summed over the destinations in a range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerGroup {
    pub owner: Option<Owner>,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub flows: u64,
    /// How many different destination addresses.
    pub destinations: u32,
    /// The biggest destinations by bytes (at most `DESTINATIONS_PER_OWNER`), ties by address.
    pub top: Vec<DestinationTotals>,
}

impl OwnerGroup {
    pub fn bytes(&self) -> u64 {
        self.bytes_sent.saturating_add(self.bytes_received)
    }
}

/// Groups `rows` by owner, biggest owner (by bytes sent plus received) first, and keeps the first `limit`. Only addresses inside
/// `scope` are counted, and only addresses on the internet are looked up; an owner is identified by its AS number. A destination
/// with no bytes at all in the range is left out. `lookup` may fail (a store read); the first failure is returned.
pub fn group_by_owner<E>(
    rows: Vec<DestinationTotals>,
    scope: AddressScope,
    limit: usize,
    mut lookup: impl FnMut(IpAddr) -> Result<Option<Owner>, E>,
) -> Result<Vec<OwnerGroup>, E> {
    // Keyed by AS number; 0 stands for "no owner" (a real AS number is never 0, and the dataset drops unrouted ranges).
    let mut groups: BTreeMap<u32, OwnerGroup> = BTreeMap::new();
    for row in rows {
        if !scope.includes(row.addr) || row.bytes() == 0 {
            continue;
        }
        let owner = if classify_addr(row.addr).is_internet() { lookup(row.addr)? } else { None };
        let key = owner.as_ref().map_or(0, |owner| owner.asn);
        let group = groups.entry(key).or_insert_with(|| OwnerGroup {
            owner,
            bytes_sent: 0,
            bytes_received: 0,
            flows: 0,
            destinations: 0,
            top: Vec::new(),
        });
        group.bytes_sent = group.bytes_sent.saturating_add(row.bytes_sent);
        group.bytes_received = group.bytes_received.saturating_add(row.bytes_received);
        group.flows = group.flows.saturating_add(row.flows);
        group.destinations += 1;
        group.top.push(row);
    }
    let mut groups: Vec<OwnerGroup> = groups.into_values().collect();
    for group in &mut groups {
        group.top.sort_by(|a, b| b.bytes().cmp(&a.bytes()).then_with(|| a.addr.cmp(&b.addr)));
        group.top.truncate(DESTINATIONS_PER_OWNER);
    }
    groups.sort_by(|a, b| {
        b.bytes().cmp(&a.bytes()).then_with(|| a.owner.as_ref().map(|o| o.asn).cmp(&b.owner.as_ref().map(|o| o.asn)))
    });
    groups.truncate(limit);
    Ok(groups)
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::net::Ipv4Addr;

    use super::*;

    fn totals(last: u8, sent: u64, received: u64, flows: u64) -> DestinationTotals {
        DestinationTotals {
            addr: IpAddr::V4(Ipv4Addr::new(203, 0, 113, last)),
            bytes_sent: sent,
            bytes_received: received,
            flows,
            programs: 1,
        }
    }

    fn owner(asn: u32, org: &str) -> Owner {
        Owner { asn, country: "US".into(), org: org.into() }
    }

    /// 203.0.113.1 to .9 belong to AS 100, .10 to .19 to AS 200, the rest to no one.
    fn lookup(addr: IpAddr) -> Result<Option<Owner>, Infallible> {
        let IpAddr::V4(v4) = addr else { return Ok(None) };
        Ok(match v4.octets()[3] {
            1..=9 => Some(owner(100, "ALPHA")),
            10..=19 => Some(owner(200, "BETA")),
            _ => None,
        })
    }

    fn group(rows: Vec<DestinationTotals>, limit: usize) -> Vec<OwnerGroup> {
        group_by_owner(rows, AddressScope::Internet, limit, lookup).unwrap()
    }

    fn asns(groups: &[OwnerGroup]) -> Vec<Option<u32>> {
        groups.iter().map(|g| g.owner.as_ref().map(|o| o.asn)).collect()
    }

    #[test]
    fn destinations_of_one_owner_are_added_up_and_counted() {
        let groups = group(vec![totals(1, 10, 90, 2), totals(2, 5, 5, 1), totals(11, 1, 1, 1)], 10);

        let alpha = groups.iter().find(|g| g.owner.as_ref().map(|o| o.asn) == Some(100)).unwrap();
        assert_eq!((alpha.bytes_sent, alpha.bytes_received, alpha.flows, alpha.destinations), (15, 95, 3, 2));
        assert_eq!(alpha.owner.as_ref().unwrap().org, "ALPHA");
    }

    #[test]
    fn owners_are_ranked_by_bytes_with_ties_broken_by_as_number() {
        let groups =
            group(vec![totals(11, 50, 50, 1), totals(1, 100, 0, 1), totals(2, 1, 1, 1), totals(30, 7, 3, 1)], 10);

        assert_eq!(asns(&groups), vec![Some(100), Some(200), None]);

        let tie = group(vec![totals(11, 5, 5, 1), totals(1, 5, 5, 1)], 10);
        assert_eq!(asns(&tie), vec![Some(100), Some(200)], "equal bytes: the lower AS number first");
    }

    #[test]
    fn addresses_with_no_owner_form_one_group_and_are_never_guessed() {
        let groups = group(vec![totals(30, 4, 4, 1), totals(31, 6, 6, 1)], 10);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].owner, None);
        assert_eq!(groups[0].destinations, 2);
    }

    #[test]
    fn an_owner_keeps_only_its_biggest_destinations_biggest_first() {
        let rows: Vec<DestinationTotals> = (1..=8).map(|n| totals(n, u64::from(n), 0, 1)).collect();

        let groups = group(rows, 10);

        assert_eq!(groups[0].destinations, 8, "the count is of all of them");
        let kept: Vec<u8> = groups[0].top.iter().map(|d| d.bytes_sent as u8).collect();
        assert_eq!(kept, vec![8, 7, 6, 5, 4]);
    }

    #[test]
    fn the_limit_keeps_the_biggest_owners() {
        let groups = group(vec![totals(1, 100, 0, 1), totals(11, 50, 0, 1), totals(30, 1, 0, 1)], 2);

        assert_eq!(asns(&groups), vec![Some(100), Some(200)]);
    }

    #[test]
    fn addresses_outside_the_scope_and_without_bytes_are_left_out_and_never_looked_up() {
        let router = DestinationTotals {
            addr: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
            bytes_sent: 1000,
            bytes_received: 1000,
            flows: 9,
            programs: 1,
        };
        let mut asked: Vec<IpAddr> = Vec::new();
        let groups = group_by_owner(vec![router.clone(), totals(1, 0, 0, 3)], AddressScope::Internet, 10, |addr| {
            asked.push(addr);
            Ok::<_, Infallible>(None)
        })
        .unwrap();
        assert!(groups.is_empty(), "this network is out of scope, a destination with no bytes is not counted");
        assert!(asked.is_empty());

        let all = group_by_owner(vec![router], AddressScope::All, 10, |addr| {
            asked.push(addr);
            Ok::<_, Infallible>(Some(owner(1, "WRONG")))
        })
        .unwrap();
        assert_eq!(all[0].owner, None, "an address on this network has no owner even when a dataset claims one");
        assert!(asked.is_empty(), "and is not even looked up");
    }

    #[test]
    fn a_failed_lookup_is_returned_not_hidden() {
        let result =
            group_by_owner(vec![totals(1, 1, 1, 1)], AddressScope::Internet, 10, |_| Err::<Option<Owner>, _>("broken"));

        assert_eq!(result, Err("broken"));
    }
}
