//! The Run and RunOnce registry keys as launch records (I4b): the machine's (64-bit and the 32-bit programs' view) and each LOADED
//! user's. A user who is not signed in has an unloaded registry hive and is not read; that is a blind spot the screen states. The
//! service's own `HKEY_CURRENT_USER` is SYSTEM's, which is not any person's, so users are read through `HKEY_USERS`.

use syntrac_core::{LaunchSource, LaunchSourceError, LauncherKind, RawEntry, SourceRead};

use crate::registry::{RegKey, Root};

const RUN: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run";
const RUN_ONCE: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\RunOnce";
const RUN_32: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Run";
const RUN_ONCE_32: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\RunOnce";
const PROFILE_LIST: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList";

pub struct RunKeyLaunchSource;

/// An account of a person (a domain or machine account), as opposed to the built-in service accounts, whose hives hold nothing a
/// person started. The `_Classes` hive of each user is not a profile.
fn is_person_sid(name: &str) -> bool {
    name.starts_with("S-1-5-21-") && !name.ends_with("_Classes")
}

/// The profile folder of a user, from the profile list (`%SystemDrive%\Users\name` with the variable expanded).
fn profile_folder(sid: &str) -> Option<String> {
    let key = RegKey::open(Root::LocalMachine, &format!(r"{PROFILE_LIST}\{sid}"))?;
    let raw = key.text("ProfileImagePath")?;
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_owned());
    let expanded = raw.replace("%SystemDrive%", &drive).replace("%systemdrive%", &drive);
    (!expanded.contains('%')).then_some(expanded)
}

fn push_values(
    read: &mut SourceRead,
    subkey_root: (Root, &str),
    kind: LauncherKind,
    place: &str,
    profile: Option<&str>,
) {
    let Some(key) = RegKey::open(subkey_root.0, subkey_root.1) else { return };
    for (name, value) in key.text_values() {
        if value.is_empty() {
            continue;
        }
        read.entries.push(RawEntry {
            kind,
            location: format!(r"{place}\{name}"),
            command: value,
            detail: String::new(),
            profile: profile.map(str::to_owned),
        });
    }
}

impl LaunchSource for RunKeyLaunchSource {
    fn name(&self) -> &'static str {
        "Run keys"
    }

    fn kinds(&self) -> &'static [LauncherKind] {
        &[LauncherKind::RunKey, LauncherKind::RunOnce]
    }

    fn read(&self) -> Result<SourceRead, LaunchSourceError> {
        // The machine's keys must be readable for the sweep to mean anything; a missing key is simply empty.
        if RegKey::open(Root::LocalMachine, r"SOFTWARE\Microsoft\Windows\CurrentVersion").is_none() {
            return Err(LaunchSourceError("the machine's registry could not be read".to_owned()));
        }
        let mut read = SourceRead::default();
        for (subkey, kind, place) in [
            (RUN, LauncherKind::RunKey, r"HKLM\Run"),
            (RUN_ONCE, LauncherKind::RunOnce, r"HKLM\RunOnce"),
            (RUN_32, LauncherKind::RunKey, r"HKLM\Run (32-bit)"),
            (RUN_ONCE_32, LauncherKind::RunOnce, r"HKLM\RunOnce (32-bit)"),
        ] {
            push_values(&mut read, (Root::LocalMachine, subkey), kind, place, None);
        }
        if let Some(users) = RegKey::open(Root::Users, "") {
            for sid in users.subkeys().into_iter().filter(|name| is_person_sid(name)) {
                let Some(profile) = profile_folder(&sid) else {
                    read.unreadable += 1;
                    continue;
                };
                for (subkey, kind, place) in
                    [(RUN, LauncherKind::RunKey, "Run"), (RUN_ONCE, LauncherKind::RunOnce, "RunOnce")]
                {
                    let place = format!(r"HKU\{sid}\{place}");
                    push_values(&mut read, (Root::Users, &format!(r"{sid}\{subkey}")), kind, &place, Some(&profile));
                }
            }
        }
        Ok(read)
    }
}
