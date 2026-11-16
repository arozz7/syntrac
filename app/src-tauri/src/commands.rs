//! Thin Tauri commands. All the logic is in `backend` / `views`; blocking work (SQLite, the pipe) runs off the UI
//! thread.

use std::sync::Arc;

use serde::Serialize;

use syntrac_core::{AddressScope, SystemClock};
use tauri::State;

use crate::backend::{Backend, Data};
use crate::health::HealthView;
use crate::history::SystemHistoryView;
use crate::history_network::{
    DestinationHistoryView, DestinationOrderParam, DestinationsView, NewDestinationsView, OwnersView,
    ProgramDestinationsView,
};
use crate::history_programs::{Grouping, ProgramHistoryView, ProgramMetricParam, ProgramsView};
use crate::known_view::KnownProgramsView;
use crate::owner_data_view::OwnerDataStateView;
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
pub async fn get_top_programs(
    backend: State<'_, SharedBackend>,
    range_secs: u64,
    metric: ProgramMetricParam,
    limit: Option<usize>,
    group_by_product: Option<bool>,
) -> Result<Data<ProgramsView>, ()> {
    let backend = Arc::clone(&backend);
    let grouping = if group_by_product.unwrap_or(false) { Grouping::ByProduct } else { Grouping::None };
    Ok(blocking(move || backend.top_programs(range_secs, metric, limit, grouping)).await.unwrap_or_else(internal))
}

#[tauri::command]
pub async fn get_program_history(
    backend: State<'_, SharedBackend>,
    range_secs: u64,
    path: String,
) -> Result<Data<ProgramHistoryView>, ()> {
    let backend = Arc::clone(&backend);
    Ok(blocking(move || backend.program_history(range_secs, path)).await.unwrap_or_else(internal))
}

fn scope_of(all_addresses: Option<bool>) -> AddressScope {
    if all_addresses.unwrap_or(false) {
        AddressScope::All
    } else {
        AddressScope::Internet
    }
}

#[tauri::command]
pub async fn get_top_destinations(
    backend: State<'_, SharedBackend>,
    range_secs: u64,
    order: DestinationOrderParam,
    limit: Option<usize>,
    all_addresses: Option<bool>,
) -> Result<Data<DestinationsView>, ()> {
    let backend = Arc::clone(&backend);
    let scope = scope_of(all_addresses);
    Ok(blocking(move || backend.top_destinations(range_secs, order, scope, limit)).await.unwrap_or_else(internal))
}

#[tauri::command]
pub async fn get_program_destinations(
    backend: State<'_, SharedBackend>,
    range_secs: u64,
    path: String,
    limit: Option<usize>,
    all_addresses: Option<bool>,
) -> Result<Data<ProgramDestinationsView>, ()> {
    let backend = Arc::clone(&backend);
    let scope = scope_of(all_addresses);
    Ok(blocking(move || backend.program_destinations(range_secs, path, scope, limit)).await.unwrap_or_else(internal))
}

#[tauri::command]
pub async fn get_top_owners(
    backend: State<'_, SharedBackend>,
    range_secs: u64,
    limit: Option<usize>,
    all_addresses: Option<bool>,
) -> Result<Data<OwnersView>, ()> {
    let backend = Arc::clone(&backend);
    let scope = scope_of(all_addresses);
    Ok(blocking(move || backend.top_owners(range_secs, scope, limit)).await.unwrap_or_else(internal))
}

#[tauri::command]
pub async fn get_new_destinations(
    backend: State<'_, SharedBackend>,
    window_secs: u64,
    limit: Option<usize>,
    all_addresses: Option<bool>,
) -> Result<Data<NewDestinationsView>, ()> {
    let backend = Arc::clone(&backend);
    let scope = scope_of(all_addresses);
    Ok(blocking(move || backend.new_destinations(window_secs, scope, limit)).await.unwrap_or_else(internal))
}

#[tauri::command]
pub async fn get_destination_history(
    backend: State<'_, SharedBackend>,
    range_secs: u64,
    addr: String,
) -> Result<Data<DestinationHistoryView>, ()> {
    let backend = Arc::clone(&backend);
    Ok(blocking(move || backend.destination_history(range_secs, addr)).await.unwrap_or_else(internal))
}

#[tauri::command]
pub async fn get_known_programs(backend: State<'_, SharedBackend>) -> Result<Data<KnownProgramsView>, ()> {
    let backend = Arc::clone(&backend);
    Ok(blocking(move || backend.known_programs()).await.unwrap_or_else(internal))
}

/// The result of a change request: the service's own words when it refuses.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteOutcome {
    pub ok: bool,
    pub id: Option<i64>,
    pub message: Option<String>,
}

impl WriteOutcome {
    fn from_result(result: Result<Option<i64>, String>) -> Self {
        match result {
            Ok(id) => Self { ok: true, id, message: None },
            Err(message) => Self { ok: false, id: None, message: Some(message) },
        }
    }
}

#[tauri::command]
pub async fn mark_known_program(
    backend: State<'_, SharedBackend>,
    path: String,
    label: String,
    note: Option<String>,
) -> Result<WriteOutcome, ()> {
    let backend = Arc::clone(&backend);
    let result = blocking(move || backend.mark_known(path, label, note).map(Some)).await;
    Ok(WriteOutcome::from_result(result.unwrap_or_else(|| Err("The request could not be run.".to_owned()))))
}

#[tauri::command]
pub async fn unmark_known_program(backend: State<'_, SharedBackend>, id: i64) -> Result<WriteOutcome, ()> {
    let backend = Arc::clone(&backend);
    let result = blocking(move || backend.unmark_known(id).map(|()| None)).await;
    Ok(WriteOutcome::from_result(result.unwrap_or_else(|| Err("The request could not be run.".to_owned()))))
}

/// The result of asking the service about owner data: its state, or why it could not say.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnerDataOutcome {
    pub ok: bool,
    pub state: Option<OwnerDataStateView>,
    pub message: Option<String>,
}

impl OwnerDataOutcome {
    fn from_result(result: Result<OwnerDataStateView, String>) -> Self {
        match result {
            Ok(state) => Self { ok: true, state: Some(state), message: None },
            Err(message) => Self { ok: false, state: None, message: Some(message) },
        }
    }
}

#[tauri::command]
pub async fn get_owner_data_status(backend: State<'_, SharedBackend>) -> Result<OwnerDataOutcome, ()> {
    let backend = Arc::clone(&backend);
    let result = blocking(move || backend.owner_data_status()).await;
    Ok(OwnerDataOutcome::from_result(result.unwrap_or_else(|| Err("The request could not be run.".to_owned()))))
}

/// Starts the download of the owner data. The window asks the user to confirm first; the service does the download.
#[tauri::command]
pub async fn refresh_owner_data(backend: State<'_, SharedBackend>) -> Result<OwnerDataOutcome, ()> {
    let backend = Arc::clone(&backend);
    let result = blocking(move || backend.refresh_owner_data()).await;
    Ok(OwnerDataOutcome::from_result(result.unwrap_or_else(|| Err("The request could not be run.".to_owned()))))
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
