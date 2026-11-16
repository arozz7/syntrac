//! The Uninstall registry keys as install records (I4c): the machine's (64-bit and the 32-bit programs' view) and each LOADED user's.
//! Read only. An entry is one subkey with a `DisplayName`; nothing is created, changed or run (the uninstall command is text, never
//! started).

use syntrac_core::{InstallSource, InstallSourceError, RawInstall, SourceInstalls};

use crate::registry::{RegKey, Root};

const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
const UNINSTALL_32: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall";

pub struct UninstallKeySource;

fn read_entry(root: Root, parent: &str, hive: &str, name: &str) -> Option<RawInstall> {
    let key = RegKey::open(root, &format!(r"{parent}\{name}"))?;
    let values = key.text_values();
    let text = |wanted: &str| {
        values
            .iter()
            .find(|(found, _)| found.eq_ignore_ascii_case(wanted))
            .map(|(_, value)| value.clone())
            .unwrap_or_default()
    };
    Some(RawInstall {
        hive: hive.to_owned(),
        key_name: name.to_owned(),
        display_name: text("DisplayName"),
        publisher: text("Publisher"),
        version: text("DisplayVersion"),
        install_date: text("InstallDate"),
        install_location: text("InstallLocation"),
        display_icon: text("DisplayIcon"),
        uninstall_string: text("UninstallString"),
        system_component: key.dword("SystemComponent") == Some(1),
    })
}

fn read_folder(read: &mut SourceInstalls, root: Root, parent: &str, hive: &str) {
    let Some(folder) = RegKey::open(root, parent) else { return };
    for name in folder.subkeys() {
        match read_entry(root, parent, hive, &name) {
            Some(entry) => read.entries.push(entry),
            None => read.unreadable += 1,
        }
    }
}

/// An account of a person, as in the Run key source.
fn is_person_sid(name: &str) -> bool {
    name.starts_with("S-1-5-21-") && !name.ends_with("_Classes")
}

impl InstallSource for UninstallKeySource {
    fn read(&self) -> Result<SourceInstalls, InstallSourceError> {
        // The machine's keys must be readable for the read to mean anything; a missing one is empty.
        if RegKey::open(Root::LocalMachine, UNINSTALL).is_none() {
            return Err(InstallSourceError("the machine's Uninstall registry key could not be read".to_owned()));
        }
        let mut read = SourceInstalls::default();
        read_folder(&mut read, Root::LocalMachine, UNINSTALL, "machine");
        read_folder(&mut read, Root::LocalMachine, UNINSTALL_32, "machine32");
        if let Some(users) = RegKey::open(Root::Users, "") {
            for sid in users.subkeys().into_iter().filter(|name| is_person_sid(name)) {
                read_folder(&mut read, Root::Users, &format!(r"{sid}\{UNINSTALL}"), &format!("user:{sid}"));
            }
        }
        Ok(read)
    }
}
