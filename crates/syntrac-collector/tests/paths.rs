//! NT device paths (as ETW reports them) must become drive-letter paths, using an injected volume map.

use syntrac_collector::paths::NtPathConverter;

fn converter() -> NtPathConverter {
    NtPathConverter::new(vec![
        (r"\Device\HarddiskVolume7".to_owned(), "C:".to_owned()),
        (r"\Device\HarddiskVolume3".to_owned(), "D:".to_owned()),
    ])
}

#[test]
fn a_spike_path_becomes_a_drive_letter_path() {
    let nt = r"\Device\HarddiskVolume7\Users\alice\anaconda3\Library\mingw-w64\bin\jq.exe";
    assert_eq!(converter().convert(nt), r"C:\Users\alice\anaconda3\Library\mingw-w64\bin\jq.exe");
}

#[test]
fn the_device_prefix_is_matched_case_insensitively() {
    assert_eq!(
        converter().convert(r"\device\harddiskvolume3\Program Files\Git\usr\bin\bash.exe"),
        r"D:\Program Files\Git\usr\bin\bash.exe"
    );
}

#[test]
fn a_device_name_that_merely_starts_the_same_is_not_a_match() {
    let nt = r"\Device\HarddiskVolume70\foo.exe";
    assert_eq!(converter().convert(nt), nt, "volume 70 is not volume 7");
}

#[test]
fn the_longest_matching_device_wins() {
    let converter = NtPathConverter::new(vec![
        (r"\Device\HarddiskVolume1".to_owned(), "A:".to_owned()),
        (r"\Device\HarddiskVolume10".to_owned(), "E:".to_owned()),
    ]);
    assert_eq!(converter.convert(r"\Device\HarddiskVolume10\a.exe"), r"E:\a.exe");
    assert_eq!(converter.convert(r"\Device\HarddiskVolume1\a.exe"), r"A:\a.exe");
}

#[test]
fn unknown_devices_pass_through_unchanged() {
    let network = r"\Device\Mup\server\share\tool.exe";
    assert_eq!(converter().convert(network), network);
}

#[test]
fn the_bare_device_root_becomes_the_drive_root() {
    assert_eq!(converter().convert(r"\Device\HarddiskVolume7"), r"C:\");
}

#[test]
fn the_win32_namespace_prefix_is_stripped() {
    assert_eq!(converter().convert(r"\??\C:\Windows\System32\cmd.exe"), r"C:\Windows\System32\cmd.exe");
}

#[test]
fn already_converted_paths_and_bare_names_are_left_alone() {
    assert_eq!(converter().convert(r"C:\Windows\notepad.exe"), r"C:\Windows\notepad.exe");
    assert_eq!(converter().convert("jq.exe"), "jq.exe");
    assert_eq!(converter().convert(""), "");
}
