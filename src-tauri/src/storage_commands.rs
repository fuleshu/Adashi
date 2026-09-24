//! Desktop conversion endpoints; the migration service also guards MCP stores.
use super::{resolve_project, AppState};
use crate::storage::migration::{self, MigrationPreview, MigrationReport, StorageStatus};
use tauri::State;

#[tauri::command]
pub(super) fn get_project_storage(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<StorageStatus, String> {
    migration::status(&resolve_project(&state, Some(&project_id))?).map_err(|e| e.to_string())
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
