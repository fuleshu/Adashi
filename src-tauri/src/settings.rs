use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub window: WindowSettings,
    pub projects: Vec<ProjectSettings>,
    pub last_active_project_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowSettings {
    pub width: u32,
    pub height: u32,
    pub x: Option<i32>,
    pub y: Option<i32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSettings {
    pub id: String,
    pub name: String,
    pub folder: String,
}

impl AppSettings {
    pub fn active_project(&self) -> Option<&ProjectSettings> {
        self.last_active_project_id
            .as_deref()
            .and_then(|id| self.projects.iter().find(|project| project.id == id))
            .or_else(|| self.projects.first())
    }
}

impl Default for WindowSettings {
    fn default() -> Self {
        Self {
            width: 1440,
            height: 940,
            x: None,
            y: None,
        }
    }
}

pub fn settings_path() -> PathBuf {
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(local_app_data).join("Adashi").join("settings.json");
    }

    PathBuf::from(".").join("Adashi").join("settings.json")
}

pub fn load_or_init(path: &Path) -> Result<AppSettings, Box<dyn std::error::Error>> {
    if path.exists() {
        let text = fs::read_to_string(path)?;
        match serde_json::from_str::<AppSettings>(text.trim_start_matches('\u{feff}')) {
            Ok(settings) => return Ok(normalize(settings)),
            Err(_) => {
                let backup_path = path.with_extension("json.invalid");
                let _ = fs::copy(path, backup_path);
                let settings = default_settings();
                save(path, &settings)?;
                return Ok(settings);
            }
        }
    }

    let settings = default_settings();
    save(path, &settings)?;
    Ok(settings)
}

pub fn save(path: &Path, settings: &AppSettings) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let text = serde_json::to_string_pretty(settings)?;
    fs::write(path, format!("{text}\n"))?;
    Ok(())
}

pub fn project_data_dir(project: &ProjectSettings) -> PathBuf {
    PathBuf::from(&project.folder).join(".adashi")
}

pub fn project_database_path(project: &ProjectSettings) -> PathBuf {
    project_data_dir(project).join("adashi.sqlite3")
}

pub fn new_project(name: String, folder: String) -> ProjectSettings {
    let id = make_project_id(&name);
    ProjectSettings { id, name, folder }
}

fn normalize(mut settings: AppSettings) -> AppSettings {
    if settings.window.width < 640 {
        settings.window.width = 1440;
    }

    if settings.window.height < 480 {
        settings.window.height = 940;
    }

    if settings.projects.is_empty() {
        settings.projects.push(default_project());
    }

    let last_id_is_valid = settings
        .last_active_project_id
        .as_ref()
        .map(|id| settings.projects.iter().any(|project| &project.id == id))
        .unwrap_or(false);

    if !last_id_is_valid {
        settings.last_active_project_id = settings.projects.first().map(|project| project.id.clone());
    }

    settings
}

fn default_settings() -> AppSettings {
    let project = default_project();
    AppSettings {
        window: WindowSettings::default(),
        last_active_project_id: Some(project.id.clone()),
        projects: vec![project],
    }
}

fn default_project() -> ProjectSettings {
    ProjectSettings {
        id: "adashi".to_string(),
        name: "Adashi".to_string(),
        folder: default_project_folder(),
    }
}

fn default_project_folder() -> String {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or(manifest_dir)
        .to_string_lossy()
        .to_string()
}

fn make_project_id(name: &str) -> String {
    let slug = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");

    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();

    if slug.is_empty() {
        format!("project-{millis}")
    } else {
        format!("{slug}-{millis}")
    }
}
