//! The "new thing" rules (docs/ALERTS-PLAN.md, F2): what is raised, what is not, and the words that say why. All pure.

use syntrac_core::findings::{Candidate, Tier};
use syntrac_core::new_things::{new_thing_candidates, ChangeFacts, IdentityChange, NewThingsFacts, MAX_NEW_PER_RULE};
use syntrac_core::{
    AccountKind, AccountSeen, Executable, ExecutableContext, FileFacts, IdentityEntry, IdentityRecord, KnownProduct,
    LauncherKind, LauncherSeen, LedgerEntry, Signature, SignatureStatus, Timestamp, VersionInfo,
};

const DAY: i64 = 86_400 * 1_000_000;

fn at(day: i64) -> Timestamp {
    Timestamp::from_micros(day * DAY)
}

/// Detection was switched on at day 10.
const SINCE: i64 = 10;

fn facts(status: SignatureStatus, signer: Option<&str>, product: Option<&str>) -> FileFacts {
    FileFacts {
        signature: Signature { status, signer: signer.map(str::to_owned), ..Signature::default() },
        version: VersionInfo { product: product.map(str::to_owned), ..VersionInfo::default() },
        sha256: Some("a".repeat(64)),
        ..FileFacts::default()
    }
}

fn entry(path: &str, first_seen_day: Option<i64>, facts: Option<FileFacts>) -> IdentityEntry {
    let exe = Executable::from_path(path).unwrap();
    IdentityEntry {
        exe: exe.clone(),
        seen: first_seen_day.map(|day| LedgerEntry { first_seen: at(day), last_seen: at(day + 1) }),
        record: facts.map(|facts| IdentityRecord {
            exe,
            facts: Some(facts),
            inspected_at: Some(at(1)),
            last_attempt: at(1),
            failure: None,
        }),
    }
}

fn launcher(kind: LauncherKind, location: &str, first_seen_day: i64) -> LauncherSeen {
    LauncherSeen {
        kind,
        location: location.into(),
        detail: String::new(),
        arguments: "--background".into(),
        first_seen: at(first_seen_day),
        last_seen: at(first_seen_day + 1),
        current: true,
    }
}

fn account(sid: &str, name: &str, kind: AccountKind, first_seen_day: i64) -> AccountSeen {
    AccountSeen {
        sid: sid.into(),
        name: Some(name.into()),
        kind,
        first_seen: at(first_seen_day),
        last_seen: at(first_seen_day + 1),
    }
}

#[derive(Default)]
struct World {
    entries: Vec<IdentityEntry>,
    known: Vec<KnownProduct>,
    launchers: Vec<(Executable, Vec<LauncherSeen>)>,
    contexts: Vec<(Executable, ExecutableContext)>,
    changes: Vec<IdentityChange>,
}

impl World {
    fn run(&self) -> Vec<Candidate> {
        new_thing_candidates(&NewThingsFacts {
            since: at(SINCE),
            now: at(20),
            known: &self.known,
            entries: &self.entries,
            launchers: &self.launchers,
            contexts: &self.contexts,
            changes: &self.changes,
        })
    }

    fn of_kind(&self, kind: &str) -> Vec<Candidate> {
        self.run().into_iter().filter(|c| c.kind == kind).collect()
    }
}

const SIGNED: &str = r"C:\Program Files\Vendor\app\tool.exe";
const DOWNLOADS: &str = r"C:\Users\someone\Downloads\setup.exe";

fn valid(signer: &str) -> FileFacts {
    facts(SignatureStatus::Valid, Some(signer), Some("Vendor Tool"))
}

fn unsigned() -> FileFacts {
    facts(SignatureStatus::Unsigned, None, None)
}

// ---- new program

#[test]
fn a_program_first_seen_after_detection_was_switched_on_is_raised_and_one_seen_before_is_not() {
    let world = World {
        entries: vec![
            entry(SIGNED, Some(SINCE + 2), Some(valid("VENDOR INC"))),
            entry(r"C:\Program Files\Old\old.exe", Some(SINCE - 3), Some(valid("OLD INC"))),
        ],
        ..World::default()
    };

    let found = world.of_kind("new_program");

    assert_eq!(found.len(), 1);
    assert_eq!(found[0].subject, SIGNED.to_lowercase());
    assert_eq!(found[0].dedupe_key, format!("new_program:{}", SIGNED.to_lowercase()));
    assert_eq!(found[0].subject_label, "tool.exe");
}

#[test]
fn a_program_that_was_never_seen_running_is_not_raised_yet() {
    let world = World { entries: vec![entry(SIGNED, None, Some(valid("VENDOR INC")))], ..World::default() };

    assert!(world.of_kind("new_program").is_empty());
}

#[test]
fn a_signed_program_in_program_files_is_unusual_not_suspicious() {
    let world = World { entries: vec![entry(SIGNED, Some(SINCE + 1), Some(valid("VENDOR INC")))], ..World::default() };

    assert_eq!(world.of_kind("new_program")[0].tier, Tier::Unusual);
}

#[test]
fn an_unsigned_program_in_a_user_folder_is_suspicious_and_the_reasons_are_in_the_words_and_the_evidence() {
    let world = World { entries: vec![entry(DOWNLOADS, Some(SINCE + 1), Some(unsigned()))], ..World::default() };

    let found = &world.of_kind("new_program")[0];

    assert_eq!(found.tier, Tier::Suspicious);
    assert!(found.why.contains("not signed"), "{}", found.why);
    assert!(found.why.contains("user folder"), "{}", found.why);
    assert!(found.evidence.iter().any(|line| line.contains(r"C:\Users\someone\Downloads\setup.exe")));
    assert!(found.evidence.iter().any(|line| line.contains("not signed")));
}

#[test]
fn an_unsigned_program_in_program_files_is_only_unusual() {
    let world = World {
        entries: vec![entry(r"C:\Program Files\Small\small.exe", Some(SINCE + 1), Some(unsigned()))],
        ..World::default()
    };

    assert_eq!(world.of_kind("new_program")[0].tier, Tier::Unusual);
}

#[test]
fn a_file_changed_after_signing_is_suspicious_wherever_it_is() {
    let altered = facts(SignatureStatus::Altered, Some("VENDOR INC"), None);
    let world = World { entries: vec![entry(SIGNED, Some(SINCE + 1), Some(altered))], ..World::default() };

    let found = &world.of_kind("new_program")[0];

    assert_eq!(found.tier, Tier::Suspicious);
    assert!(found.why.contains("changed after it was signed"), "{}", found.why);
}

#[test]
fn a_program_whose_file_has_not_been_read_yet_is_unusual_and_says_so() {
    let world = World { entries: vec![entry(DOWNLOADS, Some(SINCE + 1), None)], ..World::default() };

    let found = &world.of_kind("new_program")[0];

    assert_eq!(found.tier, Tier::Unusual);
    assert!(found.evidence.iter().any(|line| line.contains("not been read yet")));
}

#[test]
fn a_known_product_covers_its_new_programs() {
    let known = KnownProduct {
        id: 1,
        label: "Vendor Tool".into(),
        publisher: Some("vendor inc".into()),
        root: r"c:\program files\vendor".into(),
        product_name: Some("Vendor Tool".into()),
        note: None,
        created_at: at(1),
    };
    let world = World {
        entries: vec![entry(SIGNED, Some(SINCE + 1), Some(valid("VENDOR INC")))],
        known: vec![known],
        ..World::default()
    };

    assert!(world.of_kind("new_program").is_empty());
}

#[test]
fn a_new_version_in_a_new_folder_of_a_product_already_here_is_an_update_not_a_new_program() {
    let old = r"C:\Users\someone\AppData\Local\Chat\app-1.0\chat.exe";
    let new = r"C:\Users\someone\AppData\Local\Chat\app-1.1\chat.exe";
    let signed = || facts(SignatureStatus::Valid, Some("CHAT INC"), Some("Chat"));
    let world = World {
        entries: vec![entry(old, Some(SINCE - 5), Some(signed())), entry(new, Some(SINCE + 1), Some(signed()))],
        ..World::default()
    };

    assert!(world.of_kind("new_program").is_empty());
}

#[test]
fn the_same_folder_with_a_different_signer_is_not_an_update() {
    let old = r"C:\Users\someone\AppData\Local\Chat\app-1.0\chat.exe";
    let new = r"C:\Users\someone\AppData\Local\Chat\app-1.1\chat.exe";
    let world = World {
        entries: vec![
            entry(old, Some(SINCE - 5), Some(facts(SignatureStatus::Valid, Some("CHAT INC"), Some("Chat")))),
            entry(new, Some(SINCE + 1), Some(facts(SignatureStatus::Valid, Some("SOMEONE ELSE"), Some("Chat")))),
        ],
        ..World::default()
    };

    assert_eq!(world.of_kind("new_program").len(), 1);
}

#[test]
fn an_unsigned_program_beside_an_unsigned_older_one_is_not_called_an_update() {
    let world = World {
        entries: vec![
            entry(r"C:\Tools\a\old.exe", Some(SINCE - 5), Some(unsigned())),
            entry(r"C:\Tools\a\new.exe", Some(SINCE + 1), Some(unsigned())),
        ],
        ..World::default()
    };

    assert_eq!(
        world.of_kind("new_program").len(),
        1,
        "an unsigned file has no maker to say it is an update of the other"
    );
}

// ---- new start entry

fn world_with_entry(path: &str, facts: FileFacts, kind: LauncherKind, first_seen_day: i64) -> World {
    let exe = Executable::from_path(path).unwrap();
    World {
        entries: vec![entry(path, Some(SINCE - 5), Some(facts))],
        launchers: vec![(exe, vec![launcher(kind, r"\Update task", first_seen_day)])],
        ..World::default()
    }
}

#[test]
fn a_start_entry_first_seen_after_switch_on_is_raised_for_a_program_that_was_already_here() {
    let world = world_with_entry(SIGNED, valid("VENDOR INC"), LauncherKind::Task, SINCE + 1);

    let found = world.of_kind("new_start_entry");

    assert_eq!(found.len(), 1);
    assert_eq!(found[0].tier, Tier::Unusual);
    assert!(found[0].subject.starts_with(&SIGNED.to_lowercase()));
    assert!(found[0].evidence.iter().any(|line| line.contains("--background")));
}

#[test]
fn an_entry_seen_before_switch_on_or_no_longer_current_is_not_raised() {
    let before = world_with_entry(SIGNED, valid("VENDOR INC"), LauncherKind::Task, SINCE - 1);
    assert!(before.of_kind("new_start_entry").is_empty());

    let mut gone = world_with_entry(SIGNED, valid("VENDOR INC"), LauncherKind::Task, SINCE + 1);
    gone.launchers[0].1[0].current = false;
    assert!(gone.of_kind("new_start_entry").is_empty());
}

#[test]
fn an_unsigned_target_in_a_user_folder_makes_the_entry_suspicious() {
    let world = world_with_entry(DOWNLOADS, unsigned(), LauncherKind::RunKey, SINCE + 1);

    let found = &world.of_kind("new_start_entry")[0];

    assert_eq!(found.tier, Tier::Suspicious);
    assert!(found.why.contains("starts with Windows"), "{}", found.why);
}

#[test]
fn an_entry_for_a_brand_new_program_is_one_finding_about_the_program_not_two() {
    let exe = Executable::from_path(DOWNLOADS).unwrap();
    let world = World {
        entries: vec![entry(DOWNLOADS, Some(SINCE + 1), Some(unsigned()))],
        launchers: vec![(exe, vec![launcher(LauncherKind::RunKey, "HKCU Run", SINCE + 1)])],
        ..World::default()
    };

    assert_eq!(world.of_kind("new_program").len(), 1);
    assert!(world.of_kind("new_start_entry").is_empty(), "the new program's finding already says how it starts");
    assert!(world.of_kind("new_program")[0].evidence.iter().any(|line| line.contains("starts with Windows")));
}

// ---- changed file

fn change(old: ChangeFacts, new: ChangeFacts, day: i64) -> IdentityChange {
    IdentityChange { exe: Executable::from_path(SIGNED).unwrap(), old, new, at: at(day) }
}

fn side(sha: &str, signer: Option<&str>, status: SignatureStatus) -> ChangeFacts {
    ChangeFacts { sha256: Some(sha.repeat(64)), signer: signer.map(str::to_owned), status: Some(status) }
}

#[test]
fn a_new_version_by_the_same_signer_is_an_update_and_is_not_raised() {
    let world = World {
        changes: vec![change(
            side("a", Some("VENDOR INC"), SignatureStatus::Valid),
            side("b", Some("VENDOR INC"), SignatureStatus::Valid),
            SINCE + 2,
        )],
        ..World::default()
    };

    assert!(world.of_kind("changed_file").is_empty());
}

#[test]
fn a_changed_signer_is_suspicious_and_the_evidence_shows_both() {
    let world = World {
        changes: vec![change(
            side("a", Some("VENDOR INC"), SignatureStatus::Valid),
            side("b", Some("SOMEONE ELSE"), SignatureStatus::Valid),
            SINCE + 2,
        )],
        ..World::default()
    };

    let found = &world.of_kind("changed_file")[0];

    assert_eq!(found.tier, Tier::Suspicious);
    assert!(found.evidence.iter().any(|line| line.contains("VENDOR INC")));
    assert!(found.evidence.iter().any(|line| line.contains("SOMEONE ELSE")));
    assert!(found.dedupe_key.starts_with("changed_file:"));
    assert!(
        found.dedupe_key.ends_with(&"b".repeat(12)),
        "the key names the new content, so a second change is a second finding"
    );
}

#[test]
fn a_signed_file_that_became_unsigned_or_altered_is_suspicious() {
    for new_status in [SignatureStatus::Unsigned, SignatureStatus::Altered, SignatureStatus::Untrusted] {
        let world = World {
            changes: vec![change(
                side("a", Some("VENDOR INC"), SignatureStatus::Valid),
                side("b", Some("VENDOR INC"), new_status),
                SINCE + 2,
            )],
            ..World::default()
        };
        assert_eq!(world.of_kind("changed_file").len(), 1, "{new_status:?}");
    }
}

#[test]
fn an_unsigned_file_with_new_content_is_an_update_not_a_finding_and_so_is_a_change_before_switch_on() {
    let unsigned_update = World {
        changes: vec![change(
            side("a", None, SignatureStatus::Unsigned),
            side("b", None, SignatureStatus::Unsigned),
            SINCE + 2,
        )],
        ..World::default()
    };
    assert!(unsigned_update.of_kind("changed_file").is_empty());

    let old = World {
        changes: vec![change(
            side("a", Some("VENDOR INC"), SignatureStatus::Valid),
            side("b", Some("SOMEONE ELSE"), SignatureStatus::Valid),
            SINCE - 1,
        )],
        ..World::default()
    };
    assert!(old.of_kind("changed_file").is_empty());
}

// ---- new account

#[test]
fn a_program_that_starts_running_as_another_account_is_raised_with_the_accounts_it_had() {
    let exe = Executable::from_path(SIGNED).unwrap();
    let context = ExecutableContext {
        accounts: vec![
            account("S-1-5-21-1-2-3-1001", r"PC\someone", AccountKind::User, SINCE - 4),
            account("S-1-5-18", r"NT AUTHORITY\SYSTEM", AccountKind::System, SINCE + 2),
        ],
        services: vec![],
    };
    let world = World {
        entries: vec![entry(SIGNED, Some(SINCE - 5), Some(valid("VENDOR INC")))],
        contexts: vec![(exe, context)],
        ..World::default()
    };

    let found = world.of_kind("new_account");

    assert_eq!(found.len(), 1);
    assert_eq!(found[0].tier, Tier::Unusual, "a signed program moving to SYSTEM is unusual, not suspicious");
    assert!(found[0].evidence.iter().any(|line| line.contains("SYSTEM")));
    assert!(found[0].evidence.iter().any(|line| line.contains("someone")), "the account it had before is named");
    assert!(found[0].subject.ends_with("s-1-5-18"));
}

#[test]
fn moving_to_system_without_a_valid_signature_is_suspicious() {
    let exe = Executable::from_path(DOWNLOADS).unwrap();
    let context = ExecutableContext {
        accounts: vec![
            account("S-1-5-21-1-2-3-1001", r"PC\someone", AccountKind::User, SINCE - 4),
            account("S-1-5-18", r"NT AUTHORITY\SYSTEM", AccountKind::System, SINCE + 2),
        ],
        services: vec![],
    };
    let world = World {
        entries: vec![entry(DOWNLOADS, Some(SINCE - 5), Some(unsigned()))],
        contexts: vec![(exe, context)],
        ..World::default()
    };

    assert_eq!(world.of_kind("new_account")[0].tier, Tier::Suspicious);
}

#[test]
fn a_programs_first_account_is_not_a_new_account() {
    let exe = Executable::from_path(SIGNED).unwrap();
    let context = ExecutableContext {
        accounts: vec![account("S-1-5-18", r"NT AUTHORITY\SYSTEM", AccountKind::System, SINCE + 2)],
        services: vec![],
    };
    let world = World {
        entries: vec![entry(SIGNED, Some(SINCE - 5), Some(valid("VENDOR INC")))],
        contexts: vec![(exe, context)],
        ..World::default()
    };

    assert!(world.of_kind("new_account").is_empty(), "it had no account before to differ from");
}

// ---- limits

#[test]
fn a_flood_of_one_kind_is_limited_and_the_most_serious_stay() {
    let mut entries = Vec::new();
    for n in 0..(MAX_NEW_PER_RULE + 20) {
        entries.push(entry(&format!(r"C:\Program Files\Many\app{n}.exe"), Some(SINCE + 1), Some(valid("MANY INC"))));
    }
    entries.push(entry(DOWNLOADS, Some(SINCE + 1), Some(unsigned())));
    let world = World { entries, ..World::default() };

    let found = world.of_kind("new_program");

    assert_eq!(found.len(), MAX_NEW_PER_RULE);
    assert!(found.iter().any(|c| c.tier == Tier::Suspicious), "the suspicious one is kept");
}
