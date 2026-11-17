//! The pattern pack and its engine (docs/ALERTS-PLAN.md, F5a): what the shipped pack is made of, what a pack may say, and for each
//! pattern what matches, what a harmless look-alike does not, and the words that say why. All pure.

use syntrac_core::findings::{Candidate, Tier};
use syntrac_core::findings_run::clean_candidate;
use syntrac_core::pattern_run::{osa_distance, pattern_candidates, PatternFacts, MAX_PER_PATTERN, RECENT_DAYS};
use syntrac_core::patterns::{built_in_pack, parse_pack, PatternPack, BUILT_IN_PACK};
use syntrac_core::spawns::SpawnPair;
use syntrac_core::{
    Executable, FileFacts, IdentityEntry, IdentityRecord, KnownProduct, LauncherKind, LauncherSeen, LedgerEntry,
    Signature, SignatureStatus, Timestamp, VersionInfo,
};

const DAY: i64 = 86_400 * 1_000_000;

fn at(day: i64) -> Timestamp {
    Timestamp::from_micros(day * DAY)
}

/// Detection was switched on at day 100; "now" is day 120.
const SINCE: i64 = 100;
const NOW: i64 = 120;

fn facts(status: SignatureStatus, signer: Option<&str>) -> FileFacts {
    FileFacts {
        signature: Signature { status, signer: signer.map(str::to_owned), ..Signature::default() },
        version: VersionInfo::default(),
        sha256: Some("a".repeat(64)),
        ..FileFacts::default()
    }
}

fn microsoft() -> FileFacts {
    facts(SignatureStatus::Valid, Some("Microsoft Windows"))
}

fn unsigned() -> FileFacts {
    facts(SignatureStatus::Unsigned, None)
}

fn downloaded(mut file: FileFacts) -> FileFacts {
    file.zone_id = Some(3);
    file
}

/// An entry first seen at `first` and last seen at `last` (days).
fn entry_between(path: &str, first: i64, last: i64, file: Option<FileFacts>) -> IdentityEntry {
    let exe = Executable::from_path(path).unwrap();
    IdentityEntry {
        exe: exe.clone(),
        seen: Some(LedgerEntry { first_seen: at(first), last_seen: at(last) }),
        record: file.map(|facts| IdentityRecord {
            exe,
            facts: Some(facts),
            inspected_at: Some(at(1)),
            last_attempt: at(1),
            failure: None,
        }),
    }
}

/// A program that was here before the switch-on and ran yesterday.
fn entry(path: &str, file: Option<FileFacts>) -> IdentityEntry {
    entry_between(path, SINCE - 40, NOW - 1, file)
}

fn run_entry(location: &str, first_seen_day: i64) -> LauncherSeen {
    LauncherSeen {
        kind: LauncherKind::RunKey,
        location: location.into(),
        detail: String::new(),
        arguments: "--minimised".into(),
        first_seen: at(first_seen_day),
        last_seen: at(NOW - 1),
        current: true,
    }
}

#[derive(Default)]
struct World {
    entries: Vec<IdentityEntry>,
    known: Vec<KnownProduct>,
    launchers: Vec<(Executable, Vec<LauncherSeen>)>,
    first_online: Vec<Executable>,
    spawns: Vec<SpawnPair>,
}

impl World {
    fn with(entries: Vec<IdentityEntry>) -> Self {
        Self { entries, ..Self::default() }
    }

    fn run(&self) -> Vec<Candidate> {
        pattern_candidates(
            &built_in_pack(),
            &PatternFacts {
                since: at(SINCE),
                now: at(NOW),
                recent_days: RECENT_DAYS,
                known: &self.known,
                entries: &self.entries,
                launchers: &self.launchers,
                first_online: &self.first_online,
                spawns: &self.spawns,
            },
        )
    }

    fn kinds(&self) -> Vec<String> {
        self.run().into_iter().map(|c| c.kind).collect()
    }

    fn of(&self, id: &str) -> Vec<Candidate> {
        self.run().into_iter().filter(|c| c.kind == format!("pattern_{id}")).collect()
    }
}

// ---- the pack

#[test]
fn the_shipped_pack_loads_and_every_pattern_says_why_what_it_is_called_and_what_could_be_harmless() {
    let pack =
        parse_pack(BUILT_IN_PACK).expect("the shipped pack must load: an unknown condition is refused at build time");

    assert!(!pack.version.is_empty());
    assert!(pack.patterns.len() >= 12);
    let mut ids: Vec<&str> = pack.patterns.iter().map(|p| p.id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), pack.patterns.len(), "ids are unique");
    for pattern in &pack.patterns {
        assert!(pattern.technique.starts_with('T'), "{} cites a technique", pattern.id);
        assert!(!pattern.why.is_empty() && !pattern.benign.is_empty(), "{} explains itself", pattern.id);
        assert!(pattern.tier != Tier::Confirmed, "{}: a pattern is never a verdict", pattern.id);
    }
}

fn pack_with(body: &str) -> Result<PatternPack, syntrac_core::patterns::PackError> {
    parse_pack(&format!(
        "version = \"1\"\n[[pattern]]\nid = \"x\"\ntitle = \"T\"\ntechnique = \"T1036.005\"\ntier = \"unusual\"\napplies_to = \"program\"\nwhy = \"W\"\nbenign = \"B\"\n{body}"
    ))
}

#[test]
fn a_valid_small_pack_loads() {
    let pack = pack_with("[pattern.when]\nname_is = [\"a.exe\"]\n").unwrap();

    assert_eq!(pack.patterns[0].id, "x");
    assert_eq!(pack.patterns[0].tier, Tier::Unusual);
}

#[test]
fn a_pack_with_anything_outside_the_closed_vocabulary_is_refused() {
    for bad in [
        "[pattern.when]\nregex = \"x.*\"\n", // no regular expressions over user data
        "[pattern.when]\nname_is = [\"a.exe\"]\nscript = \"1\"\n", // no code in rules
        "[pattern.when]\nlocation_is = [\"the_moon\"]\n", // an unknown place
        "[pattern.when]\nsignature_is = [\"fine\"]\n", // an unknown signature state
        "[pattern.when]\nentry_kind_is = [\"cron\"]\n", // an unknown start-entry kind
        "[pattern.when]\ndownloaded = false\n", // a condition is only ever "must be true"
        "[pattern.when]\n",                  // a pattern with no condition would match everything
        "[pattern.when]\nname_is = []\n",    // an empty list is no condition
    ] {
        assert!(pack_with(bad).is_err(), "should be refused: {bad}");
    }
}

#[test]
fn a_pattern_may_never_claim_the_tier_confirmed_and_ids_and_techniques_are_checked() {
    let body = "[pattern.when]\nname_is = [\"a.exe\"]\n";
    let text = format!(
        "version = \"1\"\n[[pattern]]\nid = \"x\"\ntitle = \"T\"\ntechnique = \"T1036.005\"\ntier = \"confirmed\"\napplies_to = \"program\"\nwhy = \"W\"\nbenign = \"B\"\n{body}"
    );
    assert!(parse_pack(&text).is_err());

    assert!(pack_with("[pattern.when]\nname_is = [\"a.exe\"]\n").is_ok());
    let wrong_technique = text.replace("confirmed", "unusual").replace("T1036.005", "banana");
    assert!(parse_pack(&wrong_technique).is_err());
    let bad_id = text.replace("confirmed", "unusual").replace("id = \"x\"", "id = \"Bad Id!\"");
    assert!(parse_pack(&bad_id).is_err());
    let no_benign = text.replace("confirmed", "unusual").replace("benign = \"B\"", "benign = \"\"");
    assert!(parse_pack(&no_benign).is_err());
}

#[test]
fn two_patterns_with_the_same_id_are_refused() {
    let one = "[[pattern]]\nid = \"x\"\ntitle = \"T\"\ntechnique = \"T1036\"\ntier = \"unusual\"\napplies_to = \"program\"\nwhy = \"W\"\nbenign = \"B\"\n[pattern.when]\nname_is = [\"a.exe\"]\n";
    assert!(parse_pack(&format!("version = \"1\"\n{one}{one}")).is_err());
}

// ---- name distance

#[test]
fn the_name_distance_counts_a_swap_as_one_edit() {
    assert_eq!(osa_distance("svchost", "svchost"), 0);
    assert_eq!(osa_distance("svchost", "svch0st"), 1); // one letter changed
    assert_eq!(osa_distance("svchost", "scvhost"), 1); // two letters swapped
    assert_eq!(osa_distance("svchost", "svchst"), 1); // one letter missing
    assert_eq!(osa_distance("svchost", "svchosts"), 1); // one letter added
    assert_eq!(osa_distance("svchost", "sychosx"), 2);
    assert!(osa_distance("svchost", "a-very-different-name") > 1);
}

// ---- a Windows name in the wrong folder

#[test]
fn a_windows_program_name_outside_the_windows_folder_without_a_microsoft_signature_is_suspicious() {
    let world = World::with(vec![
        entry(r"C:\Users\a\AppData\Local\Temp\svchost.exe", Some(unsigned())),
        entry(r"C:\Tools\explorer.exe", Some(facts(SignatureStatus::Valid, Some("Some Vendor Ltd")))),
    ]);

    let found = world.of("system_name_outside_windows");

    assert_eq!(found.len(), 2);
    assert!(found.iter().all(|c| c.tier == Tier::Suspicious));
    assert_eq!(found[0].subject, r"c:\users\a\appdata\local\temp\svchost.exe");
}

#[test]
fn the_real_windows_programs_are_quiet_wherever_windows_keeps_them() {
    let world = World::with(vec![
        entry(r"C:\Windows\System32\svchost.exe", Some(microsoft())),
        entry(r"C:\Windows\SysWOW64\svchost.exe", Some(microsoft())),
        entry(r"C:\Windows\explorer.exe", Some(microsoft())),
        entry(r"\Device\HarddiskVolume3\Windows\System32\lsass.exe", Some(microsoft())),
    ]);

    assert!(world.kinds().is_empty(), "{:?}", world.kinds());
}

#[test]
fn a_microsoft_signed_copy_elsewhere_and_a_file_not_read_yet_are_not_raised() {
    let world = World::with(vec![
        entry(r"C:\Program Files\Some App\conhost.exe", Some(microsoft())),
        entry(r"C:\Users\a\Downloads\svchost.exe", None),
    ]);

    assert!(world.of("system_name_outside_windows").is_empty());
}

#[test]
fn a_name_one_letter_away_from_a_windows_program_is_raised_and_the_real_name_and_far_names_are_not() {
    let world = World::with(vec![
        entry(r"C:\Users\a\AppData\Roaming\svch0st.exe", Some(unsigned())),
        entry(r"C:\Users\a\AppData\Roaming\scvhost.exe", Some(unsigned())),
        entry(r"C:\Users\a\AppData\Roaming\service.exe", Some(unsigned())), // generic name; "services" is not in the near list
        entry(r"C:\Users\a\AppData\Roaming\csrs.exe", Some(unsigned())),    // too short to judge
        entry(r"C:\Users\a\AppData\Roaming\notepad.exe", Some(unsigned())),
        entry(r"C:\Users\a\AppData\Roaming\svchost.exe", Some(unsigned())), // equal: that is the other pattern
    ]);

    let lookalikes: Vec<String> = world.of("system_name_lookalike").into_iter().map(|c| c.subject_label).collect();

    assert_eq!(lookalikes, vec!["svch0st.exe", "scvhost.exe"]);
}

// ---- an unsigned download in a user folder

#[test]
fn an_unsigned_download_in_a_user_folder_that_was_already_here_is_suspicious() {
    let world = World::with(vec![entry(r"C:\Users\a\Downloads\tool.exe", Some(downloaded(unsigned())))]);

    let found = world.of("unsigned_download_in_user_folder");

    assert_eq!(found.len(), 1);
    assert_eq!(found[0].tier, Tier::Suspicious);
}

#[test]
fn without_the_download_mark_or_when_signed_or_in_program_files_it_is_quiet() {
    let world = World::with(vec![
        entry(r"C:\Users\a\code\build\tool.exe", Some(unsigned())), // a build output: no download mark
        entry(r"C:\Users\a\Downloads\signed.exe", Some(downloaded(facts(SignatureStatus::Valid, Some("Vendor"))))),
        entry(r"C:\Program Files\Vendor\unsigned.exe", Some(downloaded(unsigned()))),
    ]);

    assert!(world.of("unsigned_download_in_user_folder").is_empty());
}

#[test]
fn a_program_first_seen_after_the_switch_on_is_left_to_the_new_program_rule() {
    let world = World::with(vec![entry_between(
        r"C:\Users\a\Downloads\fresh.exe",
        SINCE + 3,
        NOW - 1,
        Some(downloaded(unsigned())),
    )]);

    assert!(world.of("unsigned_download_in_user_folder").is_empty());
}

// ---- start entries

#[test]
fn an_old_start_entry_for_an_unsigned_program_in_a_user_folder_is_suspicious() {
    let path = r"C:\Users\a\AppData\Roaming\helper\helper.exe";
    let exe = Executable::from_path(path).unwrap();
    let world = World {
        entries: vec![entry(path, Some(unsigned()))],
        launchers: vec![(exe, vec![run_entry("HKCU\\Run\\Helper", SINCE - 20)])],
        ..World::default()
    };

    let found = world.of("start_entry_unsigned_in_writable_folder");

    assert_eq!(found.len(), 1);
    assert_eq!(found[0].tier, Tier::Suspicious);
    assert!(found[0].subject.contains("|run_key|"), "the entry is part of the subject: {}", found[0].subject);
    assert!(found[0].evidence.iter().any(|line| line.contains("HKCU")));
}

#[test]
fn a_signed_app_that_starts_from_appdata_and_an_unsigned_one_in_program_files_are_quiet() {
    let onedrive = r"C:\Users\a\AppData\Local\Microsoft\OneDrive\OneDrive.exe";
    let vendor = r"C:\Program Files\Vendor\agent.exe";
    let world = World {
        entries: vec![entry(onedrive, Some(microsoft())), entry(vendor, Some(unsigned()))],
        launchers: vec![
            (Executable::from_path(onedrive).unwrap(), vec![run_entry("HKCU\\Run\\OneDrive", SINCE - 20)]),
            (Executable::from_path(vendor).unwrap(), vec![run_entry("HKLM\\Run\\Agent", SINCE - 20)]),
        ],
        ..World::default()
    };

    assert!(world.of("start_entry_unsigned_in_writable_folder").is_empty());
}

#[test]
fn a_start_entry_first_seen_after_the_switch_on_is_left_to_the_new_start_entry_rule() {
    let path = r"C:\Users\a\AppData\Roaming\helper\helper.exe";
    let world = World {
        entries: vec![entry(path, Some(unsigned()))],
        launchers: vec![(Executable::from_path(path).unwrap(), vec![run_entry("HKCU\\Run\\Helper", SINCE + 2)])],
        ..World::default()
    };

    assert!(world.of("start_entry_unsigned_in_writable_folder").is_empty());
}

// ---- disguised names

#[test]
fn a_document_name_that_is_really_a_program_is_raised_and_ordinary_dotted_names_are_not() {
    let world = World::with(vec![
        entry(r"C:\Users\a\Downloads\invoice.pdf.exe", Some(unsigned())),
        entry(r"C:\Users\a\Downloads\photo.JPG.scr", Some(unsigned())),
        entry(r"C:\Tools\python3.11.exe", Some(unsigned())),
        entry(r"C:\Tools\setup.x64.exe", Some(unsigned())),
        entry(r"C:\Tools\report.pdf", Some(unsigned())),
    ]);

    let labels: Vec<String> = world.of("double_extension").into_iter().map(|c| c.subject_label).collect();

    assert_eq!(labels, vec!["invoice.pdf.exe", "photo.JPG.scr"]);
}

#[test]
fn a_direction_override_in_the_name_is_raised_and_the_words_describe_it_without_repeating_it() {
    let name = "invoice\u{202E}fdp.exe";
    let world = World::with(vec![entry(&format!(r"C:\Users\a\Downloads\{name}"), Some(unsigned()))]);

    let found = world.of("direction_override_name");

    assert_eq!(found.len(), 1);
    // What is stored has passed through the cleaning every candidate goes through.
    let stored = clean_candidate(found[0].clone()).unwrap();
    let text = format!("{} {} {}", stored.title, stored.why, stored.evidence.join(" "));
    assert!(!text.contains('\u{202E}'), "the control character is never echoed");
    assert!(stored.evidence.iter().any(|line| line.contains("U+202E")), "{:?}", stored.evidence);
}

// ---- tools used by administrators and attackers

#[test]
fn a_remote_access_tool_is_unusual_when_signed_and_suspicious_when_unsigned_in_a_user_folder() {
    let world = World::with(vec![
        entry(
            r"C:\Program Files (x86)\AnyDesk\AnyDesk.exe",
            Some(facts(SignatureStatus::Valid, Some("philandro Software GmbH"))),
        ),
        entry(r"C:\Users\a\Downloads\rustdesk.exe", Some(unsigned())),
    ]);

    let found = world.of("remote_access_tool");

    assert_eq!(found.len(), 2);
    let tier_of = |label: &str| found.iter().find(|c| c.subject_label.eq_ignore_ascii_case(label)).unwrap().tier;
    assert_eq!(tier_of("AnyDesk.exe"), Tier::Unusual);
    assert_eq!(tier_of("rustdesk.exe"), Tier::Suspicious);
}

#[test]
fn tunnel_and_bulk_copy_tools_are_named_too_and_each_pattern_has_its_own_kind() {
    let world = World::with(vec![
        entry(r"C:\Tools\ngrok.exe", Some(unsigned())),
        entry(r"C:\Tools\rclone.exe", Some(unsigned())),
    ]);

    let mut kinds = world.kinds();
    kinds.sort();

    assert_eq!(kinds, vec!["pattern_bulk_copy_tool", "pattern_tunnel_tool"]);
}

// ---- what every match says, and what it does not

#[test]
fn every_match_says_which_pattern_why_and_what_could_be_harmless_and_a_new_pack_does_not_change_the_key() {
    let world = World::with(vec![entry(r"C:\Tools\rclone.exe", Some(unsigned()))]);

    let found = &world.run()[0];

    assert_eq!(found.kind, "pattern_bulk_copy_tool");
    assert_eq!(found.subject, r"c:\tools\rclone.exe");
    assert_eq!(found.dedupe_key, r"pattern_bulk_copy_tool:c:\tools\rclone.exe");
    assert!(found.title.contains("rclone.exe"));
    let evidence = found.evidence.join("\n");
    assert!(evidence.contains("T1567.002"), "{evidence}");
    assert!(evidence.contains("Often harmless"), "{evidence}");
    assert!(evidence.contains("Pattern pack 2026.11.2"), "{evidence}");
    assert!(!found.dedupe_key.contains("2026"), "the pack version is not part of the key");
}

#[test]
fn a_known_product_is_never_matched() {
    let mut world = World::with(vec![entry(r"C:\Tools\rclone.exe", Some(unsigned()))]);
    world.known.push(KnownProduct {
        id: 1,
        label: "My backup".into(),
        publisher: None,
        root: r"c:\tools".into(),
        product_name: None,
        note: None,
        created_at: at(1),
    });

    assert!(world.run().is_empty());
}

#[test]
fn only_programs_seen_in_the_recent_days_are_judged() {
    let world = World::with(vec![
        entry_between(r"C:\Tools\ngrok.exe", SINCE - 90, NOW - RECENT_DAYS - 5, Some(unsigned())),
        entry_between(r"C:\Tools\rclone.exe", SINCE - 90, NOW - RECENT_DAYS + 2, Some(unsigned())),
    ]);

    let labels: Vec<String> = world.run().into_iter().map(|c| c.subject_label).collect();

    assert_eq!(labels, vec!["rclone.exe"]);
}

#[test]
fn a_program_that_was_never_seen_running_is_not_judged() {
    let mut never = entry(r"C:\Tools\rclone.exe", Some(unsigned()));
    never.seen = None;

    assert!(World::with(vec![never]).run().is_empty());
}

#[test]
fn one_pattern_raises_at_most_a_fixed_number_of_findings_in_a_run() {
    let many: Vec<IdentityEntry> =
        (0..MAX_PER_PATTERN + 30).map(|n| entry(&format!(r"C:\Tools\{n}\rclone.exe"), Some(unsigned()))).collect();

    assert_eq!(World::with(many).of("bulk_copy_tool").len(), MAX_PER_PATTERN);
}

// ---- F5b: a program started by another program, and a helper's first use of the network

fn spawn(parent: &str, child: &str, count: u32, first_day: i64, last_day: i64) -> SpawnPair {
    SpawnPair {
        parent: Executable::from_path(parent).unwrap(),
        child: Executable::from_path(child).unwrap(),
        count,
        first: at(first_day),
        last: at(last_day),
    }
}

const WORD: &str = r"C:\Program Files\Microsoft Office\root\Office16\WINWORD.EXE";
const CMD: &str = r"C:\Windows\System32\cmd.exe";
const POWERSHELL: &str = r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe";

#[test]
fn an_office_program_starting_a_command_shell_is_suspicious_and_the_evidence_names_both_programs() {
    let world = World {
        spawns: vec![spawn(WORD, CMD, 3, NOW - 6, NOW - 2), spawn(WORD, POWERSHELL, 1, NOW - 3, NOW - 3)],
        ..World::default()
    };

    let found = world.of("office_starts_shell");

    assert_eq!(found.len(), 2);
    assert!(found.iter().all(|c| c.tier == Tier::Suspicious));
    let cmd = found.iter().find(|c| c.subject_label == "cmd.exe").unwrap();
    assert_eq!(cmd.subject, format!("{}|{}", WORD.to_lowercase(), CMD.to_lowercase()));
    assert_eq!(cmd.dedupe_key, format!("pattern_office_starts_shell:{}", cmd.subject));
    let evidence = cmd.evidence.join("\n");
    assert!(evidence.contains("WINWORD.EXE") && evidence.contains("cmd.exe"), "{evidence}");
    assert!(evidence.contains("3 time"), "how often: {evidence}");
    assert!(evidence.contains("T1059"), "{evidence}");
    assert!(!evidence.to_lowercase().contains("command line"), "command lines are not stored, so none is claimed");
}

#[test]
fn ordinary_parents_and_ordinary_children_are_quiet() {
    let world = World {
        spawns: vec![
            spawn(r"C:\Windows\explorer.exe", CMD, 40, NOW - 9, NOW - 1), // a person opening a shell
            spawn(WORD, r"C:\Windows\System32\splwow64.exe", 5, NOW - 9, NOW - 1), // not a shell
            spawn(r"C:\Program Files\Node\node.exe", POWERSHELL, 400, NOW - 9, NOW - 1),
        ],
        ..World::default()
    };

    assert!(world.run().is_empty(), "{:?}", world.kinds());
}

#[test]
fn a_browser_starting_a_shell_is_unusual_and_has_its_own_pattern() {
    let world = World {
        spawns: vec![spawn(r"C:\Program Files\Google\Chrome\Application\chrome.exe", CMD, 2, NOW - 5, NOW - 4)],
        ..World::default()
    };

    let found = world.of("browser_starts_shell");

    assert_eq!(found.len(), 1);
    assert_eq!(found[0].tier, Tier::Unusual);
    assert!(world.of("office_starts_shell").is_empty());
}

#[test]
fn a_start_older_than_the_recent_days_is_not_judged() {
    let world = World { spawns: vec![spawn(WORD, CMD, 3, NOW - 90, NOW - RECENT_DAYS - 3)], ..World::default() };

    assert!(world.run().is_empty());
}

#[test]
fn the_child_must_still_satisfy_its_own_conditions_the_pack_may_add() {
    // The pack's pattern names the children it cares about: a program with another name is not matched even from an office parent.
    let world = World { spawns: vec![spawn(WORD, r"C:\Tools\helper.exe", 1, NOW - 2, NOW - 2)], ..World::default() };

    assert!(world.of("office_starts_shell").is_empty());
}

#[test]
fn a_script_helper_using_the_network_for_the_first_time_is_suspicious() {
    let certutil = r"C:\Windows\System32\certutil.exe";
    let mut world = World::with(vec![entry(certutil, Some(microsoft()))]);
    world.first_online = vec![Executable::from_path(certutil).unwrap()];

    let found = world.of("script_helper_first_network_use");

    assert_eq!(found.len(), 1);
    assert_eq!(found[0].tier, Tier::Suspicious);
    assert!(found[0].why.to_lowercase().contains("network"));
}

#[test]
fn a_helper_that_was_online_before_and_a_program_that_is_not_a_helper_are_quiet() {
    let certutil = r"C:\Windows\System32\certutil.exe";
    let browser = r"C:\Program Files\Google\Chrome\Application\chrome.exe";
    let mut world = World::with(vec![
        entry(certutil, Some(microsoft())),
        entry(browser, Some(facts(SignatureStatus::Valid, Some("Google LLC")))),
    ]);
    // Only the browser is first online after the switch-on: it is not a helper. The helper is not in the list: it was online before.
    world.first_online = vec![Executable::from_path(browser).unwrap()];

    assert!(world.of("script_helper_first_network_use").is_empty());
}

#[test]
fn the_new_vocabulary_is_checked_too() {
    let base = |applies: &str, when: &str| {
        parse_pack(&format!(
            "version = \"1\"\n[[pattern]]\nid = \"x\"\ntitle = \"T\"\ntechnique = \"T1059\"\ntier = \"unusual\"\napplies_to = \"{applies}\"\nwhy = \"W\"\nbenign = \"B\"\n[pattern.when]\n{when}"
        ))
    };

    assert!(base("process_start", "parent_is = [\"winword.exe\"]\nname_is = [\"cmd.exe\"]\n").is_ok());
    assert!(base("program", "first_network_use = true\nname_is = [\"certutil.exe\"]\n").is_ok());
    assert!(
        base("program", "parent_is = [\"winword.exe\"]\n").is_err(),
        "a parent only makes sense for a started program"
    );
    assert!(base("process_start", "name_is = [\"cmd.exe\"]\n").is_err(), "a started-by pattern names its parents");
    assert!(base("process_start", "parent_is = [\"a.exe\"]\nalready_here = true\n").is_err());
    assert!(base("start_entry", "first_network_use = true\nentry_kind_is = [\"task\"]\n").is_err());
    assert!(base("program", "first_network_use = false\nname_is = [\"a.exe\"]\n").is_err());
}
