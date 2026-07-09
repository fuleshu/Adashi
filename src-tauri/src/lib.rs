mod design;
mod fixed_hooks;
mod mcp;
mod memory;
mod qa;
mod rules;
mod schema;
mod seed;
mod settings;
mod state;
mod tasks;

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{
    AppHandle, Manager, Monitor, PhysicalPosition, PhysicalSize, State, WebviewWindow, WindowEvent,
};
use tauri_plugin_dialog::DialogExt;

use crate::design::DesignArtifactTypeRecord;
use crate::fixed_hooks::FixedHookPrompt;
use crate::memory::ProjectMemory;
use crate::qa::{NewQaJob, QaDesignLinkInput, QaJob, QaJobQuery, QaRun, UpdateQaJob};
use crate::rules::{NewRule, Rule, UpdateRule};
use crate::settings::{
    AppSettings, ProjectSettings, RuleTemplate, RuleTemplateDraft, WindowSettings,
};
use crate::state as project_state;
use crate::tasks::{FinishTask, NewTask, Task, TaskDesignSpecificationLinkInput, UpdateTask};

struct AppState {
    settings_path: PathBuf,
    settings: Arc<Mutex<AppSettings>>,
}

const MIN_RESTORED_WINDOW_WIDTH: u32 = 640;
const MIN_RESTORED_WINDOW_HEIGHT: u32 = 480;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DashboardPayload {
    project_id: String,
    project_name: String,
    project_folder: String,
    revision: i64,
    workspace_name: String,
    workspace_description: String,
    structurizr_dsl: String,
    structurizr_workspace: String,
    structurizr_view_key: String,
    design_elements: Vec<DesignElement>,
    design_relationships: Vec<DesignRelationship>,
    uml_artifact_types: Vec<DesignArtifactTypeRecord>,
    diagrams: Vec<DesignDiagram>,
    tasks: Vec<Task>,
    guidelines: Vec<Guideline>,
    post_task_commands: Vec<PostTaskCommand>,
    qa_checks: Vec<QaCheck>,
    qa_jobs: Vec<QaJob>,
    qa_runs: Vec<QaRun>,
    rules: Vec<Rule>,
    rule_templates: Vec<RuleTemplate>,
    fixed_hook_prompts: Vec<FixedHookPrompt>,
    memory: ProjectMemory,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DesignDiagram {
    id: i64,
    kind: String,
    key: String,
    title: String,
    source: String,
    diagram_type: String,
    artifact_role: String,
    artifact_label: String,
    artifact_rank: i64,
    attached_to_external_id: Option<String>,
    attached_to_target_type: Option<String>,
    sort_order: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DesignElement {
    id: i64,
    external_id: String,
    parent_external_id: Option<String>,
    element_type: String,
    name: String,
    description: String,
    technology: String,
    tags: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DesignRelationship {
    id: i64,
    external_id: String,
    source_external_id: String,
    destination_external_id: String,
    description: String,
    technology: String,
    tags: String,
}

#[derive(Serialize)]
struct Guideline {
    id: i64,
    title: String,
    body: String,
}

#[derive(Serialize)]
struct PostTaskCommand {
    id: i64,
    label: String,
    command: String,
    trigger: String,
}

#[derive(Serialize)]
struct QaCheck {
    id: i64,
    label: String,
    command: String,
    required: bool,
}

#[tauri::command]
fn get_app_settings(state: State<'_, AppState>) -> Result<AppSettings, String> {
    state
        .settings
        .lock()
        .map(|settings| settings.clone())
        .map_err(|_| "Settings lock was poisoned".to_string())
}

#[tauri::command]
fn get_dashboard(
    project_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project = resolve_project(&state, project_id.as_deref())?;
    let db = open_project_database(&project).map_err(|err| err.to_string())?;
    load_dashboard_payload(project, &db, &state)
}

fn load_dashboard_payload(
    project: ProjectSettings,
    db: &Connection,
    state: &AppState,
) -> Result<DashboardPayload, String> {
    let workspace = db
        .query_row(
            "SELECT w.name, w.description, w.structurizr_dsl, w.structurizr_json
             FROM design_workspaces w
             ORDER BY w.id
             LIMIT 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "No design workspace has been seeded".to_string())?;

    let project_row_id = load_project_row_id(db)?;
    let revision = project_state::load_project_revision(db, project_row_id)?.revision;
    let structurizr_view_key = db
        .query_row(
            "SELECT key
             FROM diagrams
             WHERE kind = 'structurizr'
             ORDER BY sort_order, id
             LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|err| err.to_string())?
        .unwrap_or_else(|| "ProjectContext".to_string());

    Ok(DashboardPayload {
        project_id: project.id,
        project_name: project.name,
        project_folder: project.folder,
        revision,
        workspace_name: workspace.0,
        workspace_description: workspace.1,
        structurizr_dsl: workspace.2,
        structurizr_workspace: workspace.3,
        structurizr_view_key,
        design_elements: load_design_elements(&db)?,
        design_relationships: load_design_relationships(&db)?,
        uml_artifact_types: design::supported_uml_artifact_types(),
        diagrams: load_diagrams(&db)?,
        tasks: tasks::load_tasks(db, project_row_id, None)?,
        guidelines: load_guidelines(&db)?,
        post_task_commands: load_post_task_commands(&db)?,
        qa_checks: load_qa_checks(&db)?,
        qa_jobs: qa::load_jobs(db, project_row_id, None)?,
        qa_runs: qa::load_runs(db, project_row_id, Some(20))?,
        rules: rules::load_rules(db)?,
        rule_templates: load_rule_templates(state)?,
        fixed_hook_prompts: fixed_hooks::load_fixed_hook_prompts(db, project_row_id)?,
        memory: memory::load_memory(db, project_row_id)?,
    })
}

fn load_rule_templates(state: &AppState) -> Result<Vec<RuleTemplate>, String> {
    state
        .settings
        .lock()
        .map(|settings| settings.rule_templates.clone())
        .map_err(|_| "Settings lock was poisoned".to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectRevisionPayload {
    project_id: String,
    revision: i64,
    updated_at: String,
}

#[tauri::command]
fn get_project_revision(
    project_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<ProjectRevisionPayload, String> {
    let project = resolve_project(&state, project_id.as_deref())?;
    let db = open_project_database(&project).map_err(|err| err.to_string())?;
    let revision = project_state::load_project_revision(&db, load_project_row_id(&db)?)?;

    Ok(ProjectRevisionPayload {
        project_id: project.id,
        revision: revision.revision,
        updated_at: revision.updated_at,
    })
}

#[tauri::command]
async fn pick_project_folder(
    current_folder: Option<String>,
    app: AppHandle,
) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut dialog = app.dialog().file().set_title("Select project folder");
        if let Some(folder) = current_folder
            .as_deref()
            .map(str::trim)
            .filter(|folder| !folder.is_empty())
        {
            let folder = PathBuf::from(folder);
            if folder.is_dir() {
                dialog = dialog.set_directory(folder);
            }
        }

        let folder = dialog.blocking_pick_folder();
        folder
            .map(|folder| {
                folder
                    .simplified()
                    .into_path()
                    .map(|path| settings::normalize_project_folder_path(&path))
                    .map_err(|err| err.to_string())
            })
            .transpose()
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
fn set_active_project(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<AppSettings, String> {
    let mut settings = state
        .settings
        .lock()
        .map_err(|_| "Settings lock was poisoned".to_string())?;

    if !settings
        .projects
        .iter()
        .any(|project| project.id == project_id)
    {
        return Err(format!("Unknown project id: {project_id}"));
    }

    settings.last_active_project_id = Some(project_id);
    settings::save(&state.settings_path, &settings).map_err(|err| err.to_string())?;
    Ok(settings.clone())
}

#[tauri::command]
fn add_project(
    name: String,
    folder: String,
    state: State<'_, AppState>,
) -> Result<AppSettings, String> {
    let trimmed_name = name.trim();
    let trimmed_folder = folder.trim();

    if trimmed_name.is_empty() {
        return Err("Project name is required".to_string());
    }

    if trimmed_folder.is_empty() {
        return Err("Project folder is required".to_string());
    }

    let project = settings::new_project(
        trimmed_name.to_string(),
        settings::normalize_project_folder(trimmed_folder),
    );

    let settings = state
        .settings
        .lock()
        .map_err(|_| "Settings lock was poisoned".to_string())?;

    if settings
        .projects
        .iter()
        .any(|existing| existing.folder.eq_ignore_ascii_case(&project.folder))
    {
        return Err("That project folder is already registered".to_string());
    }

    drop(settings);

    let db = open_project_database(&project).map_err(|err| err.to_string())?;
    verify_project_database(&db)?;

    let mut settings = state
        .settings
        .lock()
        .map_err(|_| "Settings lock was poisoned".to_string())?;

    if settings
        .projects
        .iter()
        .any(|existing| existing.folder.eq_ignore_ascii_case(&project.folder))
    {
        return Err("That project folder is already registered".to_string());
    }

    settings.last_active_project_id = Some(project.id.clone());
    settings.projects.push(project);
    settings::save(&state.settings_path, &settings).map_err(|err| err.to_string())?;
    Ok(settings.clone())
}

#[tauri::command]
fn delete_project(project_id: String, state: State<'_, AppState>) -> Result<AppSettings, String> {
    let mut settings = state
        .settings
        .lock()
        .map_err(|_| "Settings lock was poisoned".to_string())?;

    if settings.projects.len() <= 1 {
        return Err("At least one project must remain configured".to_string());
    }

    let initial_len = settings.projects.len();
    settings.projects.retain(|project| project.id != project_id);

    if settings.projects.len() == initial_len {
        return Err(format!("Unknown project id: {project_id}"));
    }

    if settings.last_active_project_id.as_deref() == Some(project_id.as_str()) {
        settings.last_active_project_id =
            settings.projects.first().map(|project| project.id.clone());
    }

    settings::save(&state.settings_path, &settings).map_err(|err| err.to_string())?;
    Ok(settings.clone())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateRuleRequest {
    project_id: Option<String>,
    name: String,
    enabled: bool,
    intend: String,
    hook: String,
    prompt: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateRuleRequest {
    project_id: Option<String>,
    id: i64,
    name: String,
    enabled: bool,
    intend: String,
    hook: String,
    prompt: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveRuleTemplateRequest {
    project_id: Option<String>,
    rule_id: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateRuleFromTemplateRequest {
    project_id: Option<String>,
    template_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateMemoryRuleRequest {
    project_id: Option<String>,
    rule: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateMemoryRequest {
    project_id: Option<String>,
    memory: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateFixedHookPromptRequest {
    project_id: Option<String>,
    key: String,
    prompt: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateDesignElementRequest {
    project_id: Option<String>,
    external_id: String,
    name: String,
    description: String,
    technology: String,
    tags: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateDesignRelationshipRequest {
    project_id: Option<String>,
    external_id: String,
    description: String,
    technology: String,
    tags: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateDesignRelationshipRequest {
    project_id: Option<String>,
    source_external_id: String,
    destination_external_id: String,
    description: String,
    technology: String,
    tags: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateTaskRequest {
    project_id: Option<String>,
    title: String,
    description: Option<String>,
    design_specification_links: Option<Vec<TaskDesignSpecificationLinkInput>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateTaskRequest {
    project_id: Option<String>,
    task_id: i64,
    title: Option<String>,
    description: Option<String>,
    state: Option<String>,
    design_specification_links: Option<Vec<TaskDesignSpecificationLinkInput>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FinishTaskRequest {
    project_id: Option<String>,
    task_id: i64,
    completion_memo: String,
    created_files: Vec<String>,
    changed_files: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateQaJobRequest {
    project_id: Option<String>,
    name: String,
    description: Option<String>,
    command: String,
    working_directory: Option<String>,
    shell: Option<String>,
    timeout_seconds: Option<i64>,
    enabled: Option<bool>,
    design_specification_links: Option<Vec<QaDesignLinkInput>>,
    task_ids: Option<Vec<i64>>,
    tags: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateQaJobRequest {
    project_id: Option<String>,
    qa_job_id: i64,
    name: Option<String>,
    description: Option<String>,
    command: Option<String>,
    working_directory: Option<String>,
    shell: Option<String>,
    timeout_seconds: Option<i64>,
    enabled: Option<bool>,
    design_specification_links: Option<Vec<QaDesignLinkInput>>,
    task_ids: Option<Vec<i64>>,
    tags: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunQaJobsRequest {
    project_id: Option<String>,
    query: QaJobQuery,
    trigger_source: Option<String>,
}

#[tauri::command]
fn create_rule(
    input: CreateRuleRequest,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project = resolve_project(&state, input.project_id.as_deref())?;
    let db = open_project_database(&project).map_err(|err| err.to_string())?;
    let project_row_id = load_project_row_id(&db)?;

    rules::create_rule(
        &db,
        project_row_id,
        NewRule {
            name: input.name,
            enabled: input.enabled,
            intend: input.intend,
            hook: input.hook,
            prompt: input.prompt,
        },
    )?;
    project_state::bump_project_revision(&db, project_row_id)?;

    load_dashboard_payload(project, &db, &state)
}

#[tauri::command]
fn update_rule(
    input: UpdateRuleRequest,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project = resolve_project(&state, input.project_id.as_deref())?;
    let db = open_project_database(&project).map_err(|err| err.to_string())?;

    rules::update_rule(
        &db,
        UpdateRule {
            id: input.id,
            name: input.name,
            enabled: input.enabled,
            intend: input.intend,
            hook: input.hook,
            prompt: input.prompt,
        },
    )?;
    project_state::bump_project_revision(&db, load_project_row_id(&db)?)?;

    load_dashboard_payload(project, &db, &state)
}

#[tauri::command]
fn delete_rule(
    project_id: Option<String>,
    rule_id: i64,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project = resolve_project(&state, project_id.as_deref())?;
    let db = open_project_database(&project).map_err(|err| err.to_string())?;
    let project_row_id = load_project_row_id(&db)?;
    rules::delete_rule(&db, rule_id)?;
    project_state::bump_project_revision(&db, project_row_id)?;
    load_dashboard_payload(project, &db, &state)
}

#[tauri::command]
fn save_rule_template(
    input: SaveRuleTemplateRequest,
    state: State<'_, AppState>,
) -> Result<AppSettings, String> {
    let project = resolve_project(&state, input.project_id.as_deref())?;
    let db = open_project_database(&project).map_err(|err| err.to_string())?;
    let rule = rules::load_rule(&db, input.rule_id)?;
    let mut settings = state
        .settings
        .lock()
        .map_err(|_| "Settings lock was poisoned".to_string())?;

    settings::save_rule_template(
        &mut settings,
        RuleTemplateDraft {
            name: rule.name,
            enabled: rule.enabled,
            intend: rule.intend,
            hook: rule.hook,
            prompt: rule.prompt,
        },
    )?;
    settings::save(&state.settings_path, &settings).map_err(|err| err.to_string())?;
    Ok(settings.clone())
}

#[tauri::command]
fn create_rule_from_template(
    input: CreateRuleFromTemplateRequest,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project = resolve_project(&state, input.project_id.as_deref())?;
    let template = state
        .settings
        .lock()
        .map_err(|_| "Settings lock was poisoned".to_string())?
        .rule_templates
        .iter()
        .find(|template| template.id == input.template_id)
        .cloned()
        .ok_or_else(|| format!("Unknown rule template id: {}", input.template_id))?;
    let db = open_project_database(&project).map_err(|err| err.to_string())?;
    let project_row_id = load_project_row_id(&db)?;

    rules::create_rule_from_template(&db, project_row_id, &template)?;
    project_state::bump_project_revision(&db, project_row_id)?;
    load_dashboard_payload(project, &db, &state)
}

#[tauri::command]
fn delete_rule_template(
    template_id: String,
    state: State<'_, AppState>,
) -> Result<AppSettings, String> {
    let mut settings = state
        .settings
        .lock()
        .map_err(|_| "Settings lock was poisoned".to_string())?;

    settings::delete_rule_template(&mut settings, &template_id)?;
    settings::save(&state.settings_path, &settings).map_err(|err| err.to_string())?;
    Ok(settings.clone())
}

#[tauri::command]
fn update_memory_rule(
    input: UpdateMemoryRuleRequest,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project = resolve_project(&state, input.project_id.as_deref())?;
    let db = open_project_database(&project).map_err(|err| err.to_string())?;
    let project_row_id = load_project_row_id(&db)?;
    memory::update_memory_rule(&db, project_row_id, input.rule)?;
    project_state::bump_project_revision(&db, project_row_id)?;
    load_dashboard_payload(project, &db, &state)
}

#[tauri::command]
fn update_memory(
    input: UpdateMemoryRequest,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project = resolve_project(&state, input.project_id.as_deref())?;
    let db = open_project_database(&project).map_err(|err| err.to_string())?;
    let project_row_id = load_project_row_id(&db)?;
    memory::update_memory(&db, project_row_id, input.memory)?;
    project_state::bump_project_revision(&db, project_row_id)?;
    load_dashboard_payload(project, &db, &state)
}

#[tauri::command]
fn update_fixed_hook_prompt(
    input: UpdateFixedHookPromptRequest,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project = resolve_project(&state, input.project_id.as_deref())?;
    let db = open_project_database(&project).map_err(|err| err.to_string())?;
    let project_row_id = load_project_row_id(&db)?;
    fixed_hooks::update_fixed_hook_prompt(&db, project_row_id, input.key, input.prompt)?;
    project_state::bump_project_revision(&db, project_row_id)?;
    load_dashboard_payload(project, &db, &state)
}

#[tauri::command]
fn update_design_element(
    input: UpdateDesignElementRequest,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project = resolve_project(&state, input.project_id.as_deref())?;
    let db = open_project_database(&project).map_err(|err| err.to_string())?;
    let project_row_id = load_project_row_id(&db)?;
    let workspace_id = load_workspace_id(&db)?;
    let name = input.name.trim();

    if name.is_empty() {
        return Err("Element name is required".to_string());
    }

    let updated = db
        .execute(
            "UPDATE c4_elements
             SET name = ?1, description = ?2, technology = ?3, tags = ?4
             WHERE workspace_id = ?5 AND external_id = ?6",
            rusqlite::params![
                name,
                input.description.trim(),
                input.technology.trim(),
                input.tags.trim(),
                workspace_id,
                input.external_id,
            ],
        )
        .map_err(|err| err.to_string())?;

    if updated == 0 {
        return Err(format!("Unknown design element id: {}", input.external_id));
    }

    sync_structurizr_element(
        &db,
        workspace_id,
        &input.external_id,
        name,
        input.description.trim(),
        input.technology.trim(),
        input.tags.trim(),
    )?;
    project_state::bump_project_revision(&db, project_row_id)?;
    load_dashboard_payload(project, &db, &state)
}

#[tauri::command]
fn update_design_relationship(
    input: UpdateDesignRelationshipRequest,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project = resolve_project(&state, input.project_id.as_deref())?;
    let db = open_project_database(&project).map_err(|err| err.to_string())?;
    let project_row_id = load_project_row_id(&db)?;
    let workspace_id = load_workspace_id(&db)?;
    let description = input.description.trim();

    if description.is_empty() {
        return Err("Relationship description is required".to_string());
    }

    let updated = db
        .execute(
            "UPDATE c4_relationships
             SET description = ?1, technology = ?2, tags = ?3
             WHERE workspace_id = ?4 AND external_id = ?5",
            rusqlite::params![
                description,
                input.technology.trim(),
                input.tags.trim(),
                workspace_id,
                input.external_id,
            ],
        )
        .map_err(|err| err.to_string())?;

    if updated == 0 {
        return Err(format!(
            "Unknown design relationship id: {}",
            input.external_id
        ));
    }

    sync_structurizr_relationship(
        &db,
        workspace_id,
        &input.external_id,
        description,
        input.technology.trim(),
        input.tags.trim(),
    )?;
    project_state::bump_project_revision(&db, project_row_id)?;
    load_dashboard_payload(project, &db, &state)
}

#[tauri::command]
fn create_design_relationship(
    input: CreateDesignRelationshipRequest,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project = resolve_project(&state, input.project_id.as_deref())?;
    let db = open_project_database(&project).map_err(|err| err.to_string())?;
    let project_row_id = load_project_row_id(&db)?;
    let workspace_id = load_workspace_id(&db)?;
    let description = input.description.trim();

    if input.source_external_id == input.destination_external_id {
        return Err("A relationship must connect two different elements".to_string());
    }

    if description.is_empty() {
        return Err("Relationship description is required".to_string());
    }

    ensure_element_exists(&db, workspace_id, &input.source_external_id)?;
    ensure_element_exists(&db, workspace_id, &input.destination_external_id)?;

    let external_id = format!(
        "ui-rel-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|err| err.to_string())?
            .as_millis()
    );

    db.execute(
        "INSERT INTO c4_relationships(
            workspace_id, external_id, source_external_id, destination_external_id, description, technology, tags
         )
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            workspace_id,
            external_id,
            input.source_external_id,
            input.destination_external_id,
            description,
            input.technology.trim(),
            input.tags.trim(),
        ],
    )
    .map_err(|err| err.to_string())?;

    sync_structurizr_new_relationship(
        &db,
        workspace_id,
        &external_id,
        &input.source_external_id,
        &input.destination_external_id,
        description,
        input.technology.trim(),
        input.tags.trim(),
    )?;
    project_state::bump_project_revision(&db, project_row_id)?;
    load_dashboard_payload(project, &db, &state)
}

#[tauri::command]
fn create_task(
    input: CreateTaskRequest,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project = resolve_project(&state, input.project_id.as_deref())?;
    let db = open_project_database(&project).map_err(|err| err.to_string())?;
    let project_row_id = load_project_row_id(&db)?;
    tasks::create_task(
        &db,
        project_row_id,
        NewTask {
            title: input.title,
            description: input.description,
            design_specification_links: input.design_specification_links,
        },
    )?;
    project_state::bump_project_revision(&db, project_row_id)?;
    load_dashboard_payload(project, &db, &state)
}

#[tauri::command]
fn update_task(
    input: UpdateTaskRequest,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project = resolve_project(&state, input.project_id.as_deref())?;
    let db = open_project_database(&project).map_err(|err| err.to_string())?;
    let project_row_id = load_project_row_id(&db)?;
    tasks::update_task(
        &db,
        project_row_id,
        UpdateTask {
            task_id: input.task_id,
            title: input.title,
            description: input.description,
            state: input.state,
            design_specification_links: input.design_specification_links,
        },
    )?;
    project_state::bump_project_revision(&db, project_row_id)?;
    load_dashboard_payload(project, &db, &state)
}

#[tauri::command]
fn finish_task(
    input: FinishTaskRequest,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project = resolve_project(&state, input.project_id.as_deref())?;
    let db = open_project_database(&project).map_err(|err| err.to_string())?;
    let project_row_id = load_project_row_id(&db)?;
    tasks::finish_task(
        &db,
        project_row_id,
        FinishTask {
            task_id: input.task_id,
            completion_memo: input.completion_memo,
            created_files: input.created_files,
            changed_files: input.changed_files,
        },
    )?;
    project_state::bump_project_revision(&db, project_row_id)?;
    load_dashboard_payload(project, &db, &state)
}

#[tauri::command]
fn confirm_task(
    project_id: Option<String>,
    task_id: i64,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project = resolve_project(&state, project_id.as_deref())?;
    let db = open_project_database(&project).map_err(|err| err.to_string())?;
    let project_row_id = load_project_row_id(&db)?;
    tasks::confirm_task(&db, project_row_id, task_id)?;
    project_state::bump_project_revision(&db, project_row_id)?;
    load_dashboard_payload(project, &db, &state)
}

#[tauri::command]
fn delete_task(
    project_id: Option<String>,
    task_id: i64,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project = resolve_project(&state, project_id.as_deref())?;
    let db = open_project_database(&project).map_err(|err| err.to_string())?;
    let project_row_id = load_project_row_id(&db)?;
    tasks::delete_task(&db, project_row_id, task_id)?;
    project_state::bump_project_revision(&db, project_row_id)?;
    load_dashboard_payload(project, &db, &state)
}

#[tauri::command]
fn create_qa_job(
    input: CreateQaJobRequest,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project = resolve_project(&state, input.project_id.as_deref())?;
    let db = open_project_database(&project).map_err(|err| err.to_string())?;
    let project_row_id = load_project_row_id(&db)?;
    qa::create_job(
        &db,
        project_row_id,
        NewQaJob {
            name: input.name,
            description: input.description,
            command: input.command,
            working_directory: input.working_directory,
            shell: input.shell,
            timeout_seconds: input.timeout_seconds,
            enabled: input.enabled,
            created_by: Some("user".to_string()),
            design_specification_links: input.design_specification_links,
            task_ids: input.task_ids,
            tags: input.tags,
        },
    )?;
    project_state::bump_project_revision(&db, project_row_id)?;
    load_dashboard_payload(project, &db, &state)
}

#[tauri::command]
fn update_qa_job(
    input: UpdateQaJobRequest,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project = resolve_project(&state, input.project_id.as_deref())?;
    let db = open_project_database(&project).map_err(|err| err.to_string())?;
    let project_row_id = load_project_row_id(&db)?;
    qa::update_job(
        &db,
        project_row_id,
        UpdateQaJob {
            qa_job_id: input.qa_job_id,
            name: input.name,
            description: input.description,
            command: input.command,
            working_directory: input.working_directory,
            shell: input.shell,
            timeout_seconds: input.timeout_seconds,
            enabled: input.enabled,
            design_specification_links: input.design_specification_links,
            task_ids: input.task_ids,
            tags: input.tags,
        },
    )?;
    project_state::bump_project_revision(&db, project_row_id)?;
    load_dashboard_payload(project, &db, &state)
}

#[tauri::command]
fn delete_qa_job(
    project_id: Option<String>,
    qa_job_id: i64,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project = resolve_project(&state, project_id.as_deref())?;
    let db = open_project_database(&project).map_err(|err| err.to_string())?;
    let project_row_id = load_project_row_id(&db)?;
    qa::delete_job(&db, project_row_id, qa_job_id)?;
    project_state::bump_project_revision(&db, project_row_id)?;
    load_dashboard_payload(project, &db, &state)
}

#[tauri::command]
fn run_qa_jobs(
    input: RunQaJobsRequest,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project = resolve_project(&state, input.project_id.as_deref())?;
    let db = open_project_database(&project).map_err(|err| err.to_string())?;
    let project_row_id = load_project_row_id(&db)?;
    qa::run_jobs(
        &db,
        project_row_id,
        &project.folder,
        input.query,
        input.trigger_source.as_deref().unwrap_or("dashboard"),
    )?;
    project_state::bump_project_revision(&db, project_row_id)?;
    load_dashboard_payload(project, &db, &state)
}

pub fn run_mcp() -> Result<(), Box<dyn std::error::Error>> {
    mcp::run_stdio_server()
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let settings_path = settings::settings_path();
            let settings = Arc::new(Mutex::new(
                settings::load_or_init(&settings_path).map_err(|err| err.to_string())?,
            ));

            if let Some(window) = app.get_webview_window("main") {
                let _ = restore_window(&window, &settings);
                track_window_state(&window, settings_path.clone(), settings.clone());
            }

            app.manage(AppState {
                settings_path,
                settings,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            add_project,
            confirm_task,
            create_qa_job,
            create_rule,
            create_rule_from_template,
            create_task,
            delete_qa_job,
            delete_rule,
            delete_rule_template,
            create_design_relationship,
            delete_project,
            delete_task,
            finish_task,
            get_app_settings,
            get_dashboard,
            get_project_revision,
            pick_project_folder,
            set_active_project,
            save_rule_template,
            update_design_element,
            update_design_relationship,
            update_fixed_hook_prompt,
            update_memory,
            update_memory_rule,
            update_qa_job,
            update_rule,
            update_task,
            run_qa_jobs
        ])
        .run(tauri::generate_context!())
        .expect("error while running Adashi");
}

fn resolve_project(
    state: &State<'_, AppState>,
    project_id: Option<&str>,
) -> Result<ProjectSettings, String> {
    let settings = state
        .settings
        .lock()
        .map_err(|_| "Settings lock was poisoned".to_string())?;

    resolve_project_from_settings(&settings, project_id)
}

pub(crate) fn resolve_project_from_settings(
    settings: &AppSettings,
    project_id: Option<&str>,
) -> Result<ProjectSettings, String> {
    let project = if let Some(project_id) = project_id {
        settings
            .projects
            .iter()
            .find(|project| project.id == project_id)
    } else {
        settings.active_project()
    };

    project
        .cloned()
        .ok_or_else(|| "No active project is configured".to_string())
}

pub(crate) fn open_project_database(
    project: &ProjectSettings,
) -> Result<Connection, Box<dyn std::error::Error>> {
    let data_dir = settings::project_data_dir(project);
    fs::create_dir_all(&data_dir)?;

    let mut db = Connection::open(settings::project_database_path(project))?;
    schema::migrate(&mut db)?;
    seed::seed_initial_data(&mut db, project)?;
    fixed_hooks::ensure_fixed_hook_prompts(&db).map_err(std::io::Error::other)?;
    Ok(db)
}

fn verify_project_database(db: &Connection) -> Result<(), String> {
    let project_row_id = load_project_row_id(db)?;
    project_state::load_project_revision(db, project_row_id)?;
    memory::load_memory(db, project_row_id)?;

    if fixed_hooks::load_fixed_hook_prompts(db, project_row_id)?.len() < 2 {
        return Err("Project database is missing default fixed hook prompts".to_string());
    }

    db.query_row("SELECT id FROM design_workspaces LIMIT 1", [], |row| {
        row.get::<_, i64>(0)
    })
    .map_err(|err| format!("Project database is missing the default design workspace: {err}"))?;

    Ok(())
}

fn load_project_row_id(db: &Connection) -> Result<i64, String> {
    db.query_row("SELECT id FROM projects ORDER BY id LIMIT 1", [], |row| {
        row.get(0)
    })
    .map_err(|err| err.to_string())
}

fn load_workspace_id(db: &Connection) -> Result<i64, String> {
    db.query_row(
        "SELECT id FROM design_workspaces ORDER BY id LIMIT 1",
        [],
        |row| row.get(0),
    )
    .map_err(|err| err.to_string())
}

fn restore_window(
    window: &WebviewWindow,
    settings: &Arc<Mutex<AppSettings>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let saved = settings
        .lock()
        .map_err(|_| "Settings lock was poisoned")?
        .window
        .clone();
    let restored = restored_window_settings(&saved, &window.available_monitors()?);

    window.set_size(PhysicalSize::new(restored.width, restored.height))?;

    if let (Some(x), Some(y)) = (restored.x, restored.y) {
        window.set_position(PhysicalPosition::new(x, y))?;
    }

    Ok(())
}

fn track_window_state(
    window: &WebviewWindow,
    settings_path: PathBuf,
    settings: Arc<Mutex<AppSettings>>,
) {
    let tracked_window = window.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::Resized(_) | WindowEvent::Moved(_) | WindowEvent::CloseRequested { .. } => {
            let _ = save_window_state(&tracked_window, &settings_path, &settings);
        }
        _ => {}
    });
}

fn save_window_state(
    window: &WebviewWindow,
    settings_path: &PathBuf,
    settings: &Arc<Mutex<AppSettings>>,
) -> Result<(), Box<dyn std::error::Error>> {
    if window.is_minimized().unwrap_or(false)
        || window.is_maximized().unwrap_or(false)
        || window.is_fullscreen().unwrap_or(false)
    {
        return Ok(());
    }

    let size = window.inner_size()?;
    if size.width < MIN_RESTORED_WINDOW_WIDTH || size.height < MIN_RESTORED_WINDOW_HEIGHT {
        return Ok(());
    }

    let position = window.outer_position().ok();

    let mut app_settings = settings.lock().map_err(|_| "Settings lock was poisoned")?;
    app_settings.window = WindowSettings {
        width: size.width,
        height: size.height,
        x: position.map(|position| position.x),
        y: position.map(|position| position.y),
    };
    settings::save(settings_path, &app_settings)?;
    Ok(())
}

fn restored_window_settings(saved: &WindowSettings, monitors: &[Monitor]) -> WindowSettings {
    let Some(monitor) = best_window_monitor(saved, monitors) else {
        return WindowSettings {
            width: saved.width.max(MIN_RESTORED_WINDOW_WIDTH),
            height: saved.height.max(MIN_RESTORED_WINDOW_HEIGHT),
            x: saved.x,
            y: saved.y,
        };
    };

    let monitor_position = monitor.position();
    let monitor_size = monitor.size();
    let monitor_left = i64::from(monitor_position.x);
    let monitor_top = i64::from(monitor_position.y);
    let monitor_width = monitor_size.width.max(MIN_RESTORED_WINDOW_WIDTH);
    let monitor_height = monitor_size.height.max(MIN_RESTORED_WINDOW_HEIGHT);
    let width = saved.width.clamp(MIN_RESTORED_WINDOW_WIDTH, monitor_width);
    let height = saved
        .height
        .clamp(MIN_RESTORED_WINDOW_HEIGHT, monitor_height);

    let x = saved.x.map(|x| {
        let max_x = monitor_left + i64::from(monitor_width.saturating_sub(width));
        i64::from(x).clamp(monitor_left, max_x) as i32
    });
    let y = saved.y.map(|y| {
        let max_y = monitor_top + i64::from(monitor_height.saturating_sub(height));
        i64::from(y).clamp(monitor_top, max_y) as i32
    });

    WindowSettings {
        width,
        height,
        x,
        y,
    }
}

fn best_window_monitor<'a>(
    window: &WindowSettings,
    monitors: &'a [Monitor],
) -> Option<&'a Monitor> {
    monitors
        .iter()
        .filter_map(|monitor| {
            let intersection = window_monitor_intersection_area(window, monitor);
            (intersection > 0).then_some((intersection, monitor))
        })
        .max_by_key(|(intersection, _)| *intersection)
        .map(|(_, monitor)| monitor)
        .or_else(|| monitors.first())
}

fn window_monitor_intersection_area(window: &WindowSettings, monitor: &Monitor) -> i64 {
    let (Some(x), Some(y)) = (window.x, window.y) else {
        return 0;
    };

    let window_left = i64::from(x);
    let window_top = i64::from(y);
    let window_right = window_left + i64::from(window.width);
    let window_bottom = window_top + i64::from(window.height);
    let monitor_position = monitor.position();
    let monitor_size = monitor.size();
    let monitor_left = i64::from(monitor_position.x);
    let monitor_top = i64::from(monitor_position.y);
    let monitor_right = monitor_left + i64::from(monitor_size.width);
    let monitor_bottom = monitor_top + i64::from(monitor_size.height);

    let intersection_width = window_right.min(monitor_right) - window_left.max(monitor_left);
    let intersection_height = window_bottom.min(monitor_bottom) - window_top.max(monitor_top);
    if intersection_width < 80 || intersection_height < 80 {
        return 0;
    }

    intersection_width * intersection_height
}

fn load_diagrams(db: &Connection) -> Result<Vec<DesignDiagram>, String> {
    let mut statement = db
        .prepare(
            "SELECT
                d.id,
                d.kind,
                d.key,
                d.title,
                d.source,
                d.diagram_type,
                d.attached_to_external_id,
                CASE
                    WHEN e.external_id IS NOT NULL THEN 'element'
                    WHEN r.external_id IS NOT NULL THEN 'relationship'
                    ELSE NULL
                END AS attached_to_target_type,
                d.sort_order
             FROM diagrams d
             LEFT JOIN c4_elements e
                ON e.workspace_id = d.workspace_id
                AND e.external_id = d.attached_to_external_id
             LEFT JOIN c4_relationships r
                ON r.workspace_id = d.workspace_id
                AND r.external_id = d.attached_to_external_id
             ORDER BY d.sort_order, d.id",
        )
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map([], |row| {
            let diagram_type: String = row.get(5)?;
            Ok(DesignDiagram {
                id: row.get(0)?,
                kind: row.get(1)?,
                key: row.get(2)?,
                title: row.get(3)?,
                source: row.get(4)?,
                artifact_role: design::diagram_artifact_role(&diagram_type).to_string(),
                artifact_label: design::diagram_artifact_label(&diagram_type).to_string(),
                artifact_rank: design::diagram_artifact_rank(&diagram_type),
                diagram_type,
                attached_to_external_id: row.get(6)?,
                attached_to_target_type: row.get(7)?,
                sort_order: row.get(8)?,
            })
        })
        .map_err(|err| err.to_string())?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| err.to_string())
}

fn load_design_elements(db: &Connection) -> Result<Vec<DesignElement>, String> {
    let mut statement = db
        .prepare(
            "SELECT id, external_id, parent_external_id, element_type, name, description, technology, tags
             FROM c4_elements
             ORDER BY parent_external_id IS NOT NULL, parent_external_id, id",
        )
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(DesignElement {
                id: row.get(0)?,
                external_id: row.get(1)?,
                parent_external_id: row.get(2)?,
                element_type: row.get(3)?,
                name: row.get(4)?,
                description: row.get(5)?,
                technology: row.get(6)?,
                tags: row.get(7)?,
            })
        })
        .map_err(|err| err.to_string())?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| err.to_string())
}

fn load_design_relationships(db: &Connection) -> Result<Vec<DesignRelationship>, String> {
    let mut statement = db
        .prepare(
            "SELECT id, external_id, source_external_id, destination_external_id, description, technology, tags
             FROM c4_relationships
             ORDER BY id",
        )
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(DesignRelationship {
                id: row.get(0)?,
                external_id: row.get(1)?,
                source_external_id: row.get(2)?,
                destination_external_id: row.get(3)?,
                description: row.get(4)?,
                technology: row.get(5)?,
                tags: row.get(6)?,
            })
        })
        .map_err(|err| err.to_string())?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| err.to_string())
}

fn ensure_element_exists(
    db: &Connection,
    workspace_id: i64,
    external_id: &str,
) -> Result<(), String> {
    let exists = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM c4_elements WHERE workspace_id = ?1 AND external_id = ?2)",
            rusqlite::params![workspace_id, external_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|err| err.to_string())?
        != 0;

    if exists {
        Ok(())
    } else {
        Err(format!("Unknown design element id: {external_id}"))
    }
}

fn load_structurizr_json(db: &Connection, workspace_id: i64) -> Result<Value, String> {
    let source = db
        .query_row(
            "SELECT structurizr_json FROM design_workspaces WHERE id = ?1",
            rusqlite::params![workspace_id],
            |row| row.get::<_, String>(0),
        )
        .map_err(|err| err.to_string())?;

    serde_json::from_str(&source).map_err(|err| err.to_string())
}

fn save_structurizr_json(
    db: &Connection,
    workspace_id: i64,
    workspace_json: &Value,
) -> Result<(), String> {
    let source = serde_json::to_string_pretty(workspace_json).map_err(|err| err.to_string())?;
    let dsl = build_structurizr_dsl(db, workspace_id)?;

    db.execute(
        "UPDATE design_workspaces
         SET structurizr_dsl = ?1, structurizr_json = ?2, updated_at = CURRENT_TIMESTAMP
         WHERE id = ?3",
        rusqlite::params![dsl, source, workspace_id],
    )
    .map_err(|err| err.to_string())?;

    db.execute(
        "UPDATE diagrams
         SET source = ?1, updated_at = CURRENT_TIMESTAMP
         WHERE workspace_id = ?2 AND kind = 'structurizr'",
        rusqlite::params![source, workspace_id],
    )
    .map_err(|err| err.to_string())?;

    Ok(())
}

fn build_structurizr_dsl(db: &Connection, workspace_id: i64) -> Result<String, String> {
    let (workspace_name, workspace_description): (String, String) = db
        .query_row(
            "SELECT name, description FROM design_workspaces WHERE id = ?1",
            rusqlite::params![workspace_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|err| err.to_string())?;
    let elements = load_design_elements(db)?;
    let relationships = load_design_relationships(db)?;
    let root_system = elements
        .iter()
        .find(|element| {
            element.parent_external_id.is_none()
                && element.element_type.eq_ignore_ascii_case("Software System")
        })
        .or_else(|| {
            elements
                .iter()
                .find(|element| element.parent_external_id.is_none())
        });

    let mut dsl = String::new();
    dsl.push_str(&format!(
        "workspace \"{}\" \"{}\" {{\n",
        escape_dsl(&workspace_name),
        escape_dsl(&workspace_description)
    ));
    dsl.push_str("    model {\n");

    for element in elements
        .iter()
        .filter(|element| element.parent_external_id.is_none())
    {
        if element.element_type.eq_ignore_ascii_case("Software System") {
            dsl.push_str(&format!(
                "        {} = softwareSystem \"{}\" \"{}\" {{\n",
                dsl_identifier(&element.external_id),
                escape_dsl(&element.name),
                escape_dsl(&element.description)
            ));

            for child in elements.iter().filter(|child| {
                child.parent_external_id.as_deref() == Some(element.external_id.as_str())
            }) {
                dsl.push_str(&format!(
                    "            {} = container \"{}\" \"{}\" \"{}\"{}\n",
                    dsl_identifier(&child.external_id),
                    escape_dsl(&child.name),
                    escape_dsl(&child.description),
                    escape_dsl(&child.technology),
                    dsl_tags(&child.tags)
                ));
            }

            dsl.push_str("        }\n");
        } else if element.element_type.eq_ignore_ascii_case("Person") {
            dsl.push_str(&format!(
                "        {} = person \"{}\" \"{}\"\n",
                dsl_identifier(&element.external_id),
                escape_dsl(&element.name),
                escape_dsl(&element.description)
            ));
        } else {
            dsl.push_str(&format!(
                "        {} = softwareSystem \"{}\" \"{}\"\n",
                dsl_identifier(&element.external_id),
                escape_dsl(&element.name),
                escape_dsl(&element.description)
            ));
        }
    }

    dsl.push('\n');

    for relationship in &relationships {
        dsl.push_str(&format!(
            "        {} -> {} \"{}\" \"{}\"{}\n",
            dsl_identifier(&relationship.source_external_id),
            dsl_identifier(&relationship.destination_external_id),
            escape_dsl(&relationship.description),
            escape_dsl(&relationship.technology),
            dsl_tags(&relationship.tags)
        ));
    }

    dsl.push_str("    }\n\n");
    dsl.push_str("    views {\n");

    if let Some(system) = root_system {
        dsl.push_str(&format!(
            "        container {} \"AdashiContainers\" {{\n",
            dsl_identifier(&system.external_id)
        ));
        dsl.push_str("            include *\n");
        dsl.push_str("            autolayout lr\n");
        dsl.push_str("        }\n");
    }

    dsl.push_str("    }\n");
    dsl.push('}');
    Ok(dsl)
}

fn dsl_identifier(external_id: &str) -> String {
    let sanitized: String = external_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect();

    format!("e_{sanitized}")
}

fn dsl_tags(tags: &str) -> String {
    let trimmed = tags.trim();

    if trimmed.is_empty() {
        String::new()
    } else {
        format!(" \"{}\"", escape_dsl(trimmed))
    }
}

fn escape_dsl(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn sync_structurizr_element(
    db: &Connection,
    workspace_id: i64,
    external_id: &str,
    name: &str,
    description: &str,
    technology: &str,
    tags: &str,
) -> Result<(), String> {
    let mut workspace_json = load_structurizr_json(db, workspace_id)?;
    let element = find_json_element_mut(&mut workspace_json, external_id)
        .ok_or_else(|| format!("Element {external_id} is missing from Structurizr JSON"))?;

    element["name"] = json!(name);
    element["description"] = json!(description);
    element["technology"] = json!(technology);
    element["tags"] = json!(tags);
    save_structurizr_json(db, workspace_id, &workspace_json)
}

fn sync_structurizr_relationship(
    db: &Connection,
    workspace_id: i64,
    external_id: &str,
    description: &str,
    technology: &str,
    tags: &str,
) -> Result<(), String> {
    let mut workspace_json = load_structurizr_json(db, workspace_id)?;
    let relationship = find_json_relationship_mut(&mut workspace_json, external_id)
        .ok_or_else(|| format!("Relationship {external_id} is missing from Structurizr JSON"))?;

    relationship["description"] = json!(description);
    relationship["technology"] = json!(technology);
    relationship["tags"] = json!(tags);
    save_structurizr_json(db, workspace_id, &workspace_json)
}

fn sync_structurizr_new_relationship(
    db: &Connection,
    workspace_id: i64,
    external_id: &str,
    source_external_id: &str,
    destination_external_id: &str,
    description: &str,
    technology: &str,
    tags: &str,
) -> Result<(), String> {
    let mut workspace_json = load_structurizr_json(db, workspace_id)?;
    let relationship = json!({
        "id": external_id,
        "tags": tags,
        "sourceId": source_external_id,
        "destinationId": destination_external_id,
        "description": description,
        "technology": technology
    });

    let source_element = find_json_element_mut(&mut workspace_json, source_external_id)
        .ok_or_else(|| format!("Element {source_external_id} is missing from Structurizr JSON"))?;
    match source_element
        .get_mut("relationships")
        .and_then(Value::as_array_mut)
    {
        Some(relationships) => relationships.push(relationship),
        None => source_element["relationships"] = json!([relationship]),
    }

    if let Some(view_elements) = workspace_json
        .pointer_mut("/views/containerViews/0/elements")
        .and_then(Value::as_array_mut)
    {
        if let Some(view_element) = view_elements
            .iter_mut()
            .find(|element| element.get("id").and_then(Value::as_str) == Some(source_external_id))
        {
            match view_element
                .get_mut("relationships")
                .and_then(Value::as_array_mut)
            {
                Some(relationships) => relationships.push(json!(external_id)),
                None => view_element["relationships"] = json!([external_id]),
            }
        }
    }

    save_structurizr_json(db, workspace_id, &workspace_json)
}

fn find_json_element_mut<'a>(value: &'a mut Value, external_id: &str) -> Option<&'a mut Value> {
    if value.get("id").and_then(Value::as_str) == Some(external_id) {
        return Some(value);
    }

    match value {
        Value::Array(items) => {
            for item in items {
                if let Some(found) = find_json_element_mut(item, external_id) {
                    return Some(found);
                }
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                if let Some(found) = find_json_element_mut(item, external_id) {
                    return Some(found);
                }
            }
        }
        _ => {}
    }

    None
}

fn find_json_relationship_mut<'a>(
    value: &'a mut Value,
    external_id: &str,
) -> Option<&'a mut Value> {
    match value {
        Value::Array(items) => {
            for item in items {
                if item.get("id").and_then(Value::as_str) == Some(external_id)
                    && item.get("sourceId").is_some()
                    && item.get("destinationId").is_some()
                {
                    return Some(item);
                }

                if let Some(found) = find_json_relationship_mut(item, external_id) {
                    return Some(found);
                }
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                if let Some(found) = find_json_relationship_mut(item, external_id) {
                    return Some(found);
                }
            }
        }
        _ => {}
    }

    None
}

fn load_guidelines(db: &Connection) -> Result<Vec<Guideline>, String> {
    let mut statement = db
        .prepare("SELECT id, title, body FROM coding_guidelines ORDER BY id")
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(Guideline {
                id: row.get(0)?,
                title: row.get(1)?,
                body: row.get(2)?,
            })
        })
        .map_err(|err| err.to_string())?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| err.to_string())
}

fn load_post_task_commands(db: &Connection) -> Result<Vec<PostTaskCommand>, String> {
    let mut statement = db
        .prepare("SELECT id, label, command, trigger FROM post_task_commands ORDER BY id")
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(PostTaskCommand {
                id: row.get(0)?,
                label: row.get(1)?,
                command: row.get(2)?,
                trigger: row.get(3)?,
            })
        })
        .map_err(|err| err.to_string())?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| err.to_string())
}

fn load_qa_checks(db: &Connection) -> Result<Vec<QaCheck>, String> {
    let mut statement = db
        .prepare("SELECT id, label, command, required FROM qa_checks ORDER BY id")
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(QaCheck {
                id: row.get(0)?,
                label: row.get(1)?,
                command: row.get(2)?,
                required: row.get::<_, i64>(3)? != 0,
            })
        })
        .map_err(|err| err.to_string())?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_project_database_creates_seeded_project_store() {
        let folder = temp_project_folder("adashi-project-init");
        fs::create_dir_all(&folder).unwrap();
        let project = settings::new_project(
            "RaySplatter".to_string(),
            folder.to_string_lossy().to_string(),
        );

        let db = open_project_database(&project).unwrap();

        verify_project_database(&db).unwrap();
        assert!(settings::project_database_path(&project).exists());
        assert_eq!(
            db.query_row("SELECT repository_path FROM projects LIMIT 1", [], |row| {
                row.get::<_, String>(0)
            })
            .unwrap(),
            project.folder
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM project_memory", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            1
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM fixed_hook_prompts", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            2
        );
        assert_eq!(
            db.query_row(
                "SELECT name FROM c4_elements WHERE external_id = '1'",
                [],
                |row| { row.get::<_, String>(0) }
            )
            .unwrap(),
            "RaySplatter"
        );
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM c4_elements WHERE name = 'Adashi'",
                [],
                |row| { row.get::<_, i64>(0) }
            )
            .unwrap(),
            0
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM agent_tasks", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            0
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM qa_jobs", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            0
        );

        drop(db);
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn open_project_database_repairs_non_adashi_demo_seed() {
        let folder = temp_project_folder("adashi-project-repair");
        fs::create_dir_all(&folder).unwrap();
        let bad_seed_project = ProjectSettings {
            id: "adashi".to_string(),
            name: "Adashi".to_string(),
            folder: folder.to_string_lossy().to_string(),
        };
        let ray_project = ProjectSettings {
            id: "raysplatter".to_string(),
            name: "RaySplatter".to_string(),
            folder: folder.to_string_lossy().to_string(),
        };

        drop(open_project_database(&bad_seed_project).unwrap());
        let db = open_project_database(&ray_project).unwrap();

        assert_eq!(
            db.query_row(
                "SELECT name FROM c4_elements WHERE external_id = '1'",
                [],
                |row| { row.get::<_, String>(0) }
            )
            .unwrap(),
            "RaySplatter"
        );
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM c4_elements WHERE name = 'Adashi'",
                [],
                |row| { row.get::<_, i64>(0) }
            )
            .unwrap(),
            0
        );
        assert_eq!(
            db.query_row(
                "SELECT key FROM diagrams WHERE kind = 'structurizr'",
                [],
                |row| { row.get::<_, String>(0) }
            )
            .unwrap(),
            "ProjectContext"
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM agent_tasks", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            0
        );

        drop(db);
        let _ = fs::remove_dir_all(folder);
    }

    fn temp_project_folder(label: &str) -> PathBuf {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        std::env::temp_dir().join(format!("{label}-{}-{millis}", std::process::id()))
    }
}
