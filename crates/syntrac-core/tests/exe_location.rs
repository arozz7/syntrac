//! Where an executable lives and which product it belongs to: pure rules on the path (and, for the product, the signer).

use syntrac_core::{
    classify_location, install_root, product_key, FileFacts, LocationClass, Signature, SignatureStatus,
};

fn class(path: &str) -> LocationClass {
    classify_location(path)
}

#[test]
fn windows_and_program_files_are_told_apart_from_the_user_profile() {
    assert_eq!(class(r"C:\Windows\System32\svchost.exe"), LocationClass::System);
    assert_eq!(class(r"C:\Windows\explorer.exe"), LocationClass::System);
    assert_eq!(class(r"C:\Program Files\Mozilla Firefox\firefox.exe"), LocationClass::ProgramFiles);
    assert_eq!(class(r"C:\Program Files (x86)\Acme\acmeserv.exe"), LocationClass::ProgramFiles);
    assert_eq!(class(r"C:\ProgramData\Microsoft\Windows Defender\Platform\MsMpEng.exe"), LocationClass::ProgramData);
    assert_eq!(class(r"C:\Users\alice\Downloads\setup.exe"), LocationClass::UserProfile);
    assert_eq!(class(r"C:\Users\alice\.cargo\bin\cargo.exe"), LocationClass::UserProfile);
    assert_eq!(class(r"C:\Users\alice\AppData\Local\Programs\Microsoft VS Code\Code.exe"), LocationClass::UserProfile);
}

#[test]
fn temporary_folders_are_their_own_class_wherever_they_are() {
    assert_eq!(class(r"C:\Users\alice\AppData\Local\Temp\x\update.exe"), LocationClass::Temporary);
    assert_eq!(class(r"C:\Windows\Temp\setup.exe"), LocationClass::Temporary);
    assert_eq!(class(r"J:\Users\bob\AppData\Local\Temp\a.exe"), LocationClass::Temporary);
    // A folder merely named like a temp folder inside a program is not one.
    assert_eq!(class(r"C:\Program Files\Tempo\tempo.exe"), LocationClass::ProgramFiles);
}

#[test]
fn the_drive_letter_and_the_case_do_not_matter() {
    assert_eq!(class(r"c:\PROGRAM FILES\app\a.exe"), LocationClass::ProgramFiles);
    assert_eq!(class(r"D:\Program Files\app\a.exe"), LocationClass::ProgramFiles);
    assert_eq!(class("C:/Windows/System32/cmd.exe"), LocationClass::System, "forward slashes are accepted");
}

#[test]
fn extended_and_device_prefixes_are_removed_before_classifying() {
    assert_eq!(class(r"\\?\C:\Program Files\app\a.exe"), LocationClass::ProgramFiles);
    assert_eq!(class(r"\??\C:\Windows\System32\smss.exe"), LocationClass::System);
    assert_eq!(class(r"\Device\HarddiskVolume3\Windows\System32\csrss.exe"), LocationClass::System);
    assert_eq!(class(r"\\?\UNC\fileserver\share\tool.exe"), LocationClass::Network);
}

#[test]
fn network_other_and_unknown_paths() {
    assert_eq!(class(r"\\fileserver\share\tool.exe"), LocationClass::Network);
    assert_eq!(class(r"D:\Games\Steam\steam.exe"), LocationClass::Other);
    assert_eq!(class(r"C:\tools\x.exe"), LocationClass::Other);
    assert_eq!(class("vmmemWSL"), LocationClass::Unknown, "a name without a path says nothing about location");
    assert_eq!(class(""), LocationClass::Unknown);
    assert_eq!(class("   "), LocationClass::Unknown);
    assert_eq!(class(r"..\relative\a.exe"), LocationClass::Unknown);
}

#[test]
fn the_install_root_is_the_folder_that_names_the_product() {
    assert_eq!(
        install_root(r"C:\Program Files (x86)\Acme\acmeserv.exe").as_deref(),
        Some(r"c:\program files (x86)\acme")
    );
    assert_eq!(
        install_root(r"C:\Program Files\Mozilla Firefox\browser\crashreporter.exe").as_deref(),
        Some(r"c:\program files\mozilla firefox"),
        "deeper folders belong to the same product"
    );
    assert_eq!(
        install_root(r"C:\Users\alice\AppData\Local\Programs\Microsoft VS Code\bin\code.exe").as_deref(),
        Some(r"c:\users\alice\appdata\local\programs\microsoft vs code")
    );
    assert_eq!(
        install_root(r"C:\Users\alice\AppData\Local\Fabrikam\FabrikamAppEngine\x.exe").as_deref(),
        Some(r"c:\users\alice\appdata\local\fabrikam")
    );
    assert_eq!(install_root(r"C:\Users\alice\.cargo\bin\cargo.exe").as_deref(), Some(r"c:\users\alice\.cargo"));
    assert_eq!(install_root(r"C:\Windows\System32\svchost.exe").as_deref(), Some(r"c:\windows\system32"));
    assert_eq!(install_root(r"D:\Games\Steam\steam.exe").as_deref(), Some(r"d:\games"));
    assert_eq!(install_root(r"\\fileserver\share\Tools\x.exe").as_deref(), Some(r"\\fileserver\share\tools"));
}

#[test]
fn a_file_directly_in_a_root_folder_uses_that_folder() {
    assert_eq!(install_root(r"C:\Program Files\lonely.exe").as_deref(), Some(r"c:\program files"));
    assert_eq!(install_root(r"C:\a.exe").as_deref(), Some(r"c:\"));
}

#[test]
fn no_root_for_a_name_without_a_path() {
    assert_eq!(install_root("vmmemWSL"), None);
    assert_eq!(install_root(""), None);
}

fn facts(signer: Option<&str>, status: SignatureStatus, company: Option<&str>) -> FileFacts {
    FileFacts {
        signature: Signature { status, signer: signer.map(str::to_owned), thumbprint: None, kind: None },
        version: syntrac_core::VersionInfo { company: company.map(str::to_owned), ..Default::default() },
        ..FileFacts::default()
    }
}

#[test]
fn the_four_acme_programs_are_one_product() {
    let signed = facts(Some("Acme Software, Inc."), SignatureStatus::Valid, Some("Acme"));
    let keys: Vec<_> = ["acmeserv.exe", "acmebui.exe", "acmefilelist64.exe", "acmetransmit64.exe"]
        .iter()
        .map(|name| product_key(&format!(r"C:\Program Files (x86)\Acme\{name}"), Some(&signed)).unwrap())
        .collect();

    assert!(keys.windows(2).all(|pair| pair[0] == pair[1]), "{keys:?}");
}

#[test]
fn the_same_signer_in_two_folders_is_two_products() {
    let signed = facts(Some("Microsoft Corporation"), SignatureStatus::Valid, None);
    let a = product_key(r"C:\Program Files\Microsoft Office\a.exe", Some(&signed)).unwrap();
    let b = product_key(r"C:\Program Files\Microsoft SQL Server\a.exe", Some(&signed)).unwrap();

    assert_ne!(a, b);
}

#[test]
fn the_same_folder_with_a_different_signer_is_a_different_product() {
    let real = facts(Some("Acme Software, Inc."), SignatureStatus::Valid, None);
    let imposter = facts(None, SignatureStatus::Unsigned, None);
    let a = product_key(r"C:\Program Files (x86)\Acme\acmeserv.exe", Some(&real)).unwrap();
    let b = product_key(r"C:\Program Files (x86)\Acme\acmefake.exe", Some(&imposter)).unwrap();

    assert_ne!(a, b, "an unsigned file next to signed ones must not be folded into their product");
}

#[test]
fn the_publisher_comes_from_the_signer_first_then_the_company_name() {
    let both = facts(Some("Acme Software, Inc."), SignatureStatus::Valid, Some("Something Else"));
    let company_only = facts(None, SignatureStatus::Unsigned, Some("Acme"));
    let nothing = FileFacts::default();

    assert_eq!(
        product_key(r"C:\Program Files\X\a.exe", Some(&both)).unwrap().publisher.as_deref(),
        Some("acme software, inc.")
    );
    assert_eq!(
        product_key(r"C:\Program Files\X\a.exe", Some(&company_only)).unwrap().publisher.as_deref(),
        Some("acme")
    );
    assert_eq!(product_key(r"C:\Program Files\X\a.exe", Some(&nothing)).unwrap().publisher, None);
    assert_eq!(product_key(r"C:\Program Files\X\a.exe", None).unwrap().publisher, None, "not inspected yet");
}

#[test]
fn the_signer_is_compared_without_regard_to_case_or_padding() {
    let a = facts(Some("  ACME SOFTWARE, INC. "), SignatureStatus::Valid, None);
    let b = facts(Some("Acme Software, Inc."), SignatureStatus::Valid, None);

    assert_eq!(product_key(r"C:\Program Files\X\a.exe", Some(&a)), product_key(r"C:\Program Files\X\a.exe", Some(&b)));
}

#[test]
fn no_product_for_a_path_that_is_not_one() {
    assert_eq!(product_key("vmmemWSL", None), None);
}
