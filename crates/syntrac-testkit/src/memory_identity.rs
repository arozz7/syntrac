//! `IdentityRepository` for the in-memory repository: the reference the SQLite one must agree with.

use syntrac_core::rollup::executable_key;
use syntrac_core::{
    ChangeFacts, Executable, FileFacts, IdentityChange, IdentityChangeRepository, IdentityEntry, IdentityRecord,
    IdentityRepository, RepoError, Timestamp,
};

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
        // New CONTENT is a change worth remembering (docs/ALERTS-PLAN.md, F2): both sides must have been hashed.
        let change = self.rollups.identity_of(&record.exe.key).and_then(|old| {
            let old_facts = old.facts.as_ref()?;
            let new_facts = record.facts.as_ref()?;
            let (old_sha, new_sha) = (old_facts.sha256.as_ref()?, new_facts.sha256.as_ref()?);
            (old_sha != new_sha).then(|| IdentityChange {
                exe: record.exe.clone(),
                old: change_facts(old_facts),
                new: change_facts(new_facts),
                at: record.inspected_at.unwrap_or(record.last_attempt),
            })
        });
        if !self.rollups.save_identity(record) {
            return Err(RepoError::NotFound);
        }
        if let Some(change) = change {
            self.findings.identity_changes.push(change);
        }
        Ok(())
    }

    fn identity(&self, path: &str) -> Result<Option<IdentityRecord>, RepoError> {
        Ok(executable_key(path).and_then(|key| self.rollups.identity_of(&key).cloned()))
    }
}

fn change_facts(facts: &FileFacts) -> ChangeFacts {
    ChangeFacts {
        sha256: facts.sha256.clone(),
        signer: facts.signature.signer.clone(),
        status: Some(facts.signature.status),
    }
}

impl IdentityChangeRepository for InMemoryRepository {
    fn identity_changes_since(&self, since: Timestamp) -> Result<Vec<IdentityChange>, RepoError> {
        let mut list: Vec<IdentityChange> =
            self.findings.identity_changes.iter().filter(|change| change.at >= since).cloned().collect();
        list.sort_by_key(|change| change.at);
        Ok(list)
    }
}
