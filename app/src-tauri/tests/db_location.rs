//! Where the database is can change (the data may be moved to another drive). The UI follows what the running service
//! reports, remembers it for when the service is offline, and never overrides a path the developer chose explicitly.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use syntrac_app_lib::db_location::{DbLocation, FileMemory, PathMemory};

/// A memory that lives in the test, and counts how often it was written.
#[derive(Clone, Default)]
struct FakeMemory {
    stored: Arc<Mutex<Option<PathBuf>>>,
    writes: Arc<Mutex<u32>>,
}

impl FakeMemory {
    fn holding(path: &str) -> Self {
        let memory = Self::default();
        *memory.stored.lock().unwrap() = Some(PathBuf::from(path));
        memory
    }
    fn writes(&self) -> u32 {
        *self.writes.lock().unwrap()
    }
    fn stored(&self) -> Option<PathBuf> {
        self.stored.lock().unwrap().clone()
    }
}

impl PathMemory for FakeMemory {
    fn recall(&self) -> Option<PathBuf> {
        self.stored.lock().unwrap().clone()
    }
    fn remember(&self, path: &Path) {
        *self.stored.lock().unwrap() = Some(path.to_path_buf());
        *self.writes.lock().unwrap() += 1;
    }
}

const DEFAULT: &str = r"C:\ProgramData\Syntrac\data\syntrac.db";

fn location(pinned: bool, memory: &FakeMemory) -> DbLocation {
    DbLocation::new(PathBuf::from(DEFAULT), pinned, Box::new(memory.clone()))
}

#[test]
fn with_nothing_remembered_the_default_is_used() {
    assert_eq!(location(false, &FakeMemory::default()).current(), PathBuf::from(DEFAULT));
}

#[test]
fn a_remembered_path_wins_over_the_default() {
    let memory = FakeMemory::holding(r"D:\Syntrac\data\syntrac.db");
    assert_eq!(location(false, &memory).current(), PathBuf::from(r"D:\Syntrac\data\syntrac.db"));
}

#[test]
fn a_pinned_path_ignores_both_the_memory_and_the_service() {
    let memory = FakeMemory::holding(r"D:\Syntrac\data\syntrac.db");
    let pinned = location(true, &memory);
    assert_eq!(pinned.current(), PathBuf::from(DEFAULT));
    assert!(!pinned.learn(r"E:\somewhere\else\syntrac.db"));
    assert_eq!(pinned.current(), PathBuf::from(DEFAULT));
    assert_eq!(memory.writes(), 0);
}

#[test]
fn the_path_the_service_reports_is_followed_and_remembered() {
    let memory = FakeMemory::default();
    let location = location(false, &memory);
    assert!(location.learn("D:/Syntrac/data/syntrac.db"), "a new path is a change");
    assert_eq!(location.current(), PathBuf::from("D:/Syntrac/data/syntrac.db"));
    assert_eq!(memory.stored(), Some(PathBuf::from("D:/Syntrac/data/syntrac.db")));
}

#[test]
fn hearing_the_same_path_again_changes_nothing_and_writes_nothing() {
    let memory = FakeMemory::default();
    let location = location(false, &memory);
    location.learn("D:/Syntrac/data/syntrac.db");
    assert!(!location.learn("D:/Syntrac/data/syntrac.db"));
    assert!(!location.learn(r"D:\Syntrac\data\syntrac.db"), "the same path spelled with backslashes is the same path");
    assert_eq!(memory.writes(), 1);
}

#[test]
fn a_path_equal_to_what_was_remembered_is_not_written_again() {
    let memory = FakeMemory::holding("D:/Syntrac/data/syntrac.db");
    let location = location(false, &memory);
    assert!(!location.learn("D:/Syntrac/data/syntrac.db"));
    assert_eq!(memory.writes(), 0);
}

#[test]
fn reports_that_are_not_a_plausible_database_path_are_ignored() {
    let memory = FakeMemory::default();
    let location = location(false, &memory);
    for bad in ["", "   ", "db", r"data\syntrac.db", "D:/Syntrac/data/notes.txt", "D:/Syntrac/data/", "../x/syntrac.db"]
    {
        assert!(!location.learn(bad), "{bad:?} must be ignored");
    }
    assert_eq!(location.current(), PathBuf::from(DEFAULT));
    assert_eq!(memory.writes(), 0);
}

#[test]
fn the_file_memory_round_trips_a_path() {
    let dir = tempfile::tempdir().unwrap();
    let memory = FileMemory::new(dir.path().join("Syntrac").join("database-path.txt"));
    assert_eq!(memory.recall(), None, "nothing remembered yet, and a missing folder is fine");
    memory.remember(Path::new("D:/Syntrac/data/syntrac.db"));
    assert_eq!(memory.recall(), Some(PathBuf::from("D:/Syntrac/data/syntrac.db")));
}

#[test]
fn the_file_memory_ignores_an_empty_or_unreadable_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("p.txt");
    std::fs::write(&file, "  \r\n").unwrap();
    assert_eq!(FileMemory::new(file.clone()).recall(), None);
    std::fs::write(&file, "D:/Syntrac/data/syntrac.db\r\n").unwrap();
    assert_eq!(
        FileMemory::new(file).recall(),
        Some(PathBuf::from("D:/Syntrac/data/syntrac.db")),
        "trailing line ends are trimmed"
    );
}

#[test]
fn a_file_memory_that_cannot_be_written_does_not_break_following_the_service() {
    let dir = tempfile::tempdir().unwrap();
    // The "folder" is a regular file, so the memory file can never be created under it.
    let blocker = dir.path().join("blocker");
    std::fs::write(&blocker, "x").unwrap();
    let location = DbLocation::new(
        PathBuf::from(DEFAULT),
        false,
        Box::new(FileMemory::new(blocker.join("Syntrac").join("database-path.txt"))),
    );
    assert!(location.learn("D:/Syntrac/data/syntrac.db"));
    assert_eq!(location.current(), PathBuf::from("D:/Syntrac/data/syntrac.db"));
}
