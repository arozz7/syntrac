//! Thin Tauri commands. All the logic is in `backend` / `views`; blocking work (SQLite, the pipe) runs off the UI
//! thread.

use std::sync::Arc;

use syntrac_core::SystemClock;
use tauri::State;

use crate::backend::{Backend, Data};
use crate::health::HealthView;
use crate::history::SystemHistoryView;
use crate::views::{ConnectionRowView, OverviewView, ProcessListParams, ProcessRowView};

pub type SharedBackend = Arc<Backend<SystemClock>>;

async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    tauri::async_runtime::spawn_blocking(work).await.ok()
}

fn internal<T>() -> Data<T> {
    Data::Error { message: "Internal error while reading the data.".to_owned() }
}

#[tauri::command]
pub async fn get_health(backend: State<'_, SharedBackend>) -> Result<HealthView, ()> {
    let backend = Arc::clone(&backend);
    Ok(blocking(move || backend.health()).await.unwrap_or_else(|| HealthView::unknown("Internal error.")))
}

#[tauri::command]
pub async fn get_overview(backend: State<'_, SharedBackend>, range_secs: u64) -> Result<Data<OverviewView>, ()> {
    let backend = Arc::clone(&backend);
    Ok(blocking(move || backend.overview(range_secs)).await.unwrap_or_else(internal))
}

#[tauri::command]
pub async fn get_system_history(
    backend: State<'_, SharedBackend>,
    range_secs: u64,
) -> Result<Data<SystemHistoryView>, ()> {
    let backend = Arc::clone(&backend);
    Ok(blocking(move || backend.system_history(range_secs)).await.unwrap_or_else(internal))
}

#[tauri::command]
pub async fn list_processes(
    backend: State<'_, SharedBackend>,
    params: ProcessListParams,
) -> Result<Data<Vec<ProcessRowView>>, ()> {
    let backend = Arc::clone(&backend);
    Ok(blocking(move || backend.processes(params)).await.unwrap_or_else(internal))
}

#[tauri::command]
pub async fn list_connections(
    backend: State<'_, SharedBackend>,
    range_secs: u64,
    limit: Option<usize>,
) -> Result<Data<Vec<ConnectionRowView>>, ()> {
    let backend = Arc::clone(&backend);
    Ok(blocking(move || backend.connections(range_secs, limit)).await.unwrap_or_else(internal))
}
