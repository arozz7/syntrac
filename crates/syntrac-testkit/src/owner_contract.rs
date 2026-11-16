//! Contract suite for the owner dataset: every `OwnerRepository` (in-memory, SQLite, ...) must pass these.

use std::net::IpAddr;

use syntrac_core::{parse_ip2asn, DatasetInfo, OwnerRepository, Timestamp};

pub fn owner_contract<R: OwnerRepository>(make: impl Fn() -> R) {
    a_new_database_has_no_dataset_and_no_owners(make());
    a_stored_dataset_answers_lookups_at_the_edges_of_its_ranges(make());
    a_second_dataset_replaces_the_first_entirely(make());
    an_empty_dataset_is_a_dataset_with_no_owners(make());
    many_ranges_are_stored_and_found(make());
}

fn addr(text: &str) -> IpAddr {
    text.parse().unwrap()
}

fn info(rows: u32, at: i64) -> DatasetInfo {
    DatasetInfo {
        source: "iptoasn.com (test)".to_owned(),
        downloaded_at: Timestamp::from_micros(at),
        rows,
        bad_lines: 2,
        sha256: "ab".repeat(32),
    }
}

const FIRST: &str = "\
1.0.0.0\t1.0.0.255\t13335\tUS\tCLOUDFLARENET - Cloudflare, Inc.
1.0.4.0\t1.0.7.255\t56203\tAU\tGTELECOM
2001:4860::\t2001:4860:ffff:ffff:ffff:ffff:ffff:ffff\t15169\tUS\tGOOGLE
";

fn asn<R: OwnerRepository>(repo: &R, text: &str) -> Option<u32> {
    repo.lookup_owner(addr(text)).unwrap().map(|owner| owner.asn)
}

fn a_new_database_has_no_dataset_and_no_owners<R: OwnerRepository>(repo: R) {
    assert_eq!(repo.dataset_info().unwrap(), None);
    assert_eq!(repo.lookup_owner(addr("1.0.0.1")).unwrap(), None);
}

fn a_stored_dataset_answers_lookups_at_the_edges_of_its_ranges<R: OwnerRepository>(mut repo: R) {
    let ranges = parse_ip2asn(FIRST).ranges;
    repo.replace_dataset(&ranges, &info(3, 1_000)).unwrap();

    assert_eq!(repo.dataset_info().unwrap(), Some(info(3, 1_000)));
    let owner = repo.lookup_owner(addr("1.0.0.7")).unwrap().unwrap();
    assert_eq!(
        (owner.asn, owner.country.as_str(), owner.org.as_str()),
        (13335, "US", "CLOUDFLARENET - Cloudflare, Inc.")
    );
    assert_eq!(asn(&repo, "1.0.0.0"), Some(13335), "first address of a range");
    assert_eq!(asn(&repo, "1.0.0.255"), Some(13335), "last address of a range");
    assert_eq!(asn(&repo, "0.255.255.255"), None, "before the first range");
    assert_eq!(asn(&repo, "1.0.1.0"), None, "in a gap");
    assert_eq!(asn(&repo, "1.0.8.0"), None, "after the last IPv4 range");
    assert_eq!(asn(&repo, "2001:4860:4860::8888"), Some(15169));
    assert_eq!(asn(&repo, "::ffff:1.0.4.1"), Some(56203), "an IPv4-mapped address is the IPv4 address");
    assert_eq!(asn(&repo, "2001:4861::1"), None);
}

fn a_second_dataset_replaces_the_first_entirely<R: OwnerRepository>(mut repo: R) {
    repo.replace_dataset(&parse_ip2asn(FIRST).ranges, &info(3, 1_000)).unwrap();

    let second = "8.8.8.0\t8.8.8.255\t15169\tUS\tGOOGLE\n";
    repo.replace_dataset(&parse_ip2asn(second).ranges, &info(1, 2_000)).unwrap();

    assert_eq!(repo.dataset_info().unwrap(), Some(info(1, 2_000)));
    assert_eq!(asn(&repo, "8.8.8.8"), Some(15169));
    assert_eq!(asn(&repo, "1.0.0.7"), None, "nothing of the old dataset is left");
    assert_eq!(asn(&repo, "2001:4860:4860::8888"), None);
}

fn an_empty_dataset_is_a_dataset_with_no_owners<R: OwnerRepository>(mut repo: R) {
    repo.replace_dataset(&parse_ip2asn(FIRST).ranges, &info(3, 1_000)).unwrap();

    repo.replace_dataset(&[], &info(0, 3_000)).unwrap();

    assert_eq!(repo.dataset_info().unwrap().map(|i| i.rows), Some(0));
    assert_eq!(asn(&repo, "1.0.0.7"), None);
}

fn many_ranges_are_stored_and_found<R: OwnerRepository>(mut repo: R) {
    let mut text = String::new();
    for n in 0..5_000u32 {
        // 10.x.y.0/24 blocks, one owner each.
        text.push_str(&format!(
            "20.{}.{}.0\t20.{}.{}.255\t{}\tUS\tOwner {n}\n",
            n / 256,
            n % 256,
            n / 256,
            n % 256,
            1000 + n
        ));
    }
    let parsed = parse_ip2asn(&text);
    assert_eq!(parsed.ranges.len(), 5_000);
    repo.replace_dataset(&parsed.ranges, &info(5_000, 4_000)).unwrap();

    assert_eq!(asn(&repo, "20.0.0.7"), Some(1000));
    assert_eq!(asn(&repo, "20.4.100.200"), Some(1000 + 4 * 256 + 100));
    assert_eq!(asn(&repo, "20.19.136.0"), None, "just after the last block (n = 4999 is 20.19.135)");
    assert_eq!(asn(&repo, "20.19.135.1"), Some(1000 + 4999));
}
