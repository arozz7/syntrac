//! `SpawnRepository` for the in-memory repository: the reference the SQLite one must agree with.

use std::collections::BTreeMap;

use syntrac_core::rollup::executable_key;
use syntrac_core::{Executable, RepoError, SpawnPair, SpawnRepository, Timestamp};

use crate::memory::InMemoryRepository;

fn file_name(path: &str) -> String {
    path.trim().rsplit('\\').next().unwrap_or_default().to_ascii_lowercase()
}

type Group = (String, String, u32, Timestamp, Timestamp);

impl SpawnRepository for InMemoryRepository {
    fn spawn_pairs(&self, since: Timestamp, child_names: &[String]) -> Result<Vec<SpawnPair>, RepoError> {
        let names: Vec<String> = child_names.iter().map(|name| name.to_ascii_lowercase()).collect();
        // (parent key, child key) -> (parent path, child path, count, first, last); the smallest spelling wins, as MIN() does.
        let mut groups: BTreeMap<(String, String), Group> = BTreeMap::new();
        for child in self.processes.values().filter(|p| p.started >= since && names.contains(&file_name(&p.image_path)))
        {
            let Some(parent) = child.parent_id.and_then(|id| self.processes.get(&id)) else { continue };
            let (Some(parent_key), Some(child_key)) =
                (executable_key(&parent.image_path), executable_key(&child.image_path))
            else {
                continue;
            };
            let entry = groups.entry((parent_key, child_key)).or_insert_with(|| {
                (parent.image_path.clone(), child.image_path.clone(), 0, child.started, child.started)
            });
            entry.0 = entry.0.clone().min(parent.image_path.clone());
            entry.1 = entry.1.clone().min(child.image_path.clone());
            entry.2 += 1;
            entry.3 = entry.3.min(child.started);
            entry.4 = entry.4.max(child.started);
        }
        Ok(groups
            .into_values()
            .filter_map(|(parent, child, count, first, last)| {
                Some(SpawnPair {
                    parent: Executable::from_path(&parent)?,
                    child: Executable::from_path(&child)?,
                    count,
                    first,
                    last,
                })
            })
            .collect())
    }
}
