//! What raw values from Windows MEAN when reading a file: trust results, text from the file, the mark of the web, paths we
//! refuse to open. All pure; the calls that fetch the raw values are tested on real files in `file_inspector.rs`.

use syntrac_collector::file_parse::{
    parse_zone_id, precheck_path, sanitize, thumbprint_hex, translation_pairs, trust_outcome, version_value_path,
    TrustOutcome,
};
use syntrac_core::InspectErrorKind;

const TRUST_E_NOSIGNATURE: i32 = 0x800B_0100_u32 as i32;
const TRUST_E_SUBJECT_FORM_UNKNOWN: i32 = 0x800B_0003_u32 as i32;
const TRUST_E_PROVIDER_UNKNOWN: i32 = 0x800B_0001_u32 as i32;
const TRUST_E_BAD_DIGEST: i32 = 0x8009_6010_u32 as i32;
const CERT_E_UNTRUSTEDROOT: i32 = 0x800B_0109_u32 as i32;
const CERT_E_EXPIRED: i32 = 0x800B_0101_u32 as i32;
const CERT_E_CHAINING: i32 = 0x800B_010A_u32 as i32;
const CERT_E_REVOKED: i32 = 0x800B_010C_u32 as i32;
const TRUST_E_EXPLICIT_DISTRUST: i32 = 0x800B_0111_u32 as i32;
const ACCESS_DENIED: i32 = 0x8007_0005_u32 as i32;

#[test]
fn zero_is_a_valid_signature() {
    assert_eq!(trust_outcome(0), TrustOutcome::Valid);
}

#[test]
fn a_file_with_no_signature_or_of_a_kind_without_signatures_is_unsigned() {
    for code in [TRUST_E_NOSIGNATURE, TRUST_E_SUBJECT_FORM_UNKNOWN, TRUST_E_PROVIDER_UNKNOWN] {
        assert_eq!(trust_outcome(code), TrustOutcome::NoSignature, "{code:#x}");
    }
}

#[test]
fn a_signature_that_no_longer_matches_the_file_means_it_was_altered() {
    assert_eq!(trust_outcome(TRUST_E_BAD_DIGEST), TrustOutcome::Altered);
}

#[test]
fn a_signed_file_whose_certificate_cannot_be_trusted_is_untrusted() {
    for code in [CERT_E_UNTRUSTEDROOT, CERT_E_EXPIRED, CERT_E_CHAINING, CERT_E_REVOKED, TRUST_E_EXPLICIT_DISTRUST] {
        assert_eq!(trust_outcome(code), TrustOutcome::Untrusted, "{code:#x}");
    }
}

#[test]
fn an_unrelated_failure_is_a_failure_not_a_verdict_on_the_file() {
    assert_eq!(trust_outcome(ACCESS_DENIED), TrustOutcome::Failed(ACCESS_DENIED));
}

#[test]
fn text_from_a_file_loses_control_characters_and_is_capped() {
    assert_eq!(sanitize("  Acme Software, Inc.  ").as_deref(), Some("Acme Software, Inc."));
    assert_eq!(sanitize("Evil\u{0}\u{7}\r\nName\t").as_deref(), Some("EvilName"), "control characters are removed");
    assert_eq!(sanitize("").as_deref(), None);
    assert_eq!(sanitize(" \u{0} ").as_deref(), None, "nothing printable left");
    assert_eq!(sanitize(&"x".repeat(1000)).unwrap().chars().count(), 256);
    assert_eq!(sanitize(&"é".repeat(300)).unwrap().chars().count(), 256, "the cap counts characters, not bytes");
}

#[test]
fn the_thumbprint_is_upper_case_hex() {
    assert_eq!(thumbprint_hex(&[0x0a, 0xff, 0x00, 0x7c]), "0AFF007C");
    assert_eq!(thumbprint_hex(&[]), "");
}

#[test]
fn the_mark_of_the_web_is_the_zone_id() {
    assert_eq!(parse_zone_id("[ZoneTransfer]\r\nZoneId=3\r\n"), Some(3));
    assert_eq!(parse_zone_id("[ZoneTransfer]\nZoneId=4\nReferrerUrl=https://example.test/\n"), Some(4));
    assert_eq!(parse_zone_id("[ZoneTransfer]\r\nzoneid = 3 \r\n"), Some(3), "case and spaces do not matter");
    assert_eq!(parse_zone_id("[ZoneTransfer]\r\n"), None);
    assert_eq!(parse_zone_id("ZoneId=abc"), None);
    assert_eq!(parse_zone_id("ZoneId=99999999999999"), None, "not a zone");
    assert_eq!(parse_zone_id(""), None);
}

#[test]
fn only_absolute_local_paths_are_opened() {
    assert!(precheck_path(r"C:\Program Files\Acme\a.exe").is_ok());
    assert!(precheck_path(r"\\?\C:\Program Files\Acme\a.exe").is_ok());
    let refused_paths = [
        "vmmemWSL",
        "",
        "  ",
        r"..\a.exe",
        r"a.exe",
        r"\\server\share\a.exe",
        r"\\?\UNC\server\share\a.exe",
        r"\Device\HarddiskVolume3\a.exe",
        "C:a.exe",
    ];
    for refused in refused_paths {
        let error = precheck_path(refused).expect_err(refused);
        assert_eq!(error.kind, InspectErrorKind::NotAFile, "{refused}");
    }
}

#[test]
fn version_values_are_found_by_language_and_code_page() {
    assert_eq!(version_value_path(0x0409, 0x04B0, "CompanyName"), r"\StringFileInfo\040904B0\CompanyName");
    assert_eq!(version_value_path(0x0000, 0x04E4, "ProductName"), r"\StringFileInfo\000004E4\ProductName");
}

#[test]
fn the_translation_table_is_pairs_of_language_and_code_page() {
    let bytes = [0x09, 0x04, 0xB0, 0x04, 0x07, 0x04, 0xE4, 0x04, 0xFF];
    assert_eq!(
        translation_pairs(&bytes),
        vec![(0x0409, 0x04B0), (0x0407, 0x04E4)],
        "a trailing partial pair is ignored"
    );
    assert!(translation_pairs(&[]).is_empty());
}
