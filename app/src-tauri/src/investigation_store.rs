//! Phase 4, slice L4i-a (docs/PHASE4-PLAN.md): the answers of the AI help that were kept, so a program or finding that was investigated before does
//! not have to ask the model again. The page reads them through `InvestigationStore`; the real one is a small JSON file next to the settings file
//! (`%LOCALAPPDATA%\Syntrac\investigations.json`). A missing or damaged file means nothing is saved yet, and a file that cannot be written never stops
//! anything: the answers are kept in memory for the rest of the run.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use serde::{Deserialize, Serialize};

use crate::ai_investigate::SubjectRef;

/// How many answers are kept; the oldest are dropped first. They are short texts, so this is a generous bound, not a pressure.
const DEFAULT_LIMIT: usize = 500;

/// One kept answer, with what is needed to judge it later: which model gave it, at what detail, when, and exactly what was sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedInvestigation {
    pub subject: SubjectRef,
    pub answer: String,
    /// The answer was cut off at the model's length limit.
    pub truncated: bool,
    pub model: String,
    /// `summary` or `full`.
    pub level: String,
    /// The evidence block that was sent.
    pub sent: String,
    /// When it was saved, in milliseconds since 1970.
    pub saved_ms: i64,
}

/// That an answer exists for a subject and when it was saved, without the answer itself (for marking lists).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedMark {
    pub subject: SubjectRef,
    pub saved_ms: i64,
}

/// Where kept answers live. One answer per subject: saving again replaces it.
pub trait InvestigationStore: Send + Sync {
    fn get(&self, subject: &SubjectRef) -> Option<SavedInvestigation>;
    fn put(&self, entry: SavedInvestigation);
    fn forget(&self, subject: &SubjectRef);
    fn list(&self) -> Vec<SavedMark>;
}

/// What makes two subjects the same. A Windows path is the same program whatever its letter case.
fn key_of(subject: &SubjectRef) -> String {
    match subject {
        SubjectRef::Program { path } => format!("program:{}", path.to_lowercase()),
        SubjectRef::Finding { id } => format!("finding:{id}"),
    }
}

#[derive(Serialize, Deserialize, Default)]
struct FileContents {
    entries: Vec<SavedInvestigation>,
}

struct State {
    entries: Vec<SavedInvestigation>,
    path: Option<PathBuf>,
}

/// The kept answers, in memory and (when a path is given) in a JSON file written after every change.
pub struct JsonInvestigationStore {
    state: Mutex<State>,
    limit: usize,
}

impl JsonInvestigationStore {
    /// Answers that are never written (tests, or no known folder).
    pub fn in_memory() -> Self {
        Self { state: Mutex::new(State { entries: Vec::new(), path: None }), limit: DEFAULT_LIMIT }
    }

    /// Reads the file (nothing saved when there is none, or it cannot be understood) and writes every later change to it.
    pub fn with_file(path: PathBuf) -> Self {
        let entries = Self::read(&path);
        Self { state: Mutex::new(State { entries, path: Some(path) }), limit: DEFAULT_LIMIT }
    }

    /// Keeps at most this many answers (the newest).
    #[must_use]
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit.max(1);
        self
    }

    fn read(path: &Path) -> Vec<SavedInvestigation> {
        // A byte-order mark (written by Notepad and some scripts) is not part of the JSON.
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<FileContents>(text.trim_start_matches('\u{feff}')).ok())
            .map(|contents| contents.entries)
            .unwrap_or_default()
    }

    /// Writes through a temporary file so a crash never leaves half a file.
    fn write(path: &Path, entries: &[SavedInvestigation]) -> std::io::Result<()> {
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)?;
        }
        let text =
            serde_json::to_string_pretty(&FileContents { entries: entries.to_vec() }).map_err(std::io::Error::other)?;
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, text)?;
        std::fs::rename(&temporary, path)
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // Each change below is finished before the lock is let go, so a panic elsewhere leaves nothing half-done.
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn changed(state: &State) {
        if let Some(path) = &state.path {
            // A file that cannot be written must not stop the program: the answers stay in memory for this run.
            let _ = Self::write(path, &state.entries);
        }
    }
}

impl InvestigationStore for JsonInvestigationStore {
    fn get(&self, subject: &SubjectRef) -> Option<SavedInvestigation> {
        let key = key_of(subject);
        self.lock().entries.iter().find(|entry| key_of(&entry.subject) == key).cloned()
    }

    fn put(&self, entry: SavedInvestigation) {
        let key = key_of(&entry.subject);
        let mut state = self.lock();
        state.entries.retain(|existing| key_of(&existing.subject) != key);
        state.entries.push(entry);
        if state.entries.len() > self.limit {
            state.entries.sort_by_key(|kept| std::cmp::Reverse(kept.saved_ms));
            state.entries.truncate(self.limit);
        }
        Self::changed(&state);
    }

    fn forget(&self, subject: &SubjectRef) {
        let key = key_of(subject);
        let mut state = self.lock();
        let before = state.entries.len();
        state.entries.retain(|existing| key_of(&existing.subject) != key);
        if state.entries.len() != before {
            Self::changed(&state);
        }
    }

    fn list(&self) -> Vec<SavedMark> {
        self.lock()
            .entries
            .iter()
            .map(|entry| SavedMark { subject: entry.subject.clone(), saved_ms: entry.saved_ms })
            .collect()
    }
}

/// Where the file lives: next to the settings file.
pub fn investigations_path(settings_file: &Path) -> PathBuf {
    settings_file.with_file_name("investigations.json")
}
