//! Scheduled tasks as launch records (I4b): the task definition files under `System32\Tasks`, read only. The service runs as
//! SYSTEM and can read them; an unelevated reader mostly cannot, which is not an error here, just fewer entries.
//!
//! A task that is switched off in its settings starts nothing and is left out (so if one is switched off later it shows as "no
//! longer configured"). A task that only runs a COM handler has no program path to map and is left out too.

use std::path::{Path, PathBuf};

use syntrac_core::task_xml::parse_task;
use syntrac_core::{LaunchSource, LaunchSourceError, LauncherKind, RawEntry, SourceRead};

/// A task file larger than this is not read (real ones are a few KB).
const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MAX_DEPTH: usize = 8;
const MAX_FILES: usize = 5_000;

pub struct TaskLaunchSource {
    root: PathBuf,
}

impl TaskLaunchSource {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// The Task Scheduler's own folder on this machine.
    pub fn for_this_machine() -> Self {
        let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_owned());
        Self::new(PathBuf::from(system_root).join(r"System32\Tasks"))
    }
}

/// The text of a task file: UTF-16 (the usual form, with a byte-order mark) or UTF-8.
fn decode(bytes: &[u8]) -> Option<String> {
    let units = |little: bool, body: &[u8]| -> Option<String> {
        if body.len() % 2 != 0 {
            return None;
        }
        let units: Vec<u16> =
            body.chunks_exact(2)
                .map(|pair| {
                    if little {
                        u16::from_le_bytes([pair[0], pair[1]])
                    } else {
                        u16::from_be_bytes([pair[0], pair[1]])
                    }
                })
                .collect();
        String::from_utf16(&units).ok()
    };
    match bytes {
        [0xFF, 0xFE, rest @ ..] => units(true, rest),
        [0xFE, 0xFF, rest @ ..] => units(false, rest),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8(rest.to_vec()).ok(),
        other => String::from_utf8(other.to_vec()).ok(),
    }
}

fn walk(folder: &Path, depth: usize, files: &mut Vec<PathBuf>, unreadable: &mut u32) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        *unreadable += 1;
        return;
    };
    for entry in entries.flatten() {
        if files.len() >= MAX_FILES {
            return;
        }
        let path = entry.path();
        match entry.file_type() {
            Ok(kind) if kind.is_dir() && depth < MAX_DEPTH => walk(&path, depth + 1, files, unreadable),
            Ok(kind) if kind.is_file() => files.push(path),
            _ => {}
        }
    }
}

impl LaunchSource for TaskLaunchSource {
    fn name(&self) -> &'static str {
        "scheduled tasks"
    }

    fn kinds(&self) -> &'static [LauncherKind] {
        &[LauncherKind::Task]
    }

    fn read(&self) -> Result<SourceRead, LaunchSourceError> {
        // A folder that cannot be listed is a failed read, not "no tasks": an empty answer would mark every task as gone.
        if let Err(error) = std::fs::read_dir(&self.root) {
            return Err(LaunchSourceError(format!("the task folder {} cannot be read ({error})", self.root.display())));
        }
        let mut read = SourceRead::default();
        let mut files = Vec::new();
        walk(&self.root, 0, &mut files, &mut read.unreadable);
        for file in files {
            let task = std::fs::metadata(&file)
                .ok()
                .filter(|meta| meta.len() <= MAX_FILE_BYTES)
                .and_then(|_| std::fs::read(&file).ok())
                .and_then(|bytes| decode(&bytes))
                .and_then(|text| parse_task(&text));
            let Some(task) = task else {
                read.unreadable += 1;
                continue;
            };
            if !task.enabled {
                continue;
            }
            let relative = file.strip_prefix(&self.root).unwrap_or(&file);
            let location = format!(r"\{}", relative.to_string_lossy().replace('/', r"\"));
            let detail = task.triggers.join(", ");
            for action in task.actions {
                let command = if action.arguments.is_empty() {
                    action.command
                } else {
                    // The arguments are kept apart from the program by the resolver, so keep the command as written.
                    format!("{} {}", action.command, action.arguments)
                };
                read.entries.push(RawEntry {
                    kind: LauncherKind::Task,
                    location: location.clone(),
                    command,
                    detail: detail.clone(),
                    profile: None,
                });
            }
        }
        Ok(read)
    }
}
