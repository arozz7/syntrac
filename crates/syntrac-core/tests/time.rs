//! Windows FILETIME conversion must be exact: process identity depends on it.

use syntrac_core::Timestamp;

#[test]
fn filetime_from_the_spike_converts_to_the_expected_unix_microseconds() {
    // CreateTime of pid 5616 (jq.exe) captured in aiChangeLog/etw-spike-result-kw10.txt
    let created = Timestamp::from_filetime(134_390_031_692_131_159);
    assert_eq!(created.as_micros(), 1_794_529_569_213_115);
}

#[test]
fn filetime_of_the_unix_epoch_is_zero() {
    assert_eq!(Timestamp::from_filetime(116_444_736_000_000_000).as_micros(), 0);
}

#[test]
fn filetime_before_the_unix_epoch_is_negative() {
    assert!(Timestamp::from_filetime(116_444_735_000_000_000).as_micros() < 0);
}

#[test]
fn filetime_conversion_never_panics_on_extreme_values() {
    let _ = Timestamp::from_filetime(0);
    let _ = Timestamp::from_filetime(u64::MAX);
}
