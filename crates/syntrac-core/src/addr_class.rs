//! Which kind of address a destination is, so the Network screen can show the internet first and the router, printers and
//! multicast chatter on request. A property of the address alone: computed when shown, never stored.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AddrClass {
    /// An ordinary address on the internet.
    Public,
    /// This site's own network: 10/8, 172.16/12, 192.168/16, shared carrier space 100.64/10, and IPv6 unique-local fc00::/7.
    Private,
    /// Only reachable on the same link: 169.254/16 and fe80::/10.
    LinkLocal,
    /// One-to-many: 224/4 and ff00::/8 (mDNS, SSDP).
    Multicast,
    /// Everything else: unspecified, broadcast, loopback, reserved.
    Other,
}

impl AddrClass {
    /// Whether this is an address on the internet (the default view of the Network screen).
    pub fn is_internet(self) -> bool {
        self == Self::Public
    }
}

fn classify_v4(addr: Ipv4Addr) -> AddrClass {
    let [a, b, _, _] = addr.octets();
    if addr.is_loopback() || addr.is_unspecified() || addr.is_broadcast() {
        AddrClass::Other
    } else if addr.is_multicast() {
        AddrClass::Multicast
    } else if addr.is_link_local() {
        AddrClass::LinkLocal
    } else if addr.is_private() || (a == 100 && (64..=127).contains(&b)) {
        AddrClass::Private
    } else if a == 0 || a >= 240 {
        AddrClass::Other
    } else {
        AddrClass::Public
    }
}

fn classify_v6(addr: Ipv6Addr) -> AddrClass {
    let first = addr.segments()[0];
    if addr.is_loopback() || addr.is_unspecified() {
        AddrClass::Other
    } else if first & 0xff00 == 0xff00 {
        AddrClass::Multicast
    } else if first & 0xffc0 == 0xfe80 {
        AddrClass::LinkLocal
    } else if first & 0xfe00 == 0xfc00 {
        AddrClass::Private
    } else if first & 0xff00 == 0xfe00 || first & 0xffc0 == 0xfec0 {
        // fe00::/9 is reserved and fec0::/10 (site-local) was withdrawn: neither is an internet address.
        AddrClass::Other
    } else {
        AddrClass::Public
    }
}

/// The class of an address. An IPv4-mapped IPv6 address is classified as the IPv4 address it carries.
pub fn classify_addr(addr: IpAddr) -> AddrClass {
    match addr.to_canonical() {
        IpAddr::V4(v4) => classify_v4(v4),
        IpAddr::V6(v6) => classify_v6(v6),
    }
}
