//! Start with Windows (docs/ALERTS-PLAN.md, slice T2). The rules are pure and tested; the real store is `autostart_registry`.
//!
//! The program starts at sign-in through ONE value in the signed-in user's own Run key (no administrator rights, nothing for other
//! users, no service, no scheduled task): `"<this program>" --tray`, which starts it with no window, only the tray icon. It is OFF until
//! the user switches it on in the tray menu, and switching it off deletes the value.

use std::path::{Path, PathBuf};

/// The name of the value in the Run key.
pub const VALUE_NAME: &str = "SYNTRAC";

/// The flag that starts the program with no window.
pub const TRAY_FLAG: &str = "--tray";

/// How the program was started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartMode {
    /// The usual: the window opens.
    Window,
    /// `--tray`: only the tray icon (what start with Windows uses).
    Tray,
}

impl StartMode {
    pub fn from_args(args: impl IntoIterator<Item = String>) -> Self {
        if args.into_iter().skip(1).any(|arg| arg.eq_ignore_ascii_case(TRAY_FLAG)) {
            Self::Tray
        } else {
            Self::Window
        }
    }
}

/// The command to register for `exe`: the quoted path and the tray flag. `None` for a path that cannot be quoted safely (empty, or
/// containing a quote character).
pub fn autostart_command(exe: &Path) -> Option<String> {
    let text = exe.to_str()?;
    if text.trim().is_empty() || text.contains('"') {
        return None;
    }
    Some(format!("\"{text}\" {TRAY_FLAG}"))
}

/// The program named at the start of a registered command (quoted, or up to the first space).
pub fn exe_of_command(command: &str) -> Option<PathBuf> {
    let command = command.trim();
    let path = match command.strip_prefix('"') {
        Some(rest) => rest.split('"').next()?,
        None => command.split_whitespace().next()?,
    };
    (!path.is_empty()).then(|| PathBuf::from(path))
}

/// Where the registered command is kept. Every method says why it failed; "not there" is `Ok(None)` / `Ok(())`, never an error.
pub trait AutostartStore {
    fn registered(&self) -> Result<Option<String>, String>;
    fn register(&self, command: &str) -> Result<(), String>;
    fn unregister(&self) -> Result<(), String>;
}

/// Whether the program is registered to start with Windows. A value that cannot be read counts as off.
pub fn is_enabled(store: &dyn AutostartStore) -> bool {
    matches!(store.registered(), Ok(Some(_)))
}

/// Switches it on (registering THIS program) or off (removing the value). Returns what is true afterwards.
pub fn set_enabled(store: &dyn AutostartStore, exe: &Path, on: bool) -> Result<bool, String> {
    if on {
        let command =
            autostart_command(exe).ok_or_else(|| "This program's path cannot be registered safely.".to_owned())?;
        store.register(&command)?;
    } else {
        store.unregister()?;
    }
    Ok(is_enabled(store))
}

/// When it is on but the registered program no longer exists (the program was moved or rebuilt elsewhere), points it at THIS program, so
/// it keeps working. Never switches anything on, and never touches an entry that names another program that still exists (a copy the
/// user chose). Returns whether it rewrote the entry.
pub fn repair(store: &dyn AutostartStore, this_exe: &Path, exists: impl Fn(&Path) -> bool) -> bool {
    let Ok(Some(registered)) = store.registered() else { return false };
    let Some(command) = autostart_command(this_exe) else { return false };
    if registered == command {
        return false;
    }
    let stale = exe_of_command(&registered).is_none_or(|registered_exe| !exists(&registered_exe));
    stale && store.register(&command).is_ok()
}
