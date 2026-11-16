//! The environment and the file system as the launch-record resolver sees them (I4b).
//!
//! The service runs as SYSTEM, so the variables that name one person's folders would name SYSTEM's, which is wrong for every entry
//! that belongs to a person. Those are refused here: an entry that belongs to a user gets that user's folders from its profile
//! (`syntrac_core::launch_run`), and a machine-wide entry that uses one cannot be mapped and is counted as unresolved.

/// Variables that name the current user's folders: never taken from the service's own environment.
const PER_USER: [&str; 9] =
    ["userprofile", "appdata", "localappdata", "homepath", "homedrive", "homeshare", "temp", "tmp", "username"];

/// The value of an environment variable of the service, except the per-user ones.
pub fn environment(name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    if PER_USER.contains(&lower.as_str()) {
        return None;
    }
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

/// Whether a program file is there.
pub fn file_exists(path: &str) -> bool {
    std::path::Path::new(path).is_file()
}
