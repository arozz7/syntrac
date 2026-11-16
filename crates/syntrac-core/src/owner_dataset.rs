//! Which network owns an address: the pure part. Parses the downloaded IP-to-ASN file (IPtoASN's "combined" file, public domain,
//! `docs/OWNER-PLAN.md`) into sorted, non-overlapping ranges, and looks an address up in them. No I/O.
//!
//! An owner here is the owner of the NETWORK RANGE (an autonomous system) and the country it is registered in, not where the
//! machine is and not necessarily the service: it is a clue, worded that way on screen.

use std::net::IpAddr;

use crate::clean_text::clean_text;

/// Longest organisation name kept.
pub const MAX_ORG_CHARS: usize = 120;

/// One range of addresses and who owns it. Both ends are inclusive and use `addr_key`, so IPv4 and IPv6 share one ordering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerRange {
    pub start: u128,
    pub end: u128,
    pub asn: u32,
    /// The country the network is registered in (two letters; `ZZ` when the file gave none we could use).
    pub country: String,
    pub org: String,
}

/// What an address resolved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owner {
    pub asn: u32,
    pub country: String,
    pub org: String,
}

impl From<&OwnerRange> for Owner {
    fn from(range: &OwnerRange) -> Self {
        Self { asn: range.asn, country: range.country.clone(), org: range.org.clone() }
    }
}

/// The result of reading a whole file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedDataset {
    /// Sorted by start, never overlapping.
    pub ranges: Vec<OwnerRange>,
    /// Ranges the file marks as not routed (AS 0): known, but they have no owner, so they are not kept.
    pub unrouted: u32,
    /// Lines that could not be used (not a data row, bad address, backwards range, mixed families, overlapping an earlier
    /// range). Counted, never trusted.
    pub bad_lines: u32,
}

/// The 128-bit position of an address. An IPv4 address is placed in the IPv4-mapped IPv6 space, as everywhere else in SYNTRAC, so
/// `::ffff:1.2.3.4` and `1.2.3.4` are the same key.
pub fn addr_key(addr: IpAddr) -> u128 {
    match addr.to_canonical() {
        IpAddr::V4(v4) => u128::from(v4.to_ipv6_mapped()),
        IpAddr::V6(v6) => u128::from(v6),
    }
}

fn parse_addr(text: &str) -> Option<IpAddr> {
    text.parse().ok()
}

/// The first `count` whitespace-separated fields of `line`, and the rest of the line (the description may contain spaces).
fn fields(line: &str, count: usize) -> Option<(Vec<&str>, &str)> {
    let mut found = Vec::with_capacity(count);
    let mut rest = line;
    for _ in 0..count {
        rest = rest.trim_start();
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        if end == 0 {
            return None;
        }
        found.push(&rest[..end]);
        rest = &rest[end..];
    }
    Some((found, rest.trim()))
}

enum Row {
    Range(OwnerRange),
    Unrouted,
}

fn parse_row(line: &str) -> Option<Row> {
    let (head, description) = fields(line, 4)?;
    let (start, end) = (parse_addr(head[0])?, parse_addr(head[1])?);
    if start.is_ipv4() != end.is_ipv4() {
        return None;
    }
    let (start, end) = (addr_key(start), addr_key(end));
    if end < start {
        return None;
    }
    let asn: u32 = head[2].parse().ok()?;
    if asn == 0 {
        return Some(Row::Unrouted);
    }
    let country = match head[3] {
        code if code.len() == 2 && code.chars().all(|c| c.is_ascii_alphabetic()) => code.to_ascii_uppercase(),
        _ => "ZZ".to_owned(),
    };
    let org = clean_text(description, MAX_ORG_CHARS).unwrap_or_default();
    Some(Row::Range(OwnerRange { start, end, asn, country, org }))
}

/// Reads the whole file. Never fails: what cannot be used is counted in `bad_lines`.
pub fn parse_ip2asn(text: &str) -> ParsedDataset {
    let mut parsed = ParsedDataset::default();
    let mut ranges = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match parse_row(line) {
            Some(Row::Range(range)) => ranges.push(range),
            Some(Row::Unrouted) => parsed.unrouted += 1,
            None => parsed.bad_lines += 1,
        }
    }
    ranges.sort_by_key(|range| range.start);
    for range in ranges {
        // The first range wins; one that starts inside it would make a lookup ambiguous.
        match parsed.ranges.last() {
            Some(last) if range.start <= last.end => parsed.bad_lines += 1,
            _ => parsed.ranges.push(range),
        }
    }
    parsed
}

/// The range that contains `addr`, in ranges sorted by start and not overlapping (as `parse_ip2asn` returns them).
pub fn lookup_in(ranges: &[OwnerRange], addr: IpAddr) -> Option<&OwnerRange> {
    let key = addr_key(addr);
    let after = ranges.partition_point(|range| range.start <= key);
    let candidate = ranges.get(after.checked_sub(1)?)?;
    (key <= candidate.end).then_some(candidate)
}

/// The same key as an address, from 16 big-endian bytes (how the database stores it).
pub fn key_bytes(key: u128) -> [u8; 16] {
    key.to_be_bytes()
}

pub fn key_from_bytes(bytes: &[u8]) -> Option<u128> {
    let array: [u8; 16] = bytes.try_into().ok()?;
    Some(u128::from_be_bytes(array))
}
