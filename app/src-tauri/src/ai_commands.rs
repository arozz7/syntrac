//! The page's door to the optional AI help (docs/PHASE4-PLAN.md, L4c). Thin wrappers over `AiControl`; the probes and the start and stop run off the
//! user-interface thread because a server that does not answer can take a few seconds to say so.

use std::sync::Arc;

use tauri::State;

use crate::ai_control::{AiControl, AiSettingsDto, AiStatusView};

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
