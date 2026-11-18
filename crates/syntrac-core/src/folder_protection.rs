//! Which folders an ordinary program cannot write to, and which writable folders may be a "build folder"
//! (docs/ALERTS-PLAN.md, F6). PURE rules on the text of a path: nothing here touches the disk.
//!
//! Why it matters: "always expected for the whole folder" silences the "new program" finding for everything that ever appears below
//! a folder. For a folder a program can write to, a rogue program only has to copy itself there. So a whole-folder expectation is
//! only allowed for a PROTECTED folder, and the one exception for developer folders (a "build folder") has its own narrow rules.

use crate::exe_location::{classify_location, root_and_folders, LocationClass};

/// Folders below the Windows folder that ordinary users can still write to, lower case, relative to `Windows`. Found with read-only
/// `icacls` checks on a real machine and from the usual list of such places; a folder that cannot be read is listed too (safe: it
/// only makes SYNTRAC cover less).
const WRITABLE_INSIDE_WINDOWS: &[&[&str]] = &[
    &["temp"],
    &["tasks"],
    &["tracing"],
    &["debug", "wia"],
    &["registration", "crmlog"],
    &["system32", "tasks"],
    &["system32", "com", "dmp"],
    &["system32", "fxstmp"],
    &["system32", "logfiles", "scm"],
    &["system32", "microsoft", "crypto", "rsa", "machinekeys"],
    &["system32", "spool", "printers"],
    &["system32", "spool", "servers"],
    &["system32", "spool", "drivers", "color"],
    &["syswow64", "tasks"],
    &["syswow64", "com", "dmp"],
    &["syswow64", "fxstmp"],
];

/// Names of folders that hold whatever anyone drops there. A build folder is never one of these.
const TOO_GENERAL: &[&str] = &[
    "downloads",
    "desktop",
    "documents",
    "pictures",
    "music",
    "videos",
    "public",
    "temp",
    "tmp",
    "programdata",
    "appdata",
    "users",
    "windows",
    "program files",
    "program files (x86)",
];

/// The AppData folders themselves (a program's own folder inside them is fine).
const APPDATA_AREAS: &[&str] = &["local", "roaming", "locallow"];

/// Folders directly below an AppData area or ProgramData that every vendor shares: the Startup folder lives under `Microsoft`, and
/// store apps under `Packages`. Dropped programs hide here, so none is a build folder.
const SHARED_VENDOR_FOLDERS: &[&str] = &["microsoft", "packages"];

fn is_inside(folders: &[String], below: &[&str]) -> bool {
    folders.len() >= below.len() && folders.iter().zip(below).all(|(folder, name)| folder == name)
}

/// Whether a path is under Windows or Program Files (any drive) and not in a folder users can write to anyway. Used for MATCHING an
/// expectation, where the drive does not matter.
pub fn is_protected_path(path: &str) -> bool {
    let Some((_, folders)) = root_and_folders(path) else { return false };
    match classify_location(path) {
        LocationClass::ProgramFiles => true,
        LocationClass::System => {
            let below_windows = folders.get(1..).unwrap_or_default();
            !WRITABLE_INSIDE_WINDOWS.iter().any(|writable| is_inside(below_windows, writable))
        }
        _ => false,
    }
}

/// `c:` for `C:`, `c` or `C:\`; `None` for anything that is not a drive.
fn drive_root(text: &str) -> Option<String> {
    let mut chars = text.trim().chars();
    let letter = chars.next().filter(char::is_ascii_alphabetic)?.to_ascii_lowercase();
    matches!(chars.as_str(), "" | ":" | ":\\" | ":/").then(|| format!("{letter}:"))
}

fn is_on_drive(path: &str, system_drive: &str) -> bool {
    match (root_and_folders(path), drive_root(system_drive)) {
        (Some((root, _)), Some(drive)) => root == drive,
        _ => false,
    }
}

/// Protected AND on the Windows drive: what may be turned into a whole-folder expectation. A data drive's `Program Files` follows its
/// own permissions (the default on a second drive lets every signed-in user modify it), which the path cannot show.
pub fn is_protected_on_system_drive(path: &str, system_drive: &str) -> bool {
    is_protected_path(path) && is_on_drive(path, system_drive)
}

/// The same for a FOLDER (a file inside it is judged).
pub fn is_protected_folder_on_system_drive(folder: &str, system_drive: &str) -> bool {
    is_protected_on_system_drive(&in_folder(folder), system_drive)
}

/// Whether a FOLDER is protected, on any drive (for matching).
pub fn is_protected_folder(folder: &str) -> bool {
    is_protected_path(&in_folder(folder))
}

fn in_folder(folder: &str) -> String {
    format!(r"{}\x", folder.trim().trim_end_matches(['\\', '/']))
}

/// The Windows drive of this machine (`SystemDrive`), `c:` when it cannot be read.
pub fn system_drive() -> String {
    std::env::var("SystemDrive").ok().and_then(|text| drive_root(&text)).unwrap_or_else(|| "c:".to_owned())
}

/// Why `root` may not be a "build folder", or `None` when it may. A build folder is a writable tool or project folder the user chose
/// ON PURPOSE for the "new program" finding only. It is never a place where things get dropped (Temp, Downloads, Desktop,
/// Documents, a profile, AppData itself, ProgramData itself), never a drive, a share, the Windows folder or a protected folder (those
/// have the whole-folder choice).
pub fn build_folder_problem(root: &str, system_drive: &str) -> Option<&'static str> {
    let Some((_, folders)) = root_and_folders(&in_folder(root)) else {
        return Some("That is not an absolute folder path.");
    };
    let Some(last) = folders.last() else { return Some("A whole drive cannot be a build folder.") };
    match classify_location(&in_folder(root)) {
        LocationClass::Temporary => return Some("A temporary folder cannot be a build folder."),
        LocationClass::Network => return Some("A network share cannot be a build folder."),
        LocationClass::System => return Some("The Windows folder cannot be a build folder."),
        LocationClass::ProgramFiles if is_protected_folder_on_system_drive(root, system_drive) => {
            return Some("A protected folder has the whole-folder choice; it is not a build folder.");
        }
        LocationClass::Unknown => return Some("That is not an absolute folder path."),
        LocationClass::UserProfile if folders.len() < 3 => {
            return Some("A profile's own folder cannot be a build folder.");
        }
        _ => {}
    }
    let previous = folders.len().checked_sub(2).and_then(|index| folders.get(index));
    let appdata_area = previous.is_some_and(|name| name == "appdata") && APPDATA_AREAS.contains(&last.as_str());
    let shared_vendor = SHARED_VENDOR_FOLDERS.contains(&last.as_str())
        && previous.is_some_and(|name| name == "programdata" || APPDATA_AREAS.contains(&name.as_str()));
    if TOO_GENERAL.contains(&last.as_str()) || appdata_area || shared_vendor {
        return Some("That folder is too general: anything could be dropped there.");
    }
    None
}
