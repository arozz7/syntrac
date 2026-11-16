//! The IP-to-owner dataset: parsing the downloaded file and looking an address up. Pure: text in, rows out.

use std::net::IpAddr;

use syntrac_core::owner_dataset::{addr_key, lookup_in, parse_ip2asn, OwnerRange};

fn addr(text: &str) -> IpAddr {
    text.parse().unwrap()
}

const SAMPLE: &str = "\
1.0.0.0\t1.0.0.255\t13335\tUS\tCLOUDFLARENET - Cloudflare, Inc.
1.0.1.0\t1.0.3.255\t0\tNone\tNot routed
1.0.4.0\t1.0.7.255\t56203\tAU\tGTELECOM-AUSTRALIA Gtelecom-AUSTRALIA
2001:4860::\t2001:4860:ffff:ffff:ffff:ffff:ffff:ffff\t15169\tUS\tGOOGLE
";

fn owners(ranges: &[OwnerRange], text: &str) -> Option<(u32, String, String)> {
    lookup_in(ranges, addr(text)).map(|r| (r.asn, r.country.clone(), r.org.clone()))
}

// ---- parsing

#[test]
fn rows_are_read_with_their_owner_and_unrouted_ranges_are_counted_not_kept() {
    let parsed = parse_ip2asn(SAMPLE);

    assert_eq!(parsed.ranges.len(), 3);
    assert_eq!((parsed.unrouted, parsed.bad_lines), (1, 0));
    assert_eq!(
        owners(&parsed.ranges, "1.0.0.7"),
        Some((13335, "US".to_owned(), "CLOUDFLARENET - Cloudflare, Inc.".to_owned()))
    );
}

#[test]
fn a_header_row_comments_blank_lines_and_spaces_instead_of_tabs_are_accepted() {
    let text = "# comment\nrange_start range_end AS_number country_code AS_description\n\n1.0.0.0 1.0.0.255 13335 US Cloudflare, Inc.\n";

    let parsed = parse_ip2asn(text);

    assert_eq!(parsed.ranges.len(), 1);
    assert_eq!(parsed.ranges[0].org, "Cloudflare, Inc.");
    assert_eq!(parsed.bad_lines, 1, "the header row is not a data row and is counted, not trusted");
}

#[test]
fn a_malformed_line_is_counted_and_never_stops_the_rest() {
    let text = "garbage\n1.0.0.0\t1.0.0.255\tnot-a-number\tUS\tX\n1.0.0.0\t1.0.0.255\t1\tUS\tFine\n1.0.2.0\t1.0.1.0\t2\tUS\tBackwards\n9.9.9.9\t2001::1\t3\tUS\tMixed families\n";

    let parsed = parse_ip2asn(text);

    assert_eq!(parsed.ranges.len(), 1);
    assert_eq!(parsed.ranges[0].asn, 1);
    assert_eq!(parsed.bad_lines, 4);
}

#[test]
fn an_overlapping_range_is_skipped_and_counted_so_a_lookup_has_one_answer() {
    let text =
        "1.0.0.0\t1.0.0.255\t1\tUS\tFirst\n1.0.0.128\t1.0.1.255\t2\tUS\tOverlaps\n1.0.2.0\t1.0.2.255\t3\tUS\tAfter\n";

    let parsed = parse_ip2asn(text);

    assert_eq!(parsed.ranges.iter().map(|r| r.asn).collect::<Vec<_>>(), [1, 3]);
    assert_eq!(parsed.bad_lines, 1);
}

#[test]
fn ranges_come_out_sorted_whatever_order_the_file_has() {
    let text = "5.0.0.0\t5.0.0.255\t5\tUS\tB\n1.0.0.0\t1.0.0.255\t1\tUS\tA\n";

    let parsed = parse_ip2asn(text);

    assert_eq!(parsed.ranges.iter().map(|r| r.asn).collect::<Vec<_>>(), [1, 5]);
}

#[test]
fn text_from_the_file_is_cleaned_and_capped() {
    let long = "N".repeat(1000);
    let text = format!("1.0.0.0\t1.0.0.255\t7\tUS\tEvil\u{202E}Corp\u{0007}\n1.0.1.0\t1.0.1.255\t8\tXX\t{long}\n");

    let parsed = parse_ip2asn(&text);

    assert_eq!(parsed.ranges[0].org, "EvilCorp");
    assert_eq!(parsed.ranges[1].org.chars().count(), 120);
}

#[test]
fn an_empty_or_nonsense_file_has_no_ranges() {
    assert!(parse_ip2asn("").ranges.is_empty());
    let parsed = parse_ip2asn("<html>not the data</html>\n");
    assert!(parsed.ranges.is_empty());
    assert_eq!(parsed.bad_lines, 1);
}

// ---- looking an address up

#[test]
fn an_address_is_found_at_the_first_and_last_address_of_its_range_and_not_outside() {
    let ranges = parse_ip2asn(SAMPLE).ranges;

    assert!(owners(&ranges, "1.0.0.0").is_some());
    assert!(owners(&ranges, "1.0.0.255").is_some());
    assert_eq!(owners(&ranges, "0.255.255.255"), None, "just before the first range");
    assert_eq!(owners(&ranges, "1.0.1.0"), None, "in the unrouted gap");
    assert_eq!(owners(&ranges, "1.0.8.0"), None, "after the last IPv4 range");
}

#[test]
fn ipv6_is_looked_up_and_an_ipv4_mapped_address_is_the_ipv4_address() {
    let ranges = parse_ip2asn(SAMPLE).ranges;

    assert_eq!(owners(&ranges, "2001:4860:4860::8888").map(|o| o.0), Some(15169));
    assert_eq!(owners(&ranges, "::ffff:1.0.0.7").map(|o| o.0), Some(13335));
    assert_eq!(owners(&ranges, "2001:4861::1"), None);
}

#[test]
fn an_empty_dataset_knows_no_owner() {
    assert_eq!(lookup_in(&[], addr("1.1.1.1")), None);
}

#[test]
fn the_key_of_an_address_orders_like_the_address_and_maps_ipv4_into_ipv6() {
    assert_eq!(addr_key(addr("1.2.3.4")), addr_key(addr("::ffff:1.2.3.4")));
    assert!(addr_key(addr("1.2.3.4")) < addr_key(addr("1.2.3.5")));
    assert!(addr_key(addr("255.255.255.255")) < addr_key(addr("2001::")), "IPv4 space sorts below real IPv6 space");
}
