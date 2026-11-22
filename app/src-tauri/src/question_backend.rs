//! Phase 5, slice B3 (docs/PHASE5-PLAN.md): the question box's facts, read from the database through the backend. Every call opens the
//! database read-only, like every screen does, and a database that is missing or unreadable is an error text for the model, never a crash.

use syntrac_core::investigate::{Evidence, FindingFacts};
use syntrac_core::Clock;

use crate::backend::{Backend, Data};
use crate::history_programs::ProgramMetricParam;
use crate::question_data::{open_findings, program_family, ranked_programs, recorded_program};
use crate::question_tools::{ProgramFamily, QuestionData, TopProgram};

/// The facts for the tools of the question box. `user_name` is only used to be left out of what is told.
pub struct BackendQuestionData<C> {
    backend: std::sync::Arc<Backend<C>>,
    user_name: Option<String>,
}

impl<C> BackendQuestionData<C> {
    pub fn new(backend: std::sync::Arc<Backend<C>>, user_name: Option<String>) -> Self {
        Self { backend, user_name }
    }
}

fn plain<T>(data: Data<T>) -> Result<T, String> {
    match data {
        Data::Ok { data } => Ok(data),
        Data::NoDatabase { message } | Data::SchemaMismatch { message } | Data::Error { message } => Err(message),
    }
}

impl<C: Clock + Send + Sync> QuestionData for BackendQuestionData<C> {
    fn program(&self, path: &str) -> Result<Option<Evidence>, String> {
        let user = self.user_name.clone();
        plain(self.backend.with_store(|store, now| recorded_program(store, now, path, user)))
    }

    fn findings(&self, limit: usize) -> Result<Vec<FindingFacts>, String> {
        plain(self.backend.with_store(|store, _| open_findings(store, limit)))
    }

    fn top_programs(&self, metric: ProgramMetricParam, days: u64, limit: usize) -> Result<Vec<TopProgram>, String> {
        plain(self.backend.with_store(|store, now| ranked_programs(store, now, metric, days, limit)))
    }

    fn family(&self, path: &str) -> Result<Option<ProgramFamily>, String> {
        plain(self.backend.with_store(|store, now| program_family(store, now, path)))
    }
}
