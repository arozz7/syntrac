//! Where an executable lives and which product it belongs to: PURE rules on the path (and, for the product, the signer).
//!
//! Location is a weak signal on its own (a rogue program can copy itself anywhere an account may write), but it is free, and
//! combined with the signature it separates "installed by an administrator" from "dropped into a user folder". These
//! functions only read the text of the path; nothing here touches the disk.

use crate::exe_identity::FileFacts;

/// A coarse class for the folder an executable runs from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LocationClass {
    /// `Windows` (including System32): writing needs administrator rights.
    System,
    /// `Program Files` and `Program Files (x86)`: writing needs administrator rights.
    ProgramFiles,
    /// `ProgramData`: shared; parts of it are writable by ordinary users.
    ProgramData,
    /// A user's profile: Downloads, AppData (outside Temp), tool folders. Writable by that user.
    UserProfile,
    /// A temporary folder (a user's `AppData\Local\Temp` or `Windows\Temp`). Installers run from here; so does unwanted software.
    Temporary,
    /// A network share.
    Network,
    /// Any other absolute path.
    Other,
    /// A bare name or a relative path: the path says nothing about location.
    Unknown,
}

impl LocationClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::ProgramFiles => "program_files",
            Self::ProgramData => "program_data",
            Self::UserProfile => "user_profile",
            Self::Temporary => "temporary",
            Self::Network => "network",
            Self::Other => "other",
            Self::Unknown => "unknown",
        }
    }

    /// The class named by `as_str`; `Unknown` is not one a rule can name (it says nothing about location).
    pub fn parse(text: &str) -> Option<Self> {
        [
            Self::System,
            Self::ProgramFiles,
            Self::ProgramData,
            Self::UserProfile,
            Self::Temporary,
            Self::Network,
            Self::Other,
        ]
        .into_iter()
        .find(|class| class.as_str() == text)
    }
}

/// The lower-cased folders of an absolute path, split into a root text and the folders between the root and the file name.
struct SplitPath {
    /// `c:`, `\\server\share` or `\device\harddiskvolume3`.
    root: String,
    network: bool,
    /// The folders only (the file name is dropped), lower-cased.
    folders: Vec<String>,
}

fn split(path: &str) -> Option<SplitPath> {
    let lowered = path.trim().to_lowercase().replace('/', "\\");
    let mut rest = lowered.as_str();
    let mut unc = false;
    if let Some(after) = rest.strip_prefix(r"\\?\unc\") {
        rest = after;
        unc = true;
    } else if let Some(after) = rest.strip_prefix(r"\\?\").or_else(|| rest.strip_prefix(r"\??\")) {
        rest = after;
    } else if rest.starts_with(r"\\") {
        rest = &rest[2..];
        unc = true;
    }

    let (root, network, mut names): (String, bool, Vec<&str>) = if unc {
        let mut parts = parts(rest);
        let (server, share) = (parts.next()?, parts.next()?);
        (format!(r"\\{server}\{share}"), true, parts.collect())
    } else if let Some(device) = rest.strip_prefix(r"\device\") {
        let mut parts = parts(device);
        let volume = parts.next().filter(|name| name.starts_with("harddiskvolume"))?;
        (format!(r"\device\{volume}"), false, parts.collect())
    } else {
        let mut parts = parts(rest);
        let mut drive = parts.next()?.chars();
        let letter = drive.next().filter(char::is_ascii_lowercase)?;
        if drive.as_str() != ":" {
            return None;
        }
        (format!("{letter}:"), false, parts.collect())
    };

    names.pop(); // the file name
    Some(SplitPath { root, network, folders: names.into_iter().map(str::to_owned).collect() })
}

fn parts(text: &str) -> impl Iterator<Item = &str> {
    text.split('\\').filter(|part| !part.is_empty())
}

fn is_temp(folders: &[String]) -> bool {
    let user_temp = folders.len() >= 5 && folders[0] == "users" && folders[2..5] == ["appdata", "local", "temp"];
    let windows_temp = folders.len() >= 2 && folders[0] == "windows" && folders[1] == "temp";
    user_temp || windows_temp
}

fn class_of(path: &SplitPath) -> LocationClass {
    if path.network {
        return LocationClass::Network;
    }
    if is_temp(&path.folders) {
        return LocationClass::Temporary;
    }
    match path.folders.first().map(String::as_str) {
        Some("windows") => LocationClass::System,
        Some("program files" | "program files (x86)") => LocationClass::ProgramFiles,
        Some("programdata") => LocationClass::ProgramData,
        Some("users" | "documents and settings") => LocationClass::UserProfile,
        _ => LocationClass::Other,
    }
}

pub fn classify_location(path: &str) -> LocationClass {
    split(path).map_or(LocationClass::Unknown, |split| class_of(&split))
}

/// How many folders below the root name the product: `Program Files\<Vendor or product>`, `Users\<name>\AppData\Local\<app>`.
fn product_depth(class: LocationClass, folders: &[String]) -> usize {
    match class {
        LocationClass::System | LocationClass::ProgramFiles | LocationClass::ProgramData => 2,
        LocationClass::Temporary if folders.first().is_some_and(|f| f == "windows") => 3,
        LocationClass::Temporary => 6,
        LocationClass::UserProfile => match (folders.get(2), folders.get(3), folders.get(4)) {
            (Some(appdata), Some(area), Some(programs))
                if appdata == "appdata" && area == "local" && programs == "programs" =>
            {
                6
            }
            (Some(appdata), Some(area), _)
                if appdata == "appdata" && matches!(area.as_str(), "local" | "roaming" | "locallow") =>
            {
                5
            }
            _ => 3,
        },
        LocationClass::Network => 1,
        LocationClass::Other | LocationClass::Unknown => 1,
    }
}

/// The folder that names the product an executable belongs to, lower-cased: everything deeper belongs to the same product.
/// `None` for a path that is not absolute.
pub fn install_root(path: &str) -> Option<String> {
    let split = split(path)?;
    let depth = product_depth(class_of(&split), &split.folders).min(split.folders.len());
    let folders = &split.folders[..depth];
    Some(if folders.is_empty() {
        format!(r"{}\", split.root)
    } else {
        format!(r"{}\{}", split.root, folders.join("\\"))
    })
}

/// What groups executables into one product: who made them (the signer, else the company in the version resource) and where
/// they are installed. An unsigned file beside signed ones has no publisher, so it is NOT folded into their product.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProductKey {
    pub publisher: Option<String>,
    pub root: String,
}

fn normalised(text: &Option<String>) -> Option<String> {
    text.as_deref().map(str::trim).filter(|t| !t.is_empty()).map(str::to_lowercase)
}

/// `None` when the path is not absolute (no root, no product). `facts` is `None` before the file has been inspected.
pub fn product_key(path: &str, facts: Option<&FileFacts>) -> Option<ProductKey> {
    let root = install_root(path)?;
    let publisher = facts.and_then(|f| normalised(&f.signature.signer).or_else(|| normalised(&f.version.company)));
    Some(ProductKey { publisher, root })
}
