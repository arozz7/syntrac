//! In-memory repository: lets the whole pipeline run offline with no database.

use std::collections::HashMap;

use syntrac_core::{
    EventEnvelope, EventQuery, EventRepository, ProcessInstanceId, ProcessRecord, ProcessRepository, RepoError,
    Timestamp,
};

#[derive(Debug, Default)]
pub struct InMemoryRepository {
    processes: HashMap<ProcessInstanceId, ProcessRecord>,
    events: Vec<EventEnvelope>,
}

impl ProcessRepository for InMemoryRepository {
    fn record_start(&mut self, record: ProcessRecord) -> Result<(), RepoError> {
        self.processes.insert(record.id, record);
        Ok(())
    }

    fn record_exit(
        &mut self,
        id: ProcessInstanceId,
        ended: Timestamp,
        exit_code: Option<u32>,
    ) -> Result<(), RepoError> {
        let record = self.processes.get_mut(&id).ok_or(RepoError::NotFound)?;
        record.ended = Some(ended);
        record.exit_code = exit_code;
        Ok(())
    }

    fn get(&self, id: ProcessInstanceId) -> Result<Option<ProcessRecord>, RepoError> {
        Ok(self.processes.get(&id).cloned())
    }

    fn resolve(&self, pid: u32, at: Timestamp) -> Result<Option<ProcessInstanceId>, RepoError> {
        let owner = self
            .processes
            .values()
            .filter(|p| p.pid == pid && p.started <= at && p.ended.is_none_or(|end| at <= end))
            .max_by_key(|p| p.started)
            .map(|p| p.id);
        Ok(owner)
    }
}

impl EventRepository for InMemoryRepository {
    fn append(&mut self, events: &[EventEnvelope]) -> Result<(), RepoError> {
        self.events.extend_from_slice(events);
        Ok(())
    }

    fn query(&self, query: &EventQuery) -> Result<Vec<EventEnvelope>, RepoError> {
        let mut found: Vec<EventEnvelope> = self
            .events
            .iter()
            .filter(|e| query.from.is_none_or(|from| e.ts >= from))
            .filter(|e| query.to.is_none_or(|to| e.ts <= to))
            .filter(|e| query.category.is_none_or(|c| e.payload.category() == c))
            .filter(|e| query.process.is_none() || e.process_instance_id == query.process)
            .cloned()
            .collect();
        found.sort_by_key(|e| e.ts);
        if let Some(limit) = query.limit {
            found.truncate(limit);
        }
        Ok(found)
    }
}
