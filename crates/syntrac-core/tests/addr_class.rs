//! Which kind of address a destination is. A property of the address alone, so it is computed when shown and never stored.

use std::net::IpAddr;

use syntrac_core::addr_class::{classify_addr, AddrClass};

fn class(text: &str) -> AddrClass {
    classify_addr(text.parse::<IpAddr>().unwrap())
}

#[test]
fn ordinary_internet_addresses_are_public() {
    assert_eq!(class("8.8.8.8"), AddrClass::Public);
    assert_eq!(class("142.251.40.238"), AddrClass::Public);
    assert_eq!(class("2607:f8b0:4006:816::200e"), AddrClass::Public);
    // Documentation ranges are not special here: they are what the tests use for "the internet".
    assert_eq!(class("203.0.113.10"), AddrClass::Public);
}

#[test]
fn the_private_ranges_stop_exactly_at_their_edges() {
    assert_eq!(class("10.0.0.1"), AddrClass::Private);
    assert_eq!(class("10.255.255.255"), AddrClass::Private);
    assert_eq!(class("11.0.0.1"), AddrClass::Public);

    assert_eq!(class("172.15.255.255"), AddrClass::Public);
    assert_eq!(class("172.16.0.0"), AddrClass::Private);
    assert_eq!(class("172.31.255.255"), AddrClass::Private);
    assert_eq!(class("172.32.0.0"), AddrClass::Public);

    assert_eq!(class("192.168.1.20"), AddrClass::Private);
    assert_eq!(class("192.167.255.255"), AddrClass::Public);
    assert_eq!(class("192.169.0.0"), AddrClass::Public);
}

#[test]
fn shared_carrier_address_space_is_private_not_internet() {
    // 100.64.0.0/10 is what Tailscale and carrier-grade NAT use: not a public server.
    assert_eq!(class("100.64.0.1"), AddrClass::Private);
    assert_eq!(class("100.127.255.255"), AddrClass::Private);
    assert_eq!(class("100.63.255.255"), AddrClass::Public);
    assert_eq!(class("100.128.0.0"), AddrClass::Public);
}

#[test]
fn ipv6_unique_local_addresses_are_private() {
    assert_eq!(class("fc00::1"), AddrClass::Private);
    assert_eq!(class("fdff:ffff::1"), AddrClass::Private);
    assert_eq!(class("fe00::1"), AddrClass::Other, "fe00::/9 is neither unique-local nor link-local");
}

#[test]
fn link_local_addresses_are_their_own_class() {
    assert_eq!(class("169.254.1.1"), AddrClass::LinkLocal);
    assert_eq!(class("169.253.255.255"), AddrClass::Public);
    assert_eq!(class("fe80::1"), AddrClass::LinkLocal);
    assert_eq!(class("febf::1"), AddrClass::LinkLocal);
    assert_eq!(class("fec0::1"), AddrClass::Other);
}

#[test]
fn multicast_is_its_own_class() {
    assert_eq!(class("224.0.0.251"), AddrClass::Multicast, "mDNS");
    assert_eq!(class("239.255.255.250"), AddrClass::Multicast, "SSDP");
    assert_eq!(class("223.255.255.255"), AddrClass::Public);
    assert_eq!(class("ff02::fb"), AddrClass::Multicast);
}

#[test]
fn special_purpose_addresses_are_other() {
    assert_eq!(class("0.0.0.0"), AddrClass::Other);
    assert_eq!(class("255.255.255.255"), AddrClass::Other, "broadcast");
    assert_eq!(class("240.0.0.1"), AddrClass::Other, "reserved");
    assert_eq!(class("127.0.0.1"), AddrClass::Other, "loopback is never a destination worth ranking");
    assert_eq!(class("::1"), AddrClass::Other);
    assert_eq!(class("::"), AddrClass::Other);
}

#[test]
fn an_ipv4_mapped_ipv6_address_is_classified_as_the_ipv4_address() {
    assert_eq!(class("::ffff:192.168.1.5"), AddrClass::Private);
    assert_eq!(class("::ffff:8.8.8.8"), AddrClass::Public);
}

#[test]
fn only_public_addresses_are_the_internet() {
    assert!(AddrClass::Public.is_internet());
    for other in [AddrClass::Private, AddrClass::LinkLocal, AddrClass::Multicast, AddrClass::Other] {
        assert!(!other.is_internet());
    }
}
