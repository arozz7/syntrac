//! The app's small settings file (docs/ALERTS-PLAN.md, slice T1): whether notifications are on, and the newest finding already announced.
//! It lives next to the other per-user data (`%LOCALAPPDATA%\Syntrac\app.json`). A missing or damaged file means the defaults, and a file
//! that cannot be written never stops anything: the values are kept in memory for the rest of the run.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use syntrac_core::llm::AiSettings;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// Windows notifications for new suspicious findings.
    pub notifications: bool,
    /// The highest finding id already announced (or seen while notifications were off). `None` until the very first run.
    pub last_announced: Option<i64>,
    /// The optional AI help (docs/PHASE4-PLAN.md): off and empty until the user sets it up.
    pub ai: AiSettings,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { notifications: true, last_announced: None, ai: AiSettings::default() }
    }
}

impl Prefs {
    /// The saved settings, or the defaults when the file is missing or cannot be understood.
    pub fn load(path: &Path) -> Self {
        // A byte-order mark (written by Notepad and some scripts) is not part of the JSON and must not reset every setting.
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(text.trim_start_matches('\u{feff}')).ok())
            .unwrap_or_default()
    }

    /// Writes the settings (creating the folder), through a temporary file so a crash never leaves half a file.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, text)?;
        std::fs::rename(&temporary, path)
    }
}

/// Where the file lives, from `LOCALAPPDATA`; `None` when that is not known (then nothing is remembered between runs).
pub fn prefs_path(get: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let local = get("LOCALAPPDATA").map(|value| value.trim().to_owned()).filter(|value| !value.is_empty())?;
    Some(PathBuf::from(local).join("Syntrac").join("app.json"))
}

struct State {
    prefs: Prefs,
    path: Option<PathBuf>,
}

/// The settings shared by the tray, the commands and the watcher. Every change is kept in memory first and then written.
#[derive(Clone)]
pub struct PrefsHandle {
    state: Arc<Mutex<State>>,
}

impl PrefsHandle {
    /// Settings that are never written (tests, or no known folder).
    pub fn in_memory(prefs: Prefs) -> Self {
        Self { state: Arc::new(Mutex::new(State { prefs, path: None })) }
    }

    /// Loads the file (the defaults when there is none) and writes every later change to it.
    pub fn with_file(path: PathBuf) -> Self {
        Self { state: Arc::new(Mutex::new(State { prefs: Prefs::load(&path), path: Some(path) })) }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // A panic while the lock was held cannot leave the settings half-changed (each change is one assignment), so keep going.
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn change(&self, apply: impl FnOnce(&mut Prefs)) {
        let mut state = self.lock();
        apply(&mut state.prefs);
        if let Some(path) = &state.path {
            // A file that cannot be written must not stop the program: the value stays in memory for this run.
            let _ = state.prefs.save(path);
        }
    }

    pub fn notifications_on(&self) -> bool {
        self.lock().prefs.notifications
    }

    pub fn set_notifications_on(&self, on: bool) {
        self.change(|prefs| prefs.notifications = on);
    }

    pub fn last_announced(&self) -> Option<i64> {
        self.lock().prefs.last_announced
    }

    pub fn set_last_announced(&self, id: i64) {
        self.change(|prefs| prefs.last_announced = Some(id));
    }

    pub fn ai_settings(&self) -> AiSettings {
        self.lock().prefs.ai.clone()
    }

    pub fn set_ai_settings(&self, settings: AiSettings) {
        self.change(|prefs| prefs.ai = settings);
    }
}
