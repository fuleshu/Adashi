mod schema;
mod seed;
mod settings;

use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{Manager, PhysicalPosition, PhysicalSize, State, WebviewWindow, WindowEvent};

use crate::settings::{AppSettings, ProjectSettings, WindowSettings};

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
    workspace_name: String,
    workspace_description: String,
    structurizr_workspace: String,
    structurizr_view_key: String,
    diagrams: Vec<DesignDiagram>,
    tasks: Vec<Task>,
    guidelines: Vec<Guideline>,
    post_task_commands: Vec<PostTaskCommand>,
    qa_checks: Vec<QaCheck>,
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
struct Task {
    id: i64,
    title: String,
    status: String,
    priority: i64,
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
fn get_dashboard(project_id: Option<String>, state: State<'_, AppState>) -> Result<DashboardPayload, String> {
    let project = resolve_project(&state, project_id.as_deref())?;
    let db = open_project_database(&project).map_err(|err| err.to_string())?;

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

    Ok(DashboardPayload {
        project_id: project.id,
        project_name: project.name,
        project_folder: project.folder,
        workspace_name: workspace.0,
        workspace_description: workspace.1,
        structurizr_workspace: workspace.2,
        structurizr_view_key: "AdashiContainers".to_string(),
        diagrams: load_diagrams(&db)?,
        tasks: load_tasks(&db)?,
        guidelines: load_guidelines(&db)?,
        post_task_commands: load_post_task_commands(&db)?,
        qa_checks: load_qa_checks(&db)?,
    })
}

#[tauri::command]
fn set_active_project(project_id: String, state: State<'_, AppState>) -> Result<AppSettings, String> {
    let mut settings = state
        .settings
        .lock()
        .map_err(|_| "Settings lock was poisoned".to_string())?;

    if !settings.projects.iter().any(|project| project.id == project_id) {
        return Err(format!("Unknown project id: {project_id}"));
    }

    settings.last_active_project_id = Some(project_id);
    settings::save(&state.settings_path, &settings).map_err(|err| err.to_string())?;
    Ok(settings.clone())
}

#[tauri::command]
fn add_project(name: String, folder: String, state: State<'_, AppState>) -> Result<AppSettings, String> {
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
        settings.last_active_project_id = settings.projects.first().map(|project| project.id.clone());
    }

    settings::save(&state.settings_path, &settings).map_err(|err| err.to_string())?;
    Ok(settings.clone())
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
            delete_project,
            get_app_settings,
            get_dashboard,
            set_active_project
        ])
        .run(tauri::generate_context!())
        .expect("error while running Adashi");
}

fn resolve_project(state: &State<'_, AppState>, project_id: Option<&str>) -> Result<ProjectSettings, String> {
    let settings = state
        .settings
        .lock()
        .map_err(|_| "Settings lock was poisoned".to_string())?;

    let project = if let Some(project_id) = project_id {
        settings.projects.iter().find(|project| project.id == project_id)
    } else {
        settings.active_project()
    };

    project
        .cloned()
        .ok_or_else(|| "No active project is configured".to_string())
}

fn open_project_database(project: &ProjectSettings) -> Result<Connection, Box<dyn std::error::Error>> {
    let data_dir = settings::project_data_dir(project);
    fs::create_dir_all(&data_dir)?;

    let mut db = Connection::open(settings::project_database_path(project))?;
    schema::migrate(&mut db)?;
    seed::seed_initial_data(&mut db, project)?;
    Ok(db)
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

    window.set_size(PhysicalSize::new(window_settings.width, window_settings.height))?;

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

fn load_tasks(db: &Connection) -> Result<Vec<Task>, String> {
    let mut statement = db
        .prepare("SELECT id, title, status, priority FROM agent_tasks ORDER BY priority, id")
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(Task {
                id: row.get(0)?,
                title: row.get(1)?,
                status: row.get(2)?,
                priority: row.get(3)?,
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
