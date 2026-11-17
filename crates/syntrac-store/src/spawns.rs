//! `SpawnRepository` for SQLite: which program started which, read from the raw process rows (the detail retention, 14 days by
//! default). One join and one group-by inside the database; only one row per (parent, child) pair comes back.

use rusqlite::params_from_iter;
use rusqlite::types::Value;
use syntrac_core::{Executable, RepoError, SpawnPair, SpawnRepository, Timestamp};

use crate::connection::{backend, SqliteStore};

impl SpawnRepository for SqliteStore {
    fn spawn_pairs(&self, since: Timestamp, child_names: &[String]) -> Result<Vec<SpawnPair>, RepoError> {
        if child_names.is_empty() {
            return Ok(Vec::new());
        }
        // The name is bound as a parameter and compared by equality (never LIKE), so `_` and `%` in a name mean themselves.
        let mut params: Vec<Value> = vec![since.as_micros().into()];
        let mut clauses = Vec::new();
        for (index, name) in child_names.iter().enumerate() {
            let slot = index + 2;
            clauses.push(format!(
                "(lower(c.image_path) = ?{slot} OR substr(lower(c.image_path), -(length(?{slot}) + 1)) = char(92) || ?{slot})"
            ));
            params.push(name.to_ascii_lowercase().into());
        }
        let sql = format!(
            "SELECT MIN(p.image_path), MIN(c.image_path), COUNT(*), MIN(c.started), MAX(c.started)
             FROM processes c JOIN processes p ON p.id = c.parent_id
             WHERE c.started >= ?1 AND ({})
             GROUP BY lower(p.image_path), lower(c.image_path)",
            clauses.join(" OR ")
        );
        let mut statement = self.conn.prepare(&sql).map_err(backend)?;
        let mut rows = statement.query(params_from_iter(params)).map_err(backend)?;
        let mut found = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let parent: String = row.get(0).map_err(backend)?;
            let child: String = row.get(1).map_err(backend)?;
            let (Some(parent), Some(child)) = (Executable::from_path(&parent), Executable::from_path(&child)) else {
                continue;
            };
            let count: i64 = row.get(2).map_err(backend)?;
            found.push(SpawnPair {
                parent,
                child,
                count: u32::try_from(count).unwrap_or(u32::MAX),
                first: Timestamp::from_micros(row.get(3).map_err(backend)?),
                last: Timestamp::from_micros(row.get(4).map_err(backend)?),
            });
        }
        Ok(found)
    }
}
