mod mcp;
mod memory;
mod rules;
mod schema;
mod seed;
mod settings;
mod state;
mod tasks;

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{Manager, PhysicalPosition, PhysicalSize, State, WebviewWindow, WindowEvent};

use crate::memory::ProjectMemory;
use crate::rules::{NewRule, Rule, UpdateRule};
use crate::settings::{AppSettings, ProjectSettings, WindowSettings};
use crate::state as project_state;
use crate::tasks::Task;

struct AppState {
    settings_path: PathBuf,
    settings: Arc<Mutex<AppSettings>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DashboardPayload {
    project_id: String,
    project_name: String,
    project_folder: String,
    revision: i64,
    workspace_name: String,
    workspace_description: String,
    structurizr_workspace: String,
    structurizr_view_key: String,
    diagrams: Vec<DesignDiagram>,
    tasks: Vec<Task>,
    guidelines: Vec<Guideline>,
    post_task_commands: Vec<PostTaskCommand>,
    qa_checks: Vec<QaCheck>,
    rules: Vec<Rule>,
    memory: ProjectMemory,
}

#[derive(Serialize)]
struct DesignDiagram {
    id: i64,
    kind: String,
    key: String,
    title: String,
    source: String,
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
    load_dashboard_payload(project, &db)
}

fn load_dashboard_payload(
    project: ProjectSettings,
    db: &Connection,
) -> Result<DashboardPayload, String> {
    let workspace = db
        .query_row(
            "SELECT w.name, w.description, w.structurizr_json
             FROM design_workspaces w
             ORDER BY w.id
             LIMIT 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "No design workspace has been seeded".to_string())?;

    let project_row_id = load_project_row_id(db)?;
    let revision = project_state::load_project_revision(db, project_row_id)?.revision;

    Ok(DashboardPayload {
        project_id: project.id,
        project_name: project.name,
        project_folder: project.folder,
        revision,
        workspace_name: workspace.0,
        workspace_description: workspace.1,
        structurizr_workspace: workspace.2,
        structurizr_view_key: "AdashiContainers".to_string(),
        diagrams: load_diagrams(&db)?,
        tasks: tasks::load_tasks(db)?,
        guidelines: load_guidelines(&db)?,
        post_task_commands: load_post_task_commands(&db)?,
        qa_checks: load_qa_checks(&db)?,
        rules: rules::load_rules(db)?,
        memory: memory::load_memory(db, project_row_id)?,
    })
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

    let mut project = settings::new_project(trimmed_name.to_string(), trimmed_folder.to_string());
    let canonical_folder = PathBuf::from(&project.folder)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(&project.folder));
    project.folder = canonical_folder.to_string_lossy().to_string();

    open_project_database(&project).map_err(|err| err.to_string())?;

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

    load_dashboard_payload(project, &db)
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

    load_dashboard_payload(project, &db)
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
    load_dashboard_payload(project, &db)
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
    load_dashboard_payload(project, &db)
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
    load_dashboard_payload(project, &db)
}

pub fn run_mcp() -> Result<(), Box<dyn std::error::Error>> {
    mcp::run_stdio_server()
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
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
            create_rule,
            delete_rule,
            delete_project,
            get_app_settings,
            get_dashboard,
            get_project_revision,
            set_active_project,
            update_memory,
            update_memory_rule,
            update_rule
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
    Ok(db)
}

fn load_project_row_id(db: &Connection) -> Result<i64, String> {
    db.query_row("SELECT id FROM projects ORDER BY id LIMIT 1", [], |row| {
        row.get(0)
    })
    .map_err(|err| err.to_string())
}

fn restore_window(
    window: &WebviewWindow,
    settings: &Arc<Mutex<AppSettings>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let window_settings = settings
        .lock()
        .map_err(|_| "Settings lock was poisoned")?
        .window
        .clone();

    window.set_size(PhysicalSize::new(
        window_settings.width,
        window_settings.height,
    ))?;

    if let (Some(x), Some(y)) = (window_settings.x, window_settings.y) {
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
    let size = window.outer_size()?;
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

fn load_diagrams(db: &Connection) -> Result<Vec<DesignDiagram>, String> {
    let mut statement = db
        .prepare("SELECT id, kind, key, title, source FROM diagrams ORDER BY sort_order, id")
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(DesignDiagram {
                id: row.get(0)?,
                kind: row.get(1)?,
                key: row.get(2)?,
                title: row.get(3)?,
                source: row.get(4)?,
            })
        })
        .map_err(|err| err.to_string())?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| err.to_string())
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
