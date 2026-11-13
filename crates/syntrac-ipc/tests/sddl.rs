//! The pipe's access list is built from text, so the text must be validated: an unchecked SID could inject
//! extra permission entries into the security descriptor string.

use syntrac_ipc::sddl::{build_pipe_sddl, validate_sid};

const USER: &str = "S-1-5-21-2396074648-1873880380-2004252892-1001";

#[test]
fn well_formed_sids_are_accepted() {
    for sid in [USER, "S-1-5-18", "S-1-5-32-544", "S-1-5-21-1-2-3-500"] {
        assert!(validate_sid(sid).is_ok(), "{sid}");
    }
}

#[test]
fn anything_else_is_rejected_including_injection_attempts() {
    for bad in [
        "",
        "S-1-",
        "S-1",
        "x",
        "s-1-5-18",
        "S-1-5-18 ",
        " S-1-5-18",
        "S-1-5-21-1)(A;;GA;;;WD",
        "S-1-5-21-1;GA",
        "S-1-5-18\n",
        "S-1-5-abc",
        "S-2-5-18",
        &format!("S-1-5-{}", "9".repeat(300)),
    ] {
        assert!(validate_sid(bad).is_err(), "{bad:?} must be rejected");
    }
}

#[test]
fn the_access_list_grants_system_administrators_and_each_named_user_and_nobody_else() {
    let sddl = build_pipe_sddl(&[USER]).unwrap();
    assert_eq!(sddl, format!("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;{USER})"));
    assert!(!sddl.contains(";WD)"), "Everyone must never be granted access");
    assert!(!sddl.contains(";AU)"), "Authenticated Users must never be granted access");
    assert!(sddl.starts_with("D:P"), "the list is protected: no inherited entries");
}

#[test]
fn several_users_get_one_entry_each_and_duplicates_are_collapsed() {
    let sddl = build_pipe_sddl(&[USER, "S-1-5-21-1-2-3-1002", USER]).unwrap();
    assert_eq!(sddl.matches("GRGW").count(), 2);
}

#[test]
fn an_invalid_sid_anywhere_fails_the_whole_list() {
    assert!(build_pipe_sddl(&[USER, "S-1-5-21-1)(A;;GA;;;WD"]).is_err());
}

#[test]
fn with_no_users_only_system_and_administrators_get_in() {
    assert_eq!(build_pipe_sddl(&[]).unwrap(), "D:P(A;;GA;;;SY)(A;;GA;;;BA)");
}
