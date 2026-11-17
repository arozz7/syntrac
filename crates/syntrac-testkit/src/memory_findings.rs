//! Findings, expectations and the detection state for the in-memory repository: the reference the SQLite one must agree with.

use syntrac_core::findings::{Expectation, Finding, FindingStatus, FindingWrite, NewExpectation};
use syntrac_core::{
    DetectionState, DetectionStateRepository, ExpectationRepository, FindingRepository, RepoError, Timestamp,
};

use crate::memory::InMemoryRepository;

#[derive(Debug, Default)]
pub(crate) struct FindingsState {
    findings: Vec<Finding>,
    /// Ids handed out: an id that was removed is never issued again.
    finding_issued: i64,
    expectations: Vec<Expectation>,
    expectation_issued: i64,
    detection: Option<DetectionState>,
    /// Changes of a file's content, recorded by `save_identity` (see `memory_identity.rs`).
    pub(crate) identity_changes: Vec<syntrac_core::IdentityChange>,
}

impl FindingRepository for InMemoryRepository {
    fn findings(&self, status: Option<FindingStatus>, limit: usize) -> Result<Vec<Finding>, RepoError> {
        let mut list: Vec<Finding> =
            self.findings.findings.iter().filter(|f| status.is_none_or(|wanted| f.status == wanted)).cloned().collect();
        list.sort_by(|a, b| b.last_at.cmp(&a.last_at).then(b.id.cmp(&a.id)));
        list.truncate(limit);
        Ok(list)
    }

    fn finding(&self, id: i64) -> Result<Option<Finding>, RepoError> {
        Ok(self.findings.findings.iter().find(|f| f.id == id).cloned())
    }

    fn findings_for_keys(&self, keys: &[String]) -> Result<Vec<Finding>, RepoError> {
        Ok(self.findings.findings.iter().filter(|f| keys.contains(&f.dedupe_key)).cloned().collect())
    }

    fn apply_writes(&mut self, writes: &[FindingWrite]) -> Result<(), RepoError> {
        let state = &mut self.findings;
        for write in writes {
            match write {
                FindingWrite::Insert(candidate) => {
                    state.finding_issued += 1;
                    state.findings.push(Finding {
                        id: state.finding_issued,
                        kind: candidate.kind.clone(),
                        subject: candidate.subject.clone(),
                        subject_label: candidate.subject_label.clone(),
                        tier: candidate.tier,
                        title: candidate.title.clone(),
                        why: candidate.why.clone(),
                        evidence: candidate.evidence.clone(),
                        dedupe_key: candidate.dedupe_key.clone(),
                        level: candidate.level,
                        status: FindingStatus::Open,
                        first_at: candidate.at,
                        last_at: candidate.at,
                        hits: 1,
                        decided_at: None,
                        snoozed_until: None,
                    });
                }
                FindingWrite::Touch { id, at } => {
                    if let Some(found) = state.findings.iter_mut().find(|f| f.id == *id) {
                        found.hits = found.hits.saturating_add(1);
                        found.last_at = found.last_at.max(*at);
                    }
                }
                FindingWrite::Reopen { id, at } => {
                    if let Some(found) = state.findings.iter_mut().find(|f| f.id == *id) {
                        found.hits = found.hits.saturating_add(1);
                        found.last_at = found.last_at.max(*at);
                        found.status = FindingStatus::Open;
                        found.snoozed_until = None;
                    }
                }
            }
        }
        Ok(())
    }

    fn set_status(
        &mut self,
        id: i64,
        status: FindingStatus,
        decided_at: Timestamp,
        snoozed_until: Option<Timestamp>,
    ) -> Result<bool, RepoError> {
        match self.findings.findings.iter_mut().find(|f| f.id == id) {
            Some(found) => {
                found.status = status;
                found.decided_at = Some(decided_at);
                found.snoozed_until = snoozed_until;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    fn status_counts(&self) -> Result<Vec<(FindingStatus, u32)>, RepoError> {
        let mut counts: Vec<(FindingStatus, u32)> = Vec::new();
        for found in &self.findings.findings {
            match counts.iter_mut().find(|(status, _)| *status == found.status) {
                Some((_, count)) => *count += 1,
                None => counts.push((found.status, 1)),
            }
        }
        Ok(counts)
    }
}

impl ExpectationRepository for InMemoryRepository {
    fn expectations(&self) -> Result<Vec<Expectation>, RepoError> {
        let mut list = self.findings.expectations.clone();
        list.sort_by_key(|e| e.id);
        Ok(list)
    }

    fn add_expectation(&mut self, expectation: &NewExpectation, at: Timestamp) -> Result<i64, RepoError> {
        let state = &mut self.findings;
        if let Some(existing) =
            state.expectations.iter_mut().find(|e| e.kind == expectation.kind && e.subject == expectation.subject)
        {
            existing.note.clone_from(&expectation.note);
            existing.ceiling = expectation.ceiling;
            return Ok(existing.id);
        }
        state.expectation_issued += 1;
        state.expectations.push(Expectation {
            id: state.expectation_issued,
            kind: expectation.kind.clone(),
            subject: expectation.subject.clone(),
            note: expectation.note.clone(),
            created_at: at,
            source_finding: expectation.source_finding,
            ceiling: expectation.ceiling,
        });
        Ok(state.expectation_issued)
    }

    fn remove_expectation(&mut self, id: i64) -> Result<bool, RepoError> {
        let before = self.findings.expectations.len();
        self.findings.expectations.retain(|e| e.id != id);
        Ok(self.findings.expectations.len() != before)
    }
}

impl DetectionStateRepository for InMemoryRepository {
    fn detection_state(&self) -> Result<Option<DetectionState>, RepoError> {
        Ok(self.findings.detection.clone())
    }

    fn switch_on(&mut self, now: Timestamp) -> Result<Timestamp, RepoError> {
        let state = self.findings.detection.get_or_insert(DetectionState {
            switched_on_at: now,
            last_run_at: None,
            last_error: None,
        });
        Ok(state.switched_on_at)
    }

    fn record_run(&mut self, at: Timestamp, error: Option<&str>) -> Result<(), RepoError> {
        let state = self.findings.detection.get_or_insert(DetectionState {
            switched_on_at: at,
            last_run_at: None,
            last_error: None,
        });
        state.last_run_at = Some(at);
        state.last_error = error.map(str::to_owned);
        Ok(())
    }
}
