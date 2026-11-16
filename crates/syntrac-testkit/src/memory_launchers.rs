//! `LauncherRepository` for the in-memory repository: the reference the SQLite one must agree with.

use std::collections::BTreeMap;

use syntrac_core::rollup::executable_key;
use syntrac_core::{
    Executable, LauncherKind, LauncherObservation, LauncherRepository, LauncherSeen, LauncherWrite, RepoError,
    Timestamp,
};

use crate::memory::InMemoryRepository;

#[derive(Debug)]
struct Entry {
    detail: String,
    arguments: String,
    first_seen: Timestamp,
    last_seen: Timestamp,
}

/// Every entry ever seen, by program key, kind and location; and when each kind was last read completely.
#[derive(Debug, Default)]
pub(crate) struct LauncherState {
    entries: BTreeMap<(String, LauncherKind, String), Entry>,
    sweeps: BTreeMap<LauncherKind, Timestamp>,
}

impl LauncherState {
    fn seen(&self, kind: LauncherKind, location: &str, entry: &Entry) -> LauncherSeen {
        LauncherSeen {
            kind,
            location: location.to_owned(),
            detail: entry.detail.clone(),
            arguments: entry.arguments.clone(),
            first_seen: entry.first_seen,
            last_seen: entry.last_seen,
            current: self.sweeps.get(&kind).is_some_and(|completed| entry.last_seen >= *completed),
        }
    }
}

fn newest_first(list: &mut [LauncherSeen]) {
    list.sort_by(|a, b| {
        b.last_seen.cmp(&a.last_seen).then_with(|| a.kind.cmp(&b.kind)).then_with(|| a.location.cmp(&b.location))
    });
}

impl LauncherRepository for InMemoryRepository {
    fn record_launchers(
        &mut self,
        rows: &[LauncherObservation],
        completed: &[LauncherKind],
        at: Timestamp,
    ) -> Result<LauncherWrite, RepoError> {
        let mut write = LauncherWrite::default();
        for row in rows {
            self.rollups.paths.entry(row.exe.key.clone()).or_insert_with(|| row.exe.path.clone());
            let key = (row.exe.key.clone(), row.kind, row.location.clone());
            write.written += 1;
            match self.launchers.entries.get_mut(&key) {
                Some(entry) => {
                    entry.first_seen = entry.first_seen.min(at);
                    entry.last_seen = entry.last_seen.max(at);
                    entry.detail.clone_from(&row.detail);
                    entry.arguments.clone_from(&row.arguments);
                }
                None => {
                    write.new += 1;
                    self.launchers.entries.insert(
                        key,
                        Entry {
                            detail: row.detail.clone(),
                            arguments: row.arguments.clone(),
                            first_seen: at,
                            last_seen: at,
                        },
                    );
                }
            }
        }
        for kind in completed {
            let moved = self.launchers.sweeps.get(kind).map_or(at, |before| (*before).max(at));
            self.launchers.sweeps.insert(*kind, moved);
        }
        Ok(write)
    }

    fn launchers_of(&self, path: &str) -> Result<Vec<LauncherSeen>, RepoError> {
        let Some(wanted) = executable_key(path) else { return Ok(Vec::new()) };
        let mut list: Vec<LauncherSeen> = self
            .launchers
            .entries
            .iter()
            .filter(|((key, _, _), _)| *key == wanted)
            .map(|((_, kind, location), entry)| self.launchers.seen(*kind, location, entry))
            .collect();
        newest_first(&mut list);
        Ok(list)
    }

    fn all_launchers(&self) -> Result<Vec<(Executable, Vec<LauncherSeen>)>, RepoError> {
        let mut by_key: BTreeMap<&String, Vec<LauncherSeen>> = BTreeMap::new();
        for ((key, kind, location), entry) in &self.launchers.entries {
            by_key.entry(key).or_default().push(self.launchers.seen(*kind, location, entry));
        }
        Ok(by_key
            .into_iter()
            .map(|(key, mut list)| {
                newest_first(&mut list);
                let path = self.rollups.paths.get(key).cloned().unwrap_or_else(|| key.clone());
                (Executable { key: key.clone(), path }, list)
            })
            .collect())
    }
}
