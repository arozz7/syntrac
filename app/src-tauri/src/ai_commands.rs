//! The page's door to the optional AI help (docs/PHASE4-PLAN.md, L4c). Thin wrappers over `AiControl`; the probes and the start and stop run off the
//! user-interface thread because a server that does not answer can take a few seconds to say so.

use std::sync::Arc;

use tauri::State;

use crate::ai_control::{AiControl, AiSettingsDto, AiStatusView};
use crate::ai_investigate::{InvestigationView, Investigator, PromptPreview, SubjectRef};
use crate::investigation_store::{SavedInvestigation, SavedMark};
use crate::question_job::{QuestionView, Questioner};

pub type SharedAi = Arc<AiControl>;

async fn off_the_ui_thread<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work).await.map_err(|_| "Internal error.".to_owned())
}

#[tauri::command]
pub fn get_ai_settings(ai: State<'_, SharedAi>) -> AiSettingsDto {
    ai.settings()
}

#[tauri::command]
pub fn set_ai_settings(ai: State<'_, SharedAi>, settings: AiSettingsDto) -> Result<AiSettingsDto, String> {
    ai.save(settings)
}

/// Asks the server (only when the help is on and set up) and answers with the state to show. The page calls this while the AI section is
/// open, not in the background.
#[tauri::command]
pub async fn get_ai_status(ai: State<'_, SharedAi>) -> Result<AiStatusView, String> {
    let ai = Arc::clone(&ai);
    off_the_ui_thread(move || ai.status()).await
}

#[tauri::command]
pub async fn start_ai_server(ai: State<'_, SharedAi>) -> Result<(), String> {
    let ai = Arc::clone(&ai);
    off_the_ui_thread(move || ai.start()).await?
}

#[tauri::command]
pub async fn stop_ai_server(ai: State<'_, SharedAi>) -> Result<(), String> {
    let ai = Arc::clone(&ai);
    off_the_ui_thread(move || ai.stop()).await?
}

pub type SharedInvestigator = Arc<Investigator>;

/// The prompt as it would be sent (`external` false) or as it would be copied for an outside model (`external` true). Asks no server.
#[tauri::command]
pub async fn ai_preview_prompt(
    investigator: State<'_, SharedInvestigator>,
    subject: SubjectRef,
    external: bool,
) -> Result<PromptPreview, String> {
    let investigator = Arc::clone(&investigator);
    off_the_ui_thread(move || investigator.preview(&subject, external)).await?
}

/// Starts an investigation and returns its number; the page then polls for the answer.
#[tauri::command]
pub async fn ai_start_investigation(
    investigator: State<'_, SharedInvestigator>,
    subject: SubjectRef,
) -> Result<u64, String> {
    let investigator = Arc::clone(&investigator);
    off_the_ui_thread(move || investigator.start(&subject)).await?
}

#[tauri::command]
pub fn ai_poll_investigation(investigator: State<'_, SharedInvestigator>, id: u64) -> Option<InvestigationView> {
    investigator.poll(id)
}

#[tauri::command]
pub fn ai_cancel_investigation(investigator: State<'_, SharedInvestigator>, id: u64) {
    investigator.cancel(id);
}

/// The answer kept from an earlier investigation of this subject, if there is one.
#[tauri::command]
pub fn ai_saved_investigation(
    investigator: State<'_, SharedInvestigator>,
    subject: SubjectRef,
) -> Option<SavedInvestigation> {
    investigator.saved(&subject)
}

/// Which subjects have a kept answer and since when, for marking the lists.
#[tauri::command]
pub fn ai_saved_list(investigator: State<'_, SharedInvestigator>) -> Vec<SavedMark> {
    investigator.saved_list()
}

#[tauri::command]
pub fn ai_forget_investigation(investigator: State<'_, SharedInvestigator>, subject: SubjectRef) {
    investigator.forget(&subject);
}

pub type SharedQuestioner = Arc<Questioner>;

/// Starts a question and returns its number; the page then polls for the answer and for the tool calls made.
#[tauri::command]
pub async fn ai_ask_question(questioner: State<'_, SharedQuestioner>, question: String) -> Result<u64, String> {
    let questioner = Arc::clone(&questioner);
    off_the_ui_thread(move || questioner.start(&question)).await?
}

#[tauri::command]
pub fn ai_poll_question(questioner: State<'_, SharedQuestioner>, id: u64) -> Option<QuestionView> {
    questioner.poll(id)
}

#[tauri::command]
pub fn ai_cancel_question(questioner: State<'_, SharedQuestioner>, id: u64) {
    questioner.cancel(id);
}
