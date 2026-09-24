#[cfg(test)]
use rusqlite::{Connection, OptionalExtension};
use crate::storage::{ProjectStorage,StorageError};
use crate::storage::api::{ReadSnapshot,Mutation,Change,TaskWrite,QaWrite,RuleWrite,MemoryWrite,MockupWrite,DesignWrite,FixedPromptWrite,LocalDerivedCache,HealthCacheKey};
use crate::project::open_project_store;
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
#[cfg(test)]
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{
    AppHandle, Manager, Monitor, PhysicalPosition, PhysicalSize, State, WebviewWindow, WindowEvent,
};
use tauri_plugin_dialog::DialogExt;

use crate::design::{DesignArtifactTypeRecord, DesignChange};
use crate::design_health;
use crate::fixed_hooks::FixedHookPrompt;
use crate::memory::ProjectMemory;
use crate::mockups::{
    CreateMockupInput, MockupMutationInput, MockupSummary, SaveDraftInput, UiMockup,
};
use crate::qa::{NewQaJob, QaDesignLinkInput, QaJob, QaJobQuery, QaRun, UpdateQaJob};
use crate::projection::{self, ProjectionStatus};
use crate::prompt_hygiene::{self, PromptWarning};
use crate::rules::{NewRule, Rule};
use crate::settings::{
    AppSettings, ProjectSettings, RuleTemplate, RuleTemplateDraft, WindowSettings,
    architecture_file_name, architecture_projection_enabled,
};
#[cfg(test)]
use crate::state as project_state;
use crate::tasks::{FinishTask, NewTask, Task, TaskDesignSpecificationLinkInput, UpdateTask};

#[path = "storage_commands.rs"]
mod storage_commands;

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
    change_cursor: crate::storage::ChangeCursor,
    workspace_name: String,
    workspace_description: String,
    structurizr_dsl: String,
    structurizr_workspace: String,
    structurizr_view_key: String,
    design_elements: Vec<DesignElement>,
    design_relationships: Vec<DesignRelationship>,
    uml_artifact_types: Vec<DesignArtifactTypeRecord>,
    diagrams: Vec<DesignDiagram>,
    mockups: Vec<MockupSummary>,
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
    architecture_projection: ProjectionStatus,
    prompt_warnings: Vec<PromptWarning>,
    /// The last recorded design-to-code check, so the design view can show state per element
    /// without reading the source tree on every dashboard refresh.
    design_health: design_health::DesignHealthResult,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DesignDiagram {
    version: i64,
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
    version: i64,
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
    version: i64,
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
    let project=resolve_project(&state,project_id.as_deref())?;
    let mut store=open_project_store(&project).map_err(|e|e.to_string())?;
    load_dashboard_payload(project,&mut store,&state)
}

fn load_dashboard_payload(project:ProjectSettings,store:&mut dyn ProjectStorage,state:&AppState) -> Result<DashboardPayload,String> {
    let snapshot=store.snapshot().map_err(|e|e.to_string())?;
    let db=snapshot.as_ref();
    let inventory=db.design_inventory().map_err(|e|e.to_string())?;
    let legacy=db.legacy_content().map_err(|e|e.to_string())?;
    let revision=db.metadata().revision;
    let workspace=inventory.workspace;
    let mut diagrams=inventory.diagrams;
    diagrams.sort_by_key(|v|(v.value.sort_order,v.id));
    let structurizr_view_key=diagrams.iter().find(|d|d.value.language=="structurizr").map(|d|d.value.key.clone()).unwrap_or_else(||"ProjectContext".into());
    let architecture_projection={
        let settings=state.settings.lock().map_err(|_|"Settings lock poisoned".to_string())?.clone();
        let file_name=architecture_file_name(&settings,&project.id);
        let enabled=architecture_projection_enabled(&settings,&project.id);
        let mut status=projection::status(db,Path::new(&project.folder),&file_name,enabled)?;
        if let Err(error)=projection::regenerate(db,Path::new(&project.folder),&file_name,enabled) {status.error=Some(error);}
        status
    };
    let rules=db.rules().map_err(|e|e.to_string())?;
    let rule_templates=load_rule_templates(state)?;
    let fixed_hook_prompts=db.fixed_prompts().map_err(|e|e.to_string())?;
    let prompt_warnings=collect_prompt_warnings(&rules,&fixed_hook_prompts,&rule_templates);
    let design_health=recorded_design_health(db,Path::new(&project.folder))?;
    Ok(DashboardPayload {
        project_id:project.id,project_name:project.name,project_folder:project.folder,revision,
        change_cursor:db.metadata().cursor.clone(),
        workspace_name:workspace.name,workspace_description:workspace.description,
        structurizr_dsl:workspace.structurizr_dsl,structurizr_workspace:workspace.structurizr_json,structurizr_view_key,
        design_elements:inventory.elements.into_iter().map(|v|DesignElement {id:v.id,version:v.value.version,external_id:v.value.external_id,parent_external_id:v.value.parent_external_id,element_type:v.value.element_type,name:v.value.name,description:v.value.description,technology:v.value.technology,tags:v.value.tags}).collect(),
        design_relationships:inventory.relationships.into_iter().map(|v|DesignRelationship {id:v.id,version:v.value.version,external_id:v.value.external_id,source_external_id:v.value.source_external_id,destination_external_id:v.value.destination_external_id,description:v.value.description,technology:v.value.technology,tags:v.value.tags}).collect(),
        diagrams:diagrams.into_iter().map(|v|DesignDiagram {id:v.id,version:v.value.version,kind:v.value.language,key:v.value.key,title:v.value.title,source:v.value.source,diagram_type:v.value.diagram_type,artifact_role:v.value.artifact_role,artifact_label:v.value.artifact_label,artifact_rank:v.value.artifact_rank,attached_to_external_id:v.value.attached_to_external_id,attached_to_target_type:v.value.attached_to_target_type,sort_order:v.value.sort_order}).collect(),
        uml_artifact_types:design::supported_uml_artifact_types(),mockups:db.mockups(false).map_err(|e|e.to_string())?,
        tasks:db.tasks(&tasks::ALL_TASK_STATES).map_err(|e|e.to_string())?,
        guidelines:legacy.guidelines.into_iter().map(|v|Guideline {id:v.id,title:v.title,body:v.body}).collect(),
        post_task_commands:legacy.post_task_commands.into_iter().map(|v|PostTaskCommand {id:v.id,label:v.label,command:v.command,trigger:v.trigger}).collect(),
        qa_checks:legacy.qa_checks.into_iter().map(|v|QaCheck {id:v.id,label:v.label,command:v.command,required:v.required}).collect(),
        qa_jobs:db.qa_jobs(&QaJobQuery::default()).map_err(|e|e.to_string())?,qa_runs:db.qa_runs(20).map_err(|e|e.to_string())?,
        rules,rule_templates,fixed_hook_prompts,memory:db.memory().map_err(|e|e.to_string())?,architecture_projection,prompt_warnings,design_health,
    })
}

/// The recorded design-to-code check, assembled without touching the filesystem.
///
/// The scan itself reads source files, so it runs on request rather than on every dashboard
/// refresh; this rebuilds the same shape from the recorded rows so the design view always has
/// something to show, including the honest "never scanned" state.
fn recorded_design_health(db:&dyn ReadSnapshot,folder:&Path) -> Result<design_health::DesignHealthResult,String> {
    let key=HealthCacheKey {project_id:db.metadata().identity.id.clone(),computer_id:crate::computer::id()?.into(),cursor:db.metadata().cursor.clone()};
    let mut result=crate::storage::local_cache::FileDerivedCache::for_checkout(folder).health(&key).ok().flatten().unwrap_or_else(||design_health::DesignHealthResult {
        counts:Default::default(),elements:vec![],summary:"Not scanned for the current project revision.".into(),
        not_checked:"These checks locate bound code; they do not verify its implementation.".into(),
    });
    for element in &mut result.elements {element.waivers=db.health_waivers(&element.design_external_id).map_err(|e|e.to_string())?;}
    result.counts.waivers=result.elements.iter().map(|e|e.waivers.len() as u32).sum();
    Ok(result)
}

/// Runs the design-to-code check for a project and returns the refreshed dashboard.
#[tauri::command]
fn rescan_design_health(
    project_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project=resolve_project(&state,project_id.as_deref())?;
    let mut store=open_project_store(&project).map_err(|e|e.to_string())?;
    {let snapshot=store.snapshot().map_err(|e|e.to_string())?;
     design_health::scan_snapshot(snapshot.as_ref(),Path::new(&project.folder))?;}
    load_dashboard_payload(project,&mut store,&state)
}

/// Stored prompts that name tools which no longer exist. Detecting these turns a silent rot
/// into something a human can repair.
fn collect_prompt_warnings(
    rules: &[Rule],
    fixed_hook_prompts: &[FixedHookPrompt],
    rule_templates: &[RuleTemplate],
) -> Vec<PromptWarning> {
    let mut warnings = Vec::new();
    for rule in rules {
        if let Some(warning) = prompt_hygiene::warning_for(
            &format!("rule:{}", rule.id),
            &rule.name,
            &rule.prompt,
        ) {
            warnings.push(warning);
        }
    }
    for prompt in fixed_hook_prompts {
        if let Some(warning) = prompt_hygiene::warning_for(
            &format!("fixed-hook:{}", prompt.key),
            &prompt.title,
            &prompt.prompt,
        ) {
            warnings.push(warning);
        }
    }
    for template in rule_templates {
        if let Some(warning) = prompt_hygiene::warning_for(
            &format!("rule-template:{}", template.id),
            &template.name,
            &template.prompt,
        ) {
            warnings.push(warning);
        }
    }
    warnings
}

#[tauri::command]
fn get_mockup(
    project_id: String,
    external_id: String,
    state: State<'_, AppState>,
) -> Result<UiMockup, String> {
    let project=resolve_project(&state,Some(&project_id))?;
    let mut store=open_project_store(&project).map_err(|e|e.to_string())?;
    let snapshot=store.snapshot().map_err(|e|e.to_string())?;
    snapshot.mockup(external_id.trim()).map_err(|e|e.to_string())
}

#[tauri::command]
fn create_mockup(
    project_id: String,
    input: CreateMockupInput,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project=resolve_project(&state,Some(&project_id))?;
    mutate_dashboard(project,input.operation_id.clone(),vec![Change::Mockup(MockupWrite::Create(input))],&state)
}

#[tauri::command]
fn save_mockup_draft(
    project_id: String,
    input: SaveDraftInput,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project=resolve_project(&state,Some(&project_id))?;
    mutate_dashboard(project,input.operation_id.clone(),vec![Change::Mockup(MockupWrite::SaveDraft(input))],&state)
}

macro_rules! mockup_lifecycle_command {
    ($name:ident, $runtime:ident) => {
        #[tauri::command]
        fn $name(
            project_id: String,
            input: MockupMutationInput,
            state: State<'_, AppState>,
        ) -> Result<DashboardPayload, String> {
            let project=resolve_project(&state,Some(&project_id))?;
            mutate_dashboard(project,input.operation_id.clone(),vec![Change::Mockup(MockupWrite::$runtime(input))],&state)
        }
    };
}

mockup_lifecycle_command!(request_mockup_revision, RequestRevision);
mockup_lifecycle_command!(resume_mockup_editing, ResumeEditing);
mockup_lifecycle_command!(accept_mockup_proposal, AcceptProposal);
mockup_lifecycle_command!(reject_mockup_proposal, RejectProposal);
mockup_lifecycle_command!(discard_mockup_draft, DiscardDraft);

#[tauri::command]
fn delete_mockup(
    project_id: String,
    input: MockupMutationInput,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project=resolve_project(&state,Some(&project_id))?;
    mutate_dashboard(project,input.operation_id.clone(),vec![Change::Mockup(MockupWrite::Delete(input))],&state)
}

fn mutate_dashboard(project:ProjectSettings,operation_id:String,changes:Vec<Change>,state:&AppState) -> Result<DashboardPayload,String> {
    let mut store=open_project_store(&project).map_err(|e|e.to_string())?;
    store.commit(Mutation {operation_id,changes}).map_err(desktop_storage_error)?;
    load_dashboard_payload(project,&mut store,state)
}

fn desktop_storage_error(error:StorageError) -> String {
    match error {
        StorageError::Conflict(conflicts)=>serde_json::json!({"code":"resource.conflict","conflicts":conflicts}).to_string(),
        StorageError::DesignRejected(result)=>serde_json::to_string(&result.errors).unwrap_or_else(|_|"Design validation failed".into()),
        other=>other.to_string(),
    }
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
    change_cursor: crate::storage::ChangeCursor,
    updated_at: String,
}

#[tauri::command]
fn get_project_revision(
    project_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<ProjectRevisionPayload, String> {
    let project=resolve_project(&state,project_id.as_deref())?;
    let mut store=open_project_store(&project).map_err(|e|e.to_string())?;
    let snapshot=store.snapshot().map_err(|e|e.to_string())?;
    let metadata=snapshot.metadata();
    Ok(ProjectRevisionPayload {project_id:project.id,revision:metadata.revision,change_cursor:metadata.cursor.clone(),updated_at:metadata.updated_at.clone()})
}

#[tauri::command]
fn close_app(app: AppHandle) -> Result<(), String> {
    app.exit(0);
    Ok(())
}

/// Opens a design-bound file in whatever the system has associated with it, so a file listed in
/// the inspector is one click from the editor.
///
/// The path is resolved against the project folder and refused if it escapes it. The renderer
/// sends a project-relative path, and this is the boundary where that claim is checked rather
/// than trusted.
#[tauri::command]
fn open_bound_file(
    project_id: Option<String>,
    relative_path: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;

    let project = resolve_project(&state, project_id.as_deref())?;
    let root = PathBuf::from(&project.folder)
        .canonicalize()
        .map_err(|error| format!("Project folder is not reachable: {error}"))?;
    let candidate = root.join(relative_path.trim().replace('\\', "/"));
    let resolved = candidate
        .canonicalize()
        .map_err(|_| format!("No such file in this project: {}", relative_path.trim()))?;
    if !resolved.starts_with(&root) {
        return Err(format!(
            "Refusing to open a path outside the project folder: {}",
            relative_path.trim()
        ));
    }
    if !resolved.is_file() {
        return Err(format!("Not a file: {}", relative_path.trim()));
    }

    app.opener()
        .open_path(resolved.to_string_lossy().to_string(), None::<&str>)
        .map_err(|error| error.to_string())
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
    add_project_to_settings(name, folder, &state)
}

fn add_project_to_settings(
    name: String,
    folder: String,
    state: &AppState,
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

    if let Some(existing) = duplicate_project_name(&settings, &project) {
        return Err(format!(
            "A project named '{}' is already registered",
            existing.name
        ));
    }

    drop(settings);

    let mut store=open_project_store(&project).map_err(|e|e.to_string())?;
    verify_project_database(store.snapshot().map_err(|e|e.to_string())?.as_ref())?;

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

    if let Some(existing) = duplicate_project_name(&settings, &project) {
        return Err(format!(
            "A project named '{}' is already registered",
            existing.name
        ));
    }

    settings.last_active_project_id = Some(project.id.clone());
    settings.projects.push(project);
    settings::save(&state.settings_path, &settings).map_err(|err| err.to_string())?;
    Ok(settings.clone())
}

/// Project names must stay distinct so a name reference always resolves to one project.
/// Comparison folds case; the stored name is still displayed exactly as written.
fn duplicate_project_name<'a>(
    settings: &'a AppSettings,
    candidate: &ProjectSettings,
) -> Option<&'a ProjectSettings> {
    settings
        .projects
        .iter()
        .find(|existing| existing.name.eq_ignore_ascii_case(candidate.name.trim()))
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
    projection::forget_project(&mut settings, &project_id);

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
    operation_id: String,
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
    operation_id: String,
    expected_version: i64,
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
    operation_id: String,
    template_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateMemoryRuleRequest {
    project_id: Option<String>,
    operation_id: String,
    expected_version: i64,
    rule: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateMemoryRequest {
    project_id: Option<String>,
    operation_id: String,
    expected_version: i64,
    memory: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateFixedHookPromptRequest {
    project_id: Option<String>,
    operation_id: String,
    expected_version: i64,
    key: String,
    prompt: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateDesignElementRequest {
    project_id: Option<String>,
    operation_id: String,
    expected_version: i64,
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
    operation_id: String,
    expected_version: i64,
    external_id: String,
    description: String,
    technology: String,
    tags: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateDesignRelationshipRequest {
    project_id: Option<String>,
    operation_id: String,
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
    operation_id: String,
    title: String,
    description: Option<String>,
    design_specification_links: Option<Vec<TaskDesignSpecificationLinkInput>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateTaskRequest {
    project_id: Option<String>,
    operation_id: String,
    expected_version: i64,
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
    operation_id: String,
    expected_version: i64,
    task_id: i64,
    completion_memo: String,
    created_files: Vec<String>,
    changed_files: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateQaJobRequest {
    project_id: Option<String>,
    operation_id: String,
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
    operation_id: String,
    expected_version: i64,
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
    operation_id: String,
    query: QaJobQuery,
    trigger_source: Option<String>,
}







#[tauri::command]
fn create_rule(
    input: CreateRuleRequest,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project=resolve_project(&state,input.project_id.as_deref())?;
    mutate_dashboard(project,input.operation_id,vec![Change::Rule(RuleWrite::Create {input:NewRule {name:input.name,enabled:input.enabled,intend:input.intend,hook:input.hook,prompt:input.prompt} })],&state)
}

#[tauri::command]
fn update_rule(
    input: UpdateRuleRequest,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project=resolve_project(&state,input.project_id.as_deref())?;
    mutate_dashboard(project,input.operation_id,vec![Change::Rule(RuleWrite::Update {id:input.id,expected_version:input.expected_version,input:NewRule {name:input.name,enabled:input.enabled,intend:input.intend,hook:input.hook,prompt:input.prompt} })],&state)
}

#[tauri::command]
fn delete_rule(
    project_id: Option<String>,
    rule_id: i64,
    operation_id: String,
    expected_version: i64,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project=resolve_project(&state,project_id.as_deref())?;
    mutate_dashboard(project,operation_id,vec![Change::Rule(RuleWrite::Delete {id:rule_id,expected_version})],&state)
}

#[tauri::command]
fn save_rule_template(
    input: SaveRuleTemplateRequest,
    state: State<'_, AppState>,
) -> Result<AppSettings, String> {
    let project = resolve_project(&state, input.project_id.as_deref())?;
    let mut store=open_project_store(&project).map_err(|e|e.to_string())?;
    let rule=store.snapshot().map_err(|e|e.to_string())?.rules().map_err(|e|e.to_string())?.into_iter().find(|r|r.id==input.rule_id).ok_or_else(||format!("Unknown rule id: {}",input.rule_id))?;
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
    let project=resolve_project(&state,input.project_id.as_deref())?;
    let template = state
        .settings
        .lock()
        .map_err(|_| "Settings lock was poisoned".to_string())?
        .rule_templates
        .iter()
        .find(|template| template.id == input.template_id)
        .cloned()
        .ok_or_else(|| format!("Unknown rule template id: {}", input.template_id))?;
    mutate_dashboard(project,input.operation_id,vec![Change::Rule(RuleWrite::Create {input:NewRule {name:template.name,enabled:template.enabled,intend:template.intend,hook:template.hook,prompt:template.prompt}})],&state)
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
    let project=resolve_project(&state,input.project_id.as_deref())?;
    mutate_dashboard(project,input.operation_id,vec![Change::Memory(MemoryWrite::Protocol {expected_version:input.expected_version,rule:input.rule})],&state)
}

#[tauri::command]
fn update_memory(
    input: UpdateMemoryRequest,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project=resolve_project(&state,input.project_id.as_deref())?;
    mutate_dashboard(project,input.operation_id,vec![Change::Memory(MemoryWrite::Compact {expected_version:input.expected_version,summary:input.memory,superseded_note_ids:vec![]})],&state)
}

#[tauri::command]
fn update_fixed_hook_prompt(
    input: UpdateFixedHookPromptRequest,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project=resolve_project(&state,input.project_id.as_deref())?;
    mutate_dashboard(project,input.operation_id,vec![Change::FixedPrompt(FixedPromptWrite {key:input.key,expected_version:input.expected_version,prompt:input.prompt})],&state)
}

#[tauri::command]
fn update_design_element(
    input: UpdateDesignElementRequest,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project=resolve_project(&state,input.project_id.as_deref())?;
    mutate_dashboard(project,input.operation_id,vec![Change::Design(DesignWrite::EditElement {external_id:input.external_id,expected_version:input.expected_version,name:input.name,description:input.description,technology:input.technology,tags:input.tags})],&state)
}

#[tauri::command]
fn update_design_relationship(
    input: UpdateDesignRelationshipRequest,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project=resolve_project(&state,input.project_id.as_deref())?;
    mutate_dashboard(project,input.operation_id,vec![Change::Design(DesignWrite::EditRelationship {external_id:input.external_id,expected_version:input.expected_version,description:input.description,technology:input.technology,tags:input.tags})],&state)
}

#[tauri::command]
fn create_design_relationship(
    input: CreateDesignRelationshipRequest,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project=resolve_project(&state,input.project_id.as_deref())?;
    if input.source_external_id==input.destination_external_id {return Err("A relationship must connect two different elements".into());}
    let relationship_suffix = input
        .operation_id
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || *character == '-')
        .take(120)
        .collect::<String>();
    if relationship_suffix.is_empty() {
        return Err("operationId must contain an ASCII letter or digit".to_string());
    }
    let external_id = format!("ui-rel-{relationship_suffix}");
    mutate_dashboard(project,input.operation_id,vec![Change::Design(DesignWrite::Save {change_intent:"Create a design relationship from the dashboard.".into(),read_tokens:vec![],changes:vec![DesignChange::UpsertRelationship {external_id,source_external_id:input.source_external_id,destination_external_id:input.destination_external_id,description:input.description,technology:Some(input.technology),tags:Some(input.tags)}]})],&state)
}

#[tauri::command]
fn create_task(
    input: CreateTaskRequest,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project=resolve_project(&state,input.project_id.as_deref())?;
    mutate_dashboard(project,input.operation_id,vec![Change::Task(TaskWrite::Create { input:NewTask {
                title: input.title,
                description: input.description,
                design_specification_links: input.design_specification_links,
            } })],&state)
}

#[tauri::command]
fn update_task(
    input: UpdateTaskRequest,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project=resolve_project(&state,input.project_id.as_deref())?;
    mutate_dashboard(project,input.operation_id,vec![Change::Task(TaskWrite::Update { expected_version:input.expected_version,input:UpdateTask {
                task_id: input.task_id,
                title: input.title,
                description: input.description,
                state: input.state,
                design_specification_links: input.design_specification_links,
            } })],&state)
}

#[tauri::command]
fn finish_task(
    input: FinishTaskRequest,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project=resolve_project(&state,input.project_id.as_deref())?;
    mutate_dashboard(project,input.operation_id,vec![Change::Task(TaskWrite::Finish { expected_version:input.expected_version,input:FinishTask {
                task_id: input.task_id,
                completion_memo: input.completion_memo,
                created_files: input.created_files,
                changed_files: input.changed_files,
            } })],&state)
}

#[tauri::command]
fn close_task(
    project_id: Option<String>,
    task_id: i64,
    operation_id: String,
    expected_version: i64,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project=resolve_project(&state,project_id.as_deref())?;
    mutate_dashboard(project,operation_id,vec![Change::Task(TaskWrite::Close {id:task_id,expected_version})],&state)
}

#[tauri::command]
fn delete_task(
    project_id: Option<String>,
    task_id: i64,
    operation_id: String,
    expected_version: i64,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project=resolve_project(&state,project_id.as_deref())?;
    mutate_dashboard(project,operation_id,vec![Change::Task(TaskWrite::Delete {id:task_id,expected_version})],&state)
}

#[tauri::command]
fn create_qa_job(
    input: CreateQaJobRequest,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project=resolve_project(&state,input.project_id.as_deref())?;
    mutate_dashboard(project,input.operation_id,vec![Change::Qa(QaWrite::CreateJob { input:NewQaJob {
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
            } })],&state)
}

#[tauri::command]
fn update_qa_job(
    input: UpdateQaJobRequest,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project=resolve_project(&state,input.project_id.as_deref())?;
    mutate_dashboard(project,input.operation_id,vec![Change::Qa(QaWrite::UpdateJob { expected_version:input.expected_version,input:UpdateQaJob {
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
            } })],&state)
}

#[tauri::command]
fn delete_qa_job(
    project_id: Option<String>,
    qa_job_id: i64,
    operation_id: String,
    expected_version: i64,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project=resolve_project(&state,project_id.as_deref())?;
    mutate_dashboard(project,operation_id,vec![Change::Qa(QaWrite::DeleteJob {id:qa_job_id,expected_version})],&state)
}

#[tauri::command]
fn run_qa_jobs(
    input: RunQaJobsRequest,
    state: State<'_, AppState>,
) -> Result<DashboardPayload, String> {
    let project=resolve_project(&state,input.project_id.as_deref())?;
    let mut store=open_project_store(&project).map_err(|e|e.to_string())?;
    crate::qa_runner::run(&mut store,&project.folder,&input.operation_id,input.query,input.trigger_source.as_deref().unwrap_or("dashboard")).map_err(|e|e.to_string())?;
    load_dashboard_payload(project,&mut store,&state)
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
            storage_commands::get_project_storage,
            storage_commands::preview_storage_migration,
            storage_commands::migrate_project_storage,
            add_project,
            close_app,
            close_task,
            rescan_design_health,
            open_bound_file,
            accept_mockup_proposal,
            create_mockup,
            create_qa_job,
            create_rule,
            create_rule_from_template,
            create_task,
            delete_qa_job,
            delete_rule,
            delete_rule_template,
            create_design_relationship,
            delete_project,
            delete_mockup,
            delete_task,
            finish_task,
            get_app_settings,
            get_dashboard,
            get_mockup,
            get_project_revision,
            pick_project_folder,
            set_active_project,
            set_architecture_file_name,
            set_project_architecture_projection,
            save_rule_template,
            save_mockup_draft,
            request_mockup_revision,
            resume_mockup_editing,
            reject_mockup_proposal,
            discard_mockup_draft,
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
    project_ref: Option<&str>,
) -> Result<ProjectSettings, String> {
    let project_ref = project_ref
        .map(str::trim)
        .filter(|project_ref| !project_ref.is_empty())
        .ok_or_else(|| {
            "projectName is required and must be a configured project id or project name".to_string()
        })?;

    settings
        .projects
        .iter()
        .find(|project| project.id == project_ref || project.name.eq_ignore_ascii_case(project_ref))
        .cloned()
        .ok_or_else(|| format!("Unknown project id or name: {project_ref}"))
}

fn verify_project_database(snapshot:&dyn ReadSnapshot) -> Result<(),String> {
    snapshot.memory().map_err(|e|e.to_string())?;
    snapshot.design_inventory().map_err(|e|e.to_string())?;
    if snapshot.fixed_prompts().map_err(|e|e.to_string())?.len()<2 {return Err("Project database is missing default fixed hook prompts".into());}
    Ok(())
}

/// Removes blocks left under a previous file name and regenerates under the current one.
///
/// A project whose folder is gone is skipped rather than failing the whole refresh: its
/// projection is unreachable either way.
fn refresh_architecture_projection(
    settings: &AppSettings,
    previous_names: &[(String, String)],
) -> Result<(), String> {
    for project in &settings.projects {
        let folder = Path::new(&project.folder);
        if !folder.is_dir() {
            continue;
        }

        let file_name = architecture_file_name(settings, &project.id);
        if let Some((_, previous)) = previous_names
            .iter()
            .find(|(id, _)| id == &project.id)
        {
            if previous != &file_name {
                projection::remove_managed_blocks(folder, previous)?;
            }
        }

        let mut store=open_project_store(project).map_err(|e|e.to_string())?;
        let db=store.snapshot().map_err(|e|e.to_string())?;
        if architecture_projection_enabled(settings, &project.id) {
            projection::regenerate(db.as_ref(), folder, &file_name, true)?;
        } else {
            projection::remove_managed_blocks(folder, &file_name)?;
        }
    }
    Ok(())
}

fn resolved_architecture_names(settings: &AppSettings) -> Vec<(String, String)> {
    settings
        .projects
        .iter()
        .map(|project| {
            (
                project.id.clone(),
                architecture_file_name(settings, &project.id),
            )
        })
        .collect()
}

fn store_settings(state: &AppState, settings: AppSettings) -> Result<AppSettings, String> {
    settings::save(&state.settings_path, &settings).map_err(|error| error.to_string())?;
    let mut stored = state
        .settings
        .lock()
        .map_err(|_| "Settings lock was poisoned".to_string())?;
    *stored = settings.clone();
    Ok(settings)
}

/// Sets the instruction-file name used for generated architecture blocks. Default `AGENTS.md`.
///
/// Renaming removes the previous name's blocks first, so an orphaned block cannot keep being
/// injected into agents after the setting changes.
#[tauri::command]
fn set_architecture_file_name(
    file_name: String,
    state: State<'_, AppState>,
) -> Result<AppSettings, String> {
    let mut settings = state
        .settings
        .lock()
        .map_err(|_| "Settings lock was poisoned".to_string())?
        .clone();

    let previous_names = resolved_architecture_names(&settings);
    settings.architecture_projection.file_name =
        settings::sanitize_architecture_file_name(&file_name);

    refresh_architecture_projection(&settings, &previous_names)?;
    store_settings(&state, settings)
}

/// Opts one project in or out of generated in-tree projection, with an optional per-project
/// file-name override (`None` clears it and falls back to the global default).
#[tauri::command]
fn set_project_architecture_projection(
    project_id: String,
    enabled: bool,
    file_name: Option<String>,
    state: State<'_, AppState>,
) -> Result<AppSettings, String> {
    let mut settings = state
        .settings
        .lock()
        .map_err(|_| "Settings lock was poisoned".to_string())?
        .clone();

    if !settings.projects.iter().any(|p| p.id == project_id) {
        return Err(format!("Unknown project id: {project_id}"));
    }

    let previous_names = resolved_architecture_names(&settings);

    settings
        .architecture_projection
        .enabled_project_ids
        .retain(|id| id != &project_id);
    if enabled {
        settings
            .architecture_projection
            .enabled_project_ids
            .push(project_id.clone());
    }

    match file_name.as_deref().map(str::trim) {
        Some(name) if !name.is_empty() => {
            settings
                .architecture_projection
                .project_file_names
                .insert(project_id.clone(), name.to_string());
        }
        _ => {
            settings
                .architecture_projection
                .project_file_names
                .remove(&project_id);
        }
    }

    refresh_architecture_projection(&settings, &previous_names)?;
    store_settings(&state, settings)
}

#[cfg(test)]
fn load_project_row_id(db: &Connection) -> Result<i64, String> {
    db.query_row("SELECT id FROM projects ORDER BY id LIMIT 1", [], |row| {
        row.get(0)
    })
    .map_err(|err| err.to_string())
}

#[cfg(test)]
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















#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_project_without_project_input_returns_explicit_error() {
        let settings = AppSettings {
            window: WindowSettings::default(),
            projects: Vec::new(),
            last_active_project_id: None,
            rule_templates: Vec::new(), architecture_projection: Default::default(),
        };

        let error = resolve_project_from_settings(&settings, None).unwrap_err();

        assert_eq!(
            error,
            "projectName is required and must be a configured project id or project name"
        );
    }

    #[test]
    fn resolve_project_with_blank_project_input_returns_explicit_error() {
        let settings = AppSettings {
            window: WindowSettings::default(),
            projects: vec![ProjectSettings {
                id: "adashi".to_string(),
                name: "Adashi".to_string(),
                folder: "C:\\src\\Adashi".to_string(),
            }],
            last_active_project_id: Some("adashi".to_string()),
            rule_templates: Vec::new(), architecture_projection: Default::default(),
        };

        let error = resolve_project_from_settings(&settings, Some("   ")).unwrap_err();

        assert_eq!(
            error,
            "projectName is required and must be a configured project id or project name"
        );
    }

    #[test]
    fn resolve_project_with_unknown_project_input_returns_clean_error() {
        let settings = AppSettings {
            window: WindowSettings::default(),
            projects: vec![ProjectSettings {
                id: "adashi".to_string(),
                name: "Adashi".to_string(),
                folder: "C:\\src\\Adashi".to_string(),
            }],
            last_active_project_id: Some("adashi".to_string()),
            rule_templates: Vec::new(), architecture_projection: Default::default(),
        };

        let error = resolve_project_from_settings(&settings, Some("raysplatter")).unwrap_err();

        assert_eq!(error, "Unknown project id or name: raysplatter");
    }

    #[test]
    fn resolve_project_matches_id_or_case_insensitive_name() {
        let settings = AppSettings {
            window: WindowSettings::default(),
            projects: vec![
                ProjectSettings {
                    id: "adashi".to_string(),
                    name: "Adashi".to_string(),
                    folder: "C:\\src\\Adashi".to_string(),
                },
                ProjectSettings {
                    id: "raysplatter-12345".to_string(),
                    name: "RaySplatter".to_string(),
                    folder: "C:\\Unreal\\RaySplatter".to_string(),
                },
            ],
            last_active_project_id: Some("adashi".to_string()),
            rule_templates: Vec::new(), architecture_projection: Default::default(),
        };

        let by_id = resolve_project_from_settings(&settings, Some("raysplatter-12345")).unwrap();
        let by_name = resolve_project_from_settings(&settings, Some("raysplatter")).unwrap();

        assert_eq!(by_id.name, "RaySplatter");
        assert_eq!(by_name.id, "raysplatter-12345");
    }

    #[test]
    fn add_project_rejects_duplicate_names_case_insensitively() {
        let folder = temp_project_folder("adashi-duplicate-name");
        fs::create_dir_all(&folder).unwrap();
        let second_folder = temp_project_folder("adashi-duplicate-name-second");
        fs::create_dir_all(&second_folder).unwrap();
        let settings_path = folder.join("settings.json");
        let state = AppState {
            settings_path,
            settings: Arc::new(Mutex::new(AppSettings {
                window: WindowSettings::default(),
                projects: Vec::new(),
                last_active_project_id: None,
                rule_templates: Vec::new(), architecture_projection: Default::default(),
            })),
        };

        let settings = add_project_to_settings(
            "RaySplatter".to_string(),
            folder.to_string_lossy().to_string(),
            &state,
        )
        .unwrap();
        assert_eq!(settings.projects[0].name, "RaySplatter");

        let error = add_project_to_settings(
            "  raysplatter ".to_string(),
            second_folder.to_string_lossy().to_string(),
            &state,
        )
        .expect_err("a case-insensitive duplicate name must be rejected");
        assert!(error.contains("already registered"), "{error}");

        let settings = state.settings.lock().unwrap().clone();
        assert_eq!(settings.projects.len(), 1);
        assert_eq!(
            settings.projects[0].name, "RaySplatter",
            "the stored name keeps its original spelling"
        );

        let _ = fs::remove_dir_all(folder);
        let _ = fs::remove_dir_all(second_folder);
    }

    #[test]
    fn add_project_to_empty_settings_initializes_database_and_selects_project() {
        let folder = temp_project_folder("adashi-first-project");
        fs::create_dir_all(&folder).unwrap();
        let settings_path = folder.join("settings.json");
        let state = AppState {
            settings_path,
            settings: Arc::new(Mutex::new(AppSettings {
                window: WindowSettings::default(),
                projects: Vec::new(),
                last_active_project_id: None,
                rule_templates: Vec::new(), architecture_projection: Default::default(),
            })),
        };

        let settings = add_project_to_settings(
            "RaySplatter".to_string(),
            folder.to_string_lossy().to_string(),
            &state,
        )
        .unwrap();

        assert_eq!(settings.projects.len(), 1);
        assert_eq!(
            settings.last_active_project_id.as_deref(),
            Some(settings.projects[0].id.as_str())
        );
        assert!(settings::project_database_path(&settings.projects[0]).exists());

        let db = open_project_database(&settings.projects[0]).unwrap();
        verify_project_database(crate::storage::sqlite::test_snapshot(&db, load_project_row_id(&db).unwrap()).as_ref()).unwrap();
        drop(db);
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn desktop_registration_rejects_an_unavailable_backend_without_creating_sqlite() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("project");
        fs::create_dir_all(folder.join(".adashi")).unwrap();
        fs::write(folder.join(".adashi/storage.json"),
            r#"{"schemaVersion":1,"backend":{"kind":"serverSql","connectionProfile":"team","namespace":"test"}}"#).unwrap();
        let state = AppState {
            settings_path: root.path().join("settings.json"),
            settings: Arc::new(Mutex::new(AppSettings {
                window: WindowSettings::default(), projects: vec![], last_active_project_id: None,
                rule_templates: vec![], architecture_projection: Default::default(),
            })),
        };
        let error = add_project_to_settings("Text project".into(), folder.to_string_lossy().into_owned(), &state).unwrap_err();
        assert!(error.contains("storage.backend_unavailable"), "{error}");
        assert!(!folder.join(".adashi/adashi.sqlite3").exists());
        assert!(!state.settings_path.exists());
        assert!(state.settings.lock().unwrap().projects.is_empty());
    }

    /// The dashboard shows a checkbox per task state, so its payload has to carry every state.
    /// When the payload filtered states instead, the closed checkbox could be ticked but nothing
    /// could ever appear under it.
    #[test]
    fn dashboard_payload_carries_closed_tasks_for_the_state_checkboxes() {
        let folder = temp_project_folder("adashi-dashboard-task-states");
        fs::create_dir_all(&folder).unwrap();
        let state = AppState {
            settings_path: folder.join("settings.json"),
            settings: Arc::new(Mutex::new(AppSettings {
                window: WindowSettings::default(),
                projects: Vec::new(),
                last_active_project_id: None,
                rule_templates: Vec::new(),
                architecture_projection: Default::default(),
            })),
        };
        let settings = add_project_to_settings(
            "RaySplatter".to_string(),
            folder.to_string_lossy().to_string(),
            &state,
        )
        .unwrap();
        let project = settings.projects[0].clone();
        let db = open_project_database(&project).unwrap();
        let project_row_id: i64 = db
            .query_row("SELECT id FROM projects LIMIT 1", [], |row| row.get(0))
            .unwrap();
        for (number, name) in [(1_i64, "todo"), (2, "active"), (3, "finished"), (4, "closed")] {
            db.execute(
                "INSERT INTO agent_tasks(project_id, number, title, state) VALUES(?1, ?2, ?3, ?4)",
                rusqlite::params![project_row_id, number, format!("Task {name}"), name],
            )
            .unwrap();
        }

        let mut store = open_project_store(&project).unwrap();
        let payload = load_dashboard_payload(project, &mut store, &state).unwrap();
        drop(store);
        let states = payload
            .tasks
            .iter()
            .map(|task| task.state.as_str())
            .collect::<Vec<_>>();
        for expected in tasks::ALL_TASK_STATES {
            assert!(
                states.contains(&expected.as_str()),
                "the dashboard payload must carry {} tasks, found {states:?}",
                expected.as_str()
            );
        }

        drop(db);
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn open_project_database_creates_seeded_project_store() {
        let folder = temp_project_folder("adashi-project-init");
        fs::create_dir_all(&folder).unwrap();
        let project = settings::new_project(
            "RaySplatter".to_string(),
            folder.to_string_lossy().to_string(),
        );

        let db = open_project_database(&project).unwrap();

        verify_project_database(crate::storage::sqlite::test_snapshot(&db, load_project_row_id(&db).unwrap()).as_ref()).unwrap();
        assert!(settings::project_database_path(&project).exists());
        assert_eq!(
            db.query_row("SELECT repository_path FROM project_computers WHERE computer_id = ?1", [crate::computer::id().unwrap()], |row| {
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
    fn opening_and_refreshing_dashboard_does_not_write_the_database() {
        let folder = temp_project_folder("adashi-dashboard-read-only");
        let project = ProjectSettings {
            id: "adashi".into(),
            name: "Adashi".into(),
            folder: folder.to_string_lossy().into_owned(),
        };
        let state = AppState {
            settings_path: folder.join("settings.json"),
            settings: Arc::new(Mutex::new(AppSettings {
                window: WindowSettings::default(),
                projects: vec![project.clone()],
                last_active_project_id: Some(project.id.clone()),
                rule_templates: vec![],
                architecture_projection: Default::default(),
            })),
        };
        drop(open_project_database(&project).unwrap());
        let path = settings::project_database_path(&project);
        let before = fs::read(&path).unwrap();
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        for _ in 0..3 {
            let db = open_project_database(&project).unwrap();
            let mut store = open_project_store(&project).unwrap();
            let payload = load_dashboard_payload(project.clone(), &mut store, &state).unwrap();
            drop(store);
            assert!(!payload.design_elements.is_empty());
            assert_eq!(db.total_changes(), 0);
            drop(db);
            assert_eq!(fs::read(&path).unwrap(), before);
            assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
        }
        fs::remove_dir_all(folder).unwrap();
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
