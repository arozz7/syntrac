//! `InstallRepository` for the in-memory repository: the reference the SQLite one must agree with.

use std::collections::BTreeMap;

use syntrac_core::{InstallRecord, InstallRepository, InstallSeen, InstallWrite, RepoError, Timestamp};

use crate::memory::InMemoryRepository;

#[derive(Debug)]
struct Entry {
    record: InstallRecord,
    first_seen: Timestamp,
    last_seen: Timestamp,
}

/// Every record ever seen, by (hive, key name); and when the Uninstall keys were last read completely.
#[derive(Debug, Default)]
pub(crate) struct InstallState {
    entries: BTreeMap<(String, String), Entry>,
    completed_at: Option<Timestamp>,
}

impl InstallRepository for InMemoryRepository {
    fn record_installs(&mut self, rows: &[InstallRecord], at: Timestamp) -> Result<InstallWrite, RepoError> {
        let mut write = InstallWrite::default();
        for row in rows {
            write.written += 1;
            let key = (row.hive.clone(), row.key_name.clone());
            match self.installs.entries.get_mut(&key) {
                Some(entry) => {
                    entry.first_seen = entry.first_seen.min(at);
                    if at >= entry.last_seen {
                        entry.record = row.clone();
                    }
                    entry.last_seen = entry.last_seen.max(at);
                }
                None => {
                    write.new += 1;
                    self.installs.entries.insert(key, Entry { record: row.clone(), first_seen: at, last_seen: at });
                }
            }
        }
        let moved = self.installs.completed_at.map_or(at, |before| before.max(at));
        self.installs.completed_at = Some(moved);
        Ok(write)
    }

    fn installs(&self) -> Result<Vec<InstallSeen>, RepoError> {
        let completed = self.installs.completed_at;
        Ok(self
            .installs
            .entries
            .values()
            .map(|entry| InstallSeen {
                record: entry.record.clone(),
                first_seen: entry.first_seen,
                last_seen: entry.last_seen,
                current: completed.is_some_and(|done| entry.last_seen >= done),
            })
            .collect())
    }
}
