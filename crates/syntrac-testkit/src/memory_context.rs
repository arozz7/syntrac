//! `ContextRepository` for the in-memory repository: the reference the SQLite one must agree with.

use std::collections::BTreeMap;

use syntrac_core::rollup::executable_key;
use syntrac_core::{
    AccountSeen, ContextObservation, ContextRepository, ContextWrite, Executable, ExecutableContext, RepoError,
    ServiceSeen, Timestamp,
};

use crate::memory::InMemoryRepository;

#[derive(Debug, Default)]
struct Entry {
    accounts: BTreeMap<String, AccountSeen>,
    services: BTreeMap<String, ServiceSeen>,
}

/// How each program has been seen running, by executable key. Permanent, like the ledgers.
#[derive(Debug, Default)]
pub(crate) struct ContextState {
    by_exe: BTreeMap<String, Entry>,
}

fn context_of_entry(entry: &Entry) -> ExecutableContext {
    let mut accounts: Vec<AccountSeen> = entry.accounts.values().cloned().collect();
    accounts.sort_by(|a, b| b.last_seen.cmp(&a.last_seen).then_with(|| a.sid.cmp(&b.sid)));
    let mut services: Vec<ServiceSeen> = entry.services.values().cloned().collect();
    services.sort_by(|a, b| b.last_seen.cmp(&a.last_seen).then_with(|| a.name.cmp(&b.name)));
    ExecutableContext { accounts, services }
}

impl ContextRepository for InMemoryRepository {
    fn record_context(
        &mut self,
        observations: &[ContextObservation],
        at: Timestamp,
    ) -> Result<ContextWrite, RepoError> {
        let mut write = ContextWrite::default();
        for observation in observations {
            // The program joins the executable table with the spelling first seen, like the rollups do.
            self.rollups.paths.entry(observation.exe.key.clone()).or_insert_with(|| observation.exe.path.clone());
            let entry = self.rollups.contexts.by_exe.entry(observation.exe.key.clone()).or_default();

            match entry.accounts.get_mut(&observation.sid) {
                Some(seen) => {
                    seen.first_seen = seen.first_seen.min(at);
                    seen.last_seen = seen.last_seen.max(at);
                    if observation.account_name.is_some() {
                        seen.name.clone_from(&observation.account_name);
                    }
                    seen.kind = observation.kind;
                }
                None => {
                    write.new_accounts += 1;
                    entry.accounts.insert(
                        observation.sid.clone(),
                        AccountSeen {
                            sid: observation.sid.clone(),
                            name: observation.account_name.clone(),
                            kind: observation.kind,
                            first_seen: at,
                            last_seen: at,
                        },
                    );
                }
            }
            for service in &observation.services {
                match entry.services.get_mut(&service.name) {
                    Some(seen) => {
                        seen.first_seen = seen.first_seen.min(at);
                        seen.last_seen = seen.last_seen.max(at);
                        if service.display_name.is_some() {
                            seen.display_name.clone_from(&service.display_name);
                        }
                    }
                    None => {
                        write.new_services += 1;
                        entry.services.insert(
                            service.name.clone(),
                            ServiceSeen {
                                name: service.name.clone(),
                                display_name: service.display_name.clone(),
                                first_seen: at,
                                last_seen: at,
                            },
                        );
                    }
                }
            }
        }
        Ok(write)
    }

    fn context_of(&self, path: &str) -> Result<ExecutableContext, RepoError> {
        Ok(executable_key(path)
            .and_then(|key| self.rollups.contexts.by_exe.get(&key))
            .map(context_of_entry)
            .unwrap_or_default())
    }

    fn all_contexts(&self) -> Result<Vec<(Executable, ExecutableContext)>, RepoError> {
        Ok(self
            .rollups
            .contexts
            .by_exe
            .iter()
            .map(|(key, entry)| {
                let path = self.rollups.paths.get(key).cloned().unwrap_or_else(|| key.clone());
                (Executable { key: key.clone(), path }, context_of_entry(entry))
            })
            .collect())
    }
}
