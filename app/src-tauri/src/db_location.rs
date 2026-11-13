//! Where the database is. The data may be moved to another drive, so the app follows what the running collector
//! reports over its pipe, remembers it for when the collector is offline, and never overrides a path the developer
//! chose explicitly (`SYNTRAC_DB`).
//!
//! A reported path is only believed if it is plausible: absolute and a `.db` file. The pipe is created by the
//! collector service, but the app should not open an arbitrary path just because something answered on it.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Somewhere to keep the last known database path between runs. Best effort: failing to remember is harmless.
pub trait PathMemory: Send + Sync {
    fn recall(&self) -> Option<PathBuf>;
    fn remember(&self, path: &Path);
}

/// Remembers nothing (explicit paths, and tests).
pub struct NoMemory;

impl PathMemory for NoMemory {
    fn recall(&self) -> Option<PathBuf> {
        None
    }
    fn remember(&self, _path: &Path) {}
}

/// The last known path, as one line of text in a small file in the user's own local app data.
pub struct FileMemory {
    file: PathBuf,
}

impl FileMemory {
    pub fn new(file: PathBuf) -> Self {
        Self { file }
    }
}

impl PathMemory for FileMemory {
    fn recall(&self) -> Option<PathBuf> {
        let text = std::fs::read_to_string(&self.file).ok()?;
        let line = text.trim();
        (!line.is_empty()).then(|| PathBuf::from(line))
    }

    fn remember(&self, path: &Path) {
        if let Some(folder) = self.file.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        let _ = std::fs::write(&self.file, path.display().to_string());
    }
}

pub struct DbLocation {
    pinned: bool,
    current: Mutex<PathBuf>,
    memory: Box<dyn PathMemory>,
}

impl DbLocation {
    /// `initial` is the explicit path when `pinned`, otherwise the default. Unless pinned, a remembered path replaces it.
    pub fn new(initial: PathBuf, pinned: bool, memory: Box<dyn PathMemory>) -> Self {
        let start = if pinned { initial } else { memory.recall().unwrap_or(initial) };
        Self { pinned, current: Mutex::new(start), memory }
    }

    pub fn current(&self) -> PathBuf {
        self.current.lock().map(|path| path.clone()).unwrap_or_default()
    }

    /// Takes note of the path the collector reports. Returns whether the location changed.
    pub fn learn(&self, reported: &str) -> bool {
        if self.pinned {
            return false;
        }
        let Some(path) = plausible_database_path(reported) else {
            return false;
        };
        let Ok(mut current) = self.current.lock() else {
            return false;
        };
        if same_path(&current, &path) {
            return false;
        }
        self.memory.remember(&path);
        *current = path;
        true
    }
}

/// Absolute, a file name ending in `.db`, no parent-directory tricks.
fn plausible_database_path(text: &str) -> Option<PathBuf> {
    let text = text.trim();
    let path = PathBuf::from(text);
    let is_db_file = path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("db"));
    let has_parent_steps = path.components().any(|part| matches!(part, std::path::Component::ParentDir));
    (path.is_absolute() && is_db_file && !has_parent_steps).then_some(path)
}

/// Windows paths compare ignoring case and the difference between `/` and `\`.
fn same_path(first: &Path, second: &Path) -> bool {
    let normal = |path: &Path| path.display().to_string().replace('/', "\\").to_ascii_lowercase();
    normal(first) == normal(second)
}
