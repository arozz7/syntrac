//! `IdentityRepository` for the in-memory repository: the reference the SQLite one must agree with.

use syntrac_core::rollup::executable_key;
use syntrac_core::{Executable, IdentityEntry, IdentityRecord, IdentityRepository, RepoError};

use crate::memory::InMemoryRepository;

impl IdentityRepository for InMemoryRepository {
    fn identity_entries(&self) -> Result<Vec<IdentityEntry>, RepoError> {
        let mut entries: Vec<IdentityEntry> = self
            .rollups
            .known_executables()
            .map(|(key, path)| IdentityEntry {
                exe: Executable { key: key.clone(), path: path.clone() },
                seen: self.rollups.executable_ledger_of(key),
                record: self.rollups.identity_of(key).cloned(),
            })
            .collect();
        entries.sort_by(|a, b| a.exe.key.cmp(&b.exe.key));
        Ok(entries)
    }

    fn save_identity(&mut self, record: &IdentityRecord) -> Result<(), RepoError> {
        if !self.rollups.save_identity(record) {
            return Err(RepoError::NotFound);
        }
        Ok(())
    }

    fn identity(&self, path: &str) -> Result<Option<IdentityRecord>, RepoError> {
        Ok(executable_key(path).and_then(|key| self.rollups.identity_of(&key).cloned()))
    }
}
