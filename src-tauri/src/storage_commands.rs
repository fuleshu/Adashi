//! Desktop conversion endpoints; the migration service also guards MCP stores.
use super::{resolve_project, AppState};
use crate::storage::migration::{self, MigrationPreview, MigrationReport, StorageStatus};
use crate::storage::{ProjectStorage, ProjectStore, StorageError, StorageResult};
use serde::Serialize;
use tauri::State;

/// Polling can defer a busy checkout without reporting a failed conversion.
/// Other errors still reach the user; all blocking reads run off the UI thread.
fn poll_result<T>(result: StorageResult<T>) -> Result<Option<T>, String> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(StorageError::TimedOut) => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ProjectRevisionPayload {
    project_id: String,
    revision: i64,
    change_cursor: crate::storage::ChangeCursor,
    updated_at: String,
}

#[tauri::command]
pub(super) async fn get_project_revision(
    state: State<'_, AppState>,
    project_id: Option<String>,
) -> Result<Option<ProjectRevisionPayload>, String> {
    let project = resolve_project(&state, project_id.as_deref())?;
    tauri::async_runtime::spawn_blocking(move || {
        poll_result((|| {
            let mut store = ProjectStore::open(&project)?;
            let snapshot = store.snapshot()?;
            let metadata = snapshot.metadata();
            Ok(ProjectRevisionPayload {
                project_id: project.id,
                revision: metadata.revision,
                change_cursor: metadata.cursor.clone(),
                updated_at: metadata.updated_at.clone(),
            })
        })())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(super) async fn get_project_storage(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Option<StorageStatus>, String> {
    let project = resolve_project(&state, Some(&project_id))?;
    tauri::async_runtime::spawn_blocking(move || poll_result(migration::status(&project)))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub(super) async fn preview_storage_migration(
    state: State<'_, AppState>,
    project_id: String,
    target: String,
) -> Result<MigrationPreview, String> {
    let project = resolve_project(&state, Some(&project_id))?;
    tauri::async_runtime::spawn_blocking(move || {
        migration::preview(&project, &target).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub(super) async fn migrate_project_storage(
    state: State<'_, AppState>,
    project_id: String,
    target: String,
    plan_token: String,
    archive_destination: bool,
) -> Result<MigrationReport, String> {
    let project = resolve_project(&state, Some(&project_id))?;
    tauri::async_runtime::spawn_blocking(move || {
        migration::convert(&project, &target, &plan_token, archive_destination)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
