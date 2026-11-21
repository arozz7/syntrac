//! Install records (the Uninstall registry keys, I4c): turning a registry entry into a record with an install folder, and matching a
//! program to the record whose folder holds it. Pure. Evidence only: anything that can write the registry can claim any name.

use syntrac_core::install_records::{registered_for, to_record, InstallRecord, InstallSeen, RawInstall};
use syntrac_core::Timestamp;

fn raw(name: &str) -> RawInstall {
    RawInstall {
        hive: "machine".to_owned(),
        key_name: format!("{{{name}}}"),
        display_name: name.to_owned(),
        publisher: "Example, Inc.".to_owned(),
        version: "1.2.3".to_owned(),
        install_date: "20260504".to_owned(),
        install_location: String::new(),
        display_icon: String::new(),
        uninstall_string: String::new(),
        system_component: false,
    }
}

fn seen(record: InstallRecord, current: bool) -> InstallSeen {
    InstallSeen { record, first_seen: Timestamp::from_micros(1), last_seen: Timestamp::from_micros(2), current }
}

fn record_with_folder(name: &str, folder: &str) -> InstallSeen {
    let mut r = raw(name);
    r.install_location = folder.to_owned();
    seen(to_record(&r).unwrap(), true)
}

// ---- from a registry entry to a record

#[test]
fn an_install_location_becomes_the_folder_lower_cased_without_quotes_or_a_trailing_backslash() {
    let mut r = raw("Acme");
    r.install_location = r#""C:\Program Files (x86)\Acme\""#.to_owned();

    let record = to_record(&r).unwrap();

    assert_eq!(record.location.as_deref(), Some(r"c:\program files (x86)\acme"));
    assert!(!record.derived);
    assert_eq!(
        (record.display_name.as_str(), record.publisher.as_deref(), record.version.as_deref()),
        ("Acme", Some("Example, Inc."), Some("1.2.3"))
    );
    assert_eq!(record.install_date.as_deref(), Some("20260504"));
}

#[test]
fn without_an_install_location_the_folder_is_derived_from_the_icon_or_the_uninstall_command_and_says_so() {
    let mut icon = raw("Tool");
    icon.display_icon = r#""C:\Program Files\Tool\tool.exe",0"#.to_owned();
    let record = to_record(&icon).unwrap();
    assert_eq!(record.location.as_deref(), Some(r"c:\program files\tool"));
    assert!(record.derived);

    let mut uninstall = raw("Other");
    uninstall.uninstall_string = r#""C:\Program Files\Other\unins000.exe" /SILENT"#.to_owned();
    assert_eq!(to_record(&uninstall).unwrap().location.as_deref(), Some(r"c:\program files\other"));

    let mut unquoted = raw("Plain");
    unquoted.uninstall_string = r"C:\Apps\Plain\uninstall.exe /S".to_owned();
    assert_eq!(to_record(&unquoted).unwrap().location.as_deref(), Some(r"c:\apps\plain"));
}

#[test]
fn an_installer_service_command_names_no_folder_of_the_product() {
    let mut msi = raw("Installed by MSI");
    msi.uninstall_string = r"MsiExec.exe /X{AAAAAAAA-0000-0000-0000-000000000001}".to_owned();
    msi.display_icon = r"C:\Windows\Installer\{AAAAAAAA}\icon.ico".to_owned();

    assert_eq!(
        to_record(&msi).unwrap().location,
        None,
        "System32 and the installer cache are not where the product lives"
    );
}

#[test]
fn folders_that_are_too_general_are_never_used() {
    for general in [
        r"C:\",
        r"C:\Program Files",
        r"C:\Program Files (x86)\",
        r"C:\Windows",
        r"C:\Windows\System32",
        r"C:\ProgramData",
        r"C:\Users",
        r"C:\Users\someone",
        r"C:\Users\someone\AppData\Local",
        r"C:\Users\someone\AppData\Local\Programs",
        r"C:\Users\someone\AppData\Roaming",
        r"C:\Users\someone\Downloads",
        r"C:\Users\someone\AppData\Local\Temp",
        r"\\server\share",
    ] {
        let mut r = raw("X");
        r.install_location = general.to_owned();
        assert_eq!(to_record(&r).unwrap().location, None, "{general}");
    }
}

#[test]
fn a_folder_inside_a_general_one_is_fine() {
    let mut r = raw("X");
    r.install_location = r"C:\Users\someone\AppData\Local\Programs\Vendor".to_owned();

    assert!(to_record(&r).unwrap().location.is_some());
}

#[test]
fn an_entry_with_no_name_is_not_a_product_and_text_is_cleaned() {
    assert!(to_record(&raw("")).is_none());
    assert!(to_record(&raw("   ")).is_none());
    let mut r = raw("Evil\u{202E}Name\u{0007}");
    r.publisher = "\u{0007}".to_owned();
    let record = to_record(&r).unwrap();
    assert_eq!(record.display_name, "EvilName");
    assert_eq!(record.publisher, None, "nothing printable is no publisher");
}

#[test]
fn a_hidden_system_component_is_flagged() {
    let mut r = raw("Microsoft Visual C++ 2015 Redistributable");
    r.system_component = true;

    assert!(to_record(&r).unwrap().system_component);
}

// ---- which record holds a program

const BB: &str = r"C:\Program Files (x86)\Acme\acmeserv.exe";

#[test]
fn a_program_matches_the_record_whose_folder_holds_it_in_any_spelling() {
    let installs = vec![record_with_folder("Acme", r"C:\Program Files (x86)\Acme")];

    for path in [BB, r"c:\program files (x86)\acme\acmeserv.exe", r"C:\Program Files (x86)\Acme\x64\acmetransmit64.exe"]
    {
        assert_eq!(registered_for(&installs, path).len(), 1, "{path}");
    }
}

#[test]
fn a_folder_is_not_matched_by_a_longer_name_that_merely_starts_the_same() {
    let installs = vec![record_with_folder("Acme", r"C:\Program Files (x86)\Acme")];

    assert!(registered_for(&installs, r"C:\Program Files (x86)\AcmeEvil\acmeserv.exe").is_empty());
    assert!(
        registered_for(&installs, r"C:\Users\someone\Downloads\acmeserv.exe").is_empty(),
        "a look-alike elsewhere is not registered"
    );
}

#[test]
fn the_longest_folder_wins() {
    let installs = vec![
        record_with_folder("Suite", r"C:\Program Files\Vendor"),
        record_with_folder("Suite Helper", r"C:\Program Files\Vendor\Helper"),
    ];

    let found = registered_for(&installs, r"C:\Program Files\Vendor\Helper\h.exe");

    assert_eq!(found.iter().map(|s| s.record.display_name.as_str()).collect::<Vec<_>>(), ["Suite Helper"]);
    let outer = registered_for(&installs, r"C:\Program Files\Vendor\main.exe");
    assert_eq!(outer[0].record.display_name, "Suite");
}

#[test]
fn equally_specific_records_are_all_shown_not_one_picked_silently_and_duplicates_of_the_same_product_are_one() {
    let installs = vec![
        record_with_folder("Product A", r"C:\Apps\Shared"),
        record_with_folder("Product B", r"C:\Apps\Shared"),
        record_with_folder("Product B", r"C:\Apps\Shared"), // the 32-bit and 64-bit views of one entry
    ];

    let found = registered_for(&installs, r"C:\Apps\Shared\x.exe");

    assert_eq!(found.iter().map(|s| s.record.display_name.as_str()).collect::<Vec<_>>(), ["Product A", "Product B"]);
}

#[test]
fn a_record_that_is_no_longer_installed_still_matches_but_says_so_and_a_record_without_a_folder_matches_nothing() {
    let mut r = raw("Gone");
    r.install_location = r"C:\Apps\Gone".to_owned();
    let installs = vec![seen(to_record(&r).unwrap(), false), seen(to_record(&raw("Nowhere")).unwrap(), true)];

    let found = registered_for(&installs, r"C:\Apps\Gone\g.exe");

    assert_eq!(found.len(), 1);
    assert!(!found[0].current);
    assert!(registered_for(&installs, r"C:\Apps\Nowhere\n.exe").is_empty());
}

#[test]
fn a_current_record_is_listed_before_one_that_is_gone_when_equally_specific() {
    let mut old = raw("Old");
    old.install_location = r"C:\Apps\Both".to_owned();
    let installs = vec![seen(to_record(&old).unwrap(), false), record_with_folder("New", r"C:\Apps\Both")];

    let found = registered_for(&installs, r"C:\Apps\Both\b.exe");

    assert_eq!(found.iter().map(|s| s.record.display_name.as_str()).collect::<Vec<_>>(), ["New", "Old"]);
}
