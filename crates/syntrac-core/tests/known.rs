//! Known programs: building one from what the service measured, and judging other programs against the list. All pure.

use syntrac_core::known::{
    clean_label, clean_note, new_known_product, KnownError, KnownIndex, KnownProduct, LookalikeReason, Standing,
};
use syntrac_core::{
    Executable, FileFacts, IdentityFailure, IdentityRecord, InspectErrorKind, Signature, SignatureStatus, Timestamp,
    VersionInfo,
};

const SERV: &str = r"C:\Program Files (x86)\Acme\acmeserv.exe";
const TRANSMIT: &str = r"C:\Program Files (x86)\Acme\x64\acmetransmit64.exe";
const ACME_ROOT: &str = r"c:\program files (x86)\acme";

fn facts(status: SignatureStatus, signer: Option<&str>, product: Option<&str>) -> FileFacts {
    FileFacts {
        signature: Signature { status, signer: signer.map(str::to_owned), ..Signature::default() },
        version: VersionInfo { product: product.map(str::to_owned), ..VersionInfo::default() },
        ..FileFacts::default()
    }
}

fn acme_facts() -> FileFacts {
    facts(SignatureStatus::Valid, Some("ACME SOFTWARE, INC."), Some("Acme Backup"))
}

fn record(path: &str, facts: Option<FileFacts>) -> IdentityRecord {
    IdentityRecord {
        exe: Executable::from_path(path).unwrap(),
        facts,
        inspected_at: Some(Timestamp::from_micros(1)),
        last_attempt: Timestamp::from_micros(1),
        failure: None,
    }
}

fn known_acme() -> KnownProduct {
    KnownProduct {
        id: 1,
        label: "Acme".into(),
        publisher: Some("acme software, inc.".into()),
        root: ACME_ROOT.into(),
        product_name: Some("Acme Backup".into()),
        note: None,
        created_at: Timestamp::from_micros(5),
    }
}

/// The list knows Acme, and the machine has its two programs.
fn index() -> KnownIndex {
    let bb = acme_facts();
    KnownIndex::build(vec![known_acme()], [(SERV, Some(&bb)), (TRANSMIT, Some(&bb))])
}

#[test]
fn a_label_is_trimmed_and_must_be_one_to_eighty_printable_characters() {
    assert_eq!(clean_label("  Acme  ").unwrap(), "Acme");
    assert!(matches!(clean_label("   "), Err(KnownError::Invalid(_))));
    assert!(matches!(clean_label(&"x".repeat(81)), Err(KnownError::Invalid(_))));
    assert!(clean_label(&"x".repeat(80)).is_ok());
    assert!(matches!(clean_label("Back\u{0}blaze"), Err(KnownError::Invalid(_))), "control characters are refused");
    assert!(matches!(clean_label("two\nlines"), Err(KnownError::Invalid(_))));
}

#[test]
fn a_note_is_optional_trimmed_and_capped() {
    assert_eq!(clean_note(None).unwrap(), None);
    assert_eq!(clean_note(Some("   ")).unwrap(), None, "a blank note is no note");
    assert_eq!(clean_note(Some(" my backup ")).unwrap().as_deref(), Some("my backup"));
    assert!(clean_note(Some(&"x".repeat(500))).is_ok());
    assert!(matches!(clean_note(Some(&"x".repeat(501))), Err(KnownError::Invalid(_))));
    assert!(matches!(clean_note(Some("bad\u{7}note")), Err(KnownError::Invalid(_))));
}

#[test]
fn a_known_product_takes_its_publisher_and_folder_from_what_the_service_measured() {
    let measured = record(SERV, Some(acme_facts()));

    let product = new_known_product(SERV, Some(&measured), "  Acme ", Some("my cloud backup")).unwrap();

    assert_eq!(product.label, "Acme");
    assert_eq!(product.publisher.as_deref(), Some("acme software, inc."));
    assert_eq!(product.root, ACME_ROOT);
    assert_eq!(product.product_name.as_deref(), Some("Acme Backup"));
    assert_eq!(product.note.as_deref(), Some("my cloud backup"));
}

#[test]
fn a_program_that_has_not_been_read_cannot_be_marked() {
    assert_eq!(new_known_product(SERV, None, "x", None), Err(KnownError::NotInspected));

    let mut unreadable = record(SERV, None);
    unreadable.failure =
        Some(IdentityFailure { kind: InspectErrorKind::NotFound, message: "gone".into(), attempts: 1 });
    assert_eq!(new_known_product(SERV, Some(&unreadable), "x", None), Err(KnownError::Unreadable));
}

#[test]
fn a_file_changed_since_it_was_signed_cannot_be_marked() {
    let altered = record(SERV, Some(facts(SignatureStatus::Altered, Some("ACME SOFTWARE, INC."), None)));

    assert_eq!(new_known_product(SERV, Some(&altered), "x", None), Err(KnownError::Altered));
}

#[test]
fn an_unsigned_or_untrusted_program_may_be_marked_because_the_user_is_the_judge() {
    let unsigned = record(SERV, Some(facts(SignatureStatus::Unsigned, None, None)));
    let untrusted = record(SERV, Some(facts(SignatureStatus::Untrusted, Some("Contoso Editor"), None)));

    assert!(new_known_product(SERV, Some(&unsigned), "x", None).is_ok());
    let product = new_known_product(SERV, Some(&untrusted), "x", None).unwrap();
    assert_eq!(product.publisher.as_deref(), Some("contoso editor"));
}

#[test]
fn bad_labels_are_refused_before_anything_else_is_looked_at() {
    let measured = record(SERV, Some(acme_facts()));

    assert!(matches!(new_known_product(SERV, Some(&measured), "", None), Err(KnownError::Invalid(_))));
}

#[test]
fn every_program_of_a_known_product_is_known() {
    let bb = acme_facts();

    for path in [SERV, TRANSMIT, r"C:\Program Files (x86)\Acme\new_helper.exe"] {
        assert_eq!(index().standing(path, Some(&bb)), Standing::Known { id: 1, label: "Acme".into() }, "{path}");
    }
}

#[test]
fn a_program_nobody_has_read_yet_is_not_judged() {
    assert_eq!(index().standing(SERV, None), Standing::Unlisted);
}

#[test]
fn an_unrelated_program_is_unlisted() {
    let chrome = facts(SignatureStatus::Valid, Some("Google LLC"), Some("Google Chrome"));

    assert_eq!(index().standing(r"C:\Program Files\Google\Chrome\chrome.exe", Some(&chrome)), Standing::Unlisted);
}

#[test]
fn an_unsigned_program_with_the_file_name_of_a_known_one_in_another_folder_looks_like_it() {
    let rogue = facts(SignatureStatus::Unsigned, None, None);

    let standing = index().standing(r"C:\Users\alice\AppData\Local\Temp\acmeserv.exe", Some(&rogue));

    assert_eq!(
        standing,
        Standing::Lookalike {
            id: 1,
            label: "Acme".into(),
            reasons: vec![LookalikeReason::SameFileName("acmeserv.exe".into())]
        }
    );
}

#[test]
fn the_file_name_is_compared_without_regard_to_case() {
    let rogue = facts(SignatureStatus::Unsigned, None, None);

    assert!(matches!(
        index().standing(r"D:\x\ACMESERV.EXE", Some(&rogue)),
        Standing::Lookalike { reasons, .. } if reasons == vec![LookalikeReason::SameFileName("acmeserv.exe".into())]
    ));
}

#[test]
fn a_valid_signature_by_someone_else_on_a_common_file_name_is_not_flagged() {
    let other = facts(SignatureStatus::Valid, Some("Some Other Company"), Some("Other Product"));

    assert_eq!(index().standing(r"C:\Program Files\Other\acmeserv.exe", Some(&other)), Standing::Unlisted);
}

#[test]
fn a_program_that_claims_the_product_name_but_is_signed_by_someone_else_looks_like_it() {
    let imposter = facts(SignatureStatus::Valid, Some("Evil Corp"), Some("acme backup"));

    let standing = index().standing(r"C:\Program Files\Evil\thing.exe", Some(&imposter));

    assert_eq!(
        standing,
        Standing::Lookalike {
            id: 1,
            label: "Acme".into(),
            reasons: vec![LookalikeReason::SameProductName("Acme Backup".into())]
        }
    );
}

#[test]
fn a_different_signer_spelling_inside_the_same_protected_folder_is_not_flagged() {
    // Windows files are signed as "Microsoft Windows" or "Microsoft Windows Publisher" inside the same folder.
    let windows = KnownProduct {
        id: 2,
        label: "Windows".into(),
        publisher: Some("microsoft windows".into()),
        root: r"c:\windows\system32".into(),
        product_name: Some("Microsoft Windows Operating System".into()),
        note: None,
        created_at: Timestamp::from_micros(5),
    };
    let known = KnownIndex::build(vec![windows], []);
    let sibling =
        facts(SignatureStatus::Valid, Some("Microsoft Windows Publisher"), Some("Microsoft Windows Operating System"));

    assert_eq!(known.standing(r"C:\Windows\System32\svchost.exe", Some(&sibling)), Standing::Unlisted);

    let unsigned_sibling = facts(SignatureStatus::Unsigned, None, Some("Microsoft Windows Operating System"));
    assert!(
        matches!(known.standing(r"C:\Windows\System32\fake.exe", Some(&unsigned_sibling)), Standing::Lookalike { .. }),
        "an unsigned file claiming to be Windows is still flagged, even in System32"
    );
}

#[test]
fn both_reasons_are_reported_together() {
    let rogue = facts(SignatureStatus::Unsigned, None, Some("Acme Backup"));

    let standing = index().standing(r"C:\Temp\acmeserv.exe", Some(&rogue));

    assert!(matches!(
        standing,
        Standing::Lookalike { reasons, .. } if reasons.len() == 2
    ));
}

#[test]
fn an_empty_list_judges_nothing() {
    let nothing = KnownIndex::build(vec![], []);
    let any = facts(SignatureStatus::Unsigned, None, None);

    assert!(nothing.is_empty());
    assert_eq!(nothing.standing(SERV, Some(&any)), Standing::Unlisted);
}

#[test]
fn a_known_unsigned_product_covers_the_unsigned_files_of_its_folder_only() {
    let git = KnownProduct {
        id: 3,
        label: "Git tools".into(),
        publisher: None,
        root: r"c:\program files\git".into(),
        product_name: None,
        note: None,
        created_at: Timestamp::from_micros(5),
    };
    let known = KnownIndex::build(vec![git], []);
    let unsigned = facts(SignatureStatus::Unsigned, None, None);
    let signed = facts(SignatureStatus::Valid, Some("Someone"), None);

    assert!(matches!(known.standing(r"C:\Program Files\Git\usr\bin\ls.exe", Some(&unsigned)), Standing::Known { .. }));
    assert_eq!(known.standing(r"C:\Program Files\Git\usr\bin\signed.exe", Some(&signed)), Standing::Unlisted);
}

fn windows_known() -> KnownIndex {
    let windows = KnownProduct {
        id: 2,
        label: "Windows".into(),
        publisher: Some("microsoft windows".into()),
        root: r"c:\windows\system32".into(),
        product_name: Some("Microsoft Windows Operating System".into()),
        note: None,
        created_at: Timestamp::from_micros(5),
    };
    KnownIndex::build(vec![windows], [])
}

#[test]
fn the_same_maker_under_another_signer_name_in_another_folder_is_not_a_lookalike() {
    // Real Windows files carry the product name everywhere (SysWOW64, SystemApps, Defender under ProgramData) and are signed as
    // "Microsoft Windows", "Microsoft Windows Publisher" or "Microsoft Corporation".
    for signer in ["Microsoft Windows Publisher", "Microsoft Corporation", "MICROSOFT WINDOWS"] {
        let genuine = facts(SignatureStatus::Valid, Some(signer), Some("Microsoft Windows Operating System"));

        for path in [r"C:\Windows\SysWOW64\cmd.exe", r"C:\ProgramData\Microsoft\Windows Defender\x\MsMpEng.exe"] {
            assert_eq!(windows_known().standing(path, Some(&genuine)), Standing::Unlisted, "{signer} {path}");
        }
    }
}

#[test]
fn a_different_maker_claiming_the_product_name_is_still_flagged_even_with_a_valid_signature() {
    let claimant = facts(SignatureStatus::Valid, Some("Evil Corp"), Some("Microsoft Windows Operating System"));

    assert!(matches!(
        windows_known().standing(r"C:\Windows\SysWOW64\evil.exe", Some(&claimant)),
        Standing::Lookalike { .. }
    ));
}

#[test]
fn a_file_that_is_not_validly_signed_claiming_the_name_is_flagged_even_from_the_same_maker() {
    let untrusted =
        facts(SignatureStatus::Untrusted, Some("Microsoft Windows"), Some("Microsoft Windows Operating System"));
    let altered =
        facts(SignatureStatus::Altered, Some("Microsoft Windows"), Some("Microsoft Windows Operating System"));

    for suspect in [untrusted, altered] {
        assert!(matches!(
            windows_known().standing(r"C:\Windows\SysWOW64\x.exe", Some(&suspect)),
            Standing::Lookalike { .. }
        ));
    }
}

#[test]
fn the_makers_name_ignores_case_punctuation_and_a_leading_the() {
    let known = KnownIndex::build(
        vec![KnownProduct {
            id: 9,
            label: "Darktable".into(),
            publisher: Some("the darktable team".into()),
            root: r"c:\program files\darktable".into(),
            product_name: Some("darktable".into()),
            note: None,
            created_at: Timestamp::from_micros(5),
        }],
        [],
    );
    let other_build = facts(SignatureStatus::Valid, Some("Darktable, Inc."), Some("darktable"));

    assert_eq!(known.standing(r"D:\Apps\darktable\x.exe", Some(&other_build)), Standing::Unlisted);
}

#[test]
fn a_product_with_no_publisher_gives_no_maker_to_compare_so_a_valid_claimant_is_flagged() {
    let git = KnownProduct {
        id: 3,
        label: "Git tools".into(),
        publisher: None,
        root: r"c:\program files\git".into(),
        product_name: Some("Git".into()),
        note: None,
        created_at: Timestamp::from_micros(5),
    };
    let known = KnownIndex::build(vec![git], []);
    let claimant = facts(SignatureStatus::Valid, Some("Someone Else"), Some("Git"));

    assert!(matches!(known.standing(r"C:\Temp\git.exe", Some(&claimant)), Standing::Lookalike { .. }));
}
