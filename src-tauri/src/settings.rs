use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Instruction-file name used for generated architecture blocks when nothing else is set.
pub const DEFAULT_ARCHITECTURE_FILE_NAME: &str = "AGENTS.md";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub window: WindowSettings,
    pub projects: Vec<ProjectSettings>,
    pub last_active_project_id: Option<String>,
    #[serde(default)]
    pub rule_templates: Vec<RuleTemplate>,
    #[serde(default)]
    pub architecture_projection: ArchitectureProjectionSettings,
}

/// In-tree architecture projection configuration.
///
/// The design database stays canonical; these settings control the generated instruction-file
/// blocks that make the model visible in the folders an agent already reads.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchitectureProjectionSettings {
    /// Instruction-file name for generated blocks, for example `AGENTS.md`.
    pub file_name: String,
    /// Project ids that opted in to in-tree projection. Empty means no project writes files.
    #[serde(default)]
    pub enabled_project_ids: Vec<String>,
    /// Per-project file-name overrides, keyed by project id.
    #[serde(default)]
    pub project_file_names: BTreeMap<String, String>,
}

impl Default for ArchitectureProjectionSettings {
    fn default() -> Self {
        Self {
            file_name: DEFAULT_ARCHITECTURE_FILE_NAME.to_string(),
            enabled_project_ids: Vec::new(),
            project_file_names: BTreeMap::new(),
        }
    }
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

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleTemplate {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub intend: String,
    pub hook: String,
    pub prompt: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug)]
pub struct RuleTemplateDraft {
    pub name: String,
    pub enabled: bool,
    pub intend: String,
    pub hook: String,
    pub prompt: String,
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
    #[cfg(windows)]
    {
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(local_app_data)
                .join("Adashi")
                .join("settings.json");
        }

        PathBuf::from(".").join("Adashi").join("settings.json")
    }

    #[cfg(not(windows))]
    {
        let xdg_config_home = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
        let home = std::env::var_os("HOME").map(PathBuf::from);
        linux_settings_path(xdg_config_home.as_deref(), home.as_deref())
    }
}

#[cfg(not(windows))]
fn linux_settings_path(xdg_config_home: Option<&Path>, home: Option<&Path>) -> PathBuf {
    if let Some(config_home) = xdg_config_home.filter(|path| !path.as_os_str().is_empty()) {
        return config_home.join("adashi").join("settings.json");
    }

    if let Some(home) = home.filter(|path| !path.as_os_str().is_empty()) {
        return home.join(".config").join("adashi").join("settings.json");
    }

    PathBuf::from(".").join("Adashi").join("settings.json")
}

pub fn load_or_init(path: &Path) -> Result<AppSettings, Box<dyn std::error::Error>> {
    if path.exists() {
        let text = fs::read_to_string(path)?;
        match serde_json::from_str::<AppSettings>(text.trim_start_matches('\u{feff}')) {
            Ok(settings) => {
                let settings = normalize(settings);
                save(path, &settings)?;
                return Ok(settings);
            }
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

    let settings = normalize(settings.clone());
    let text = serde_json::to_string_pretty(&settings)?;
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
    ProjectSettings {
        id,
        name,
        folder: normalize_project_folder_text(&folder),
    }
}

pub fn normalize_project_folder(folder: &str) -> String {
    let folder = PathBuf::from(folder)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(folder));
    normalize_project_folder_path(&folder)
}

pub fn normalize_project_folder_path(path: &Path) -> String {
    normalize_project_folder_text(&path.to_string_lossy())
}

pub fn normalize_project_folder_text(folder: &str) -> String {
    let folder = folder.trim();

    #[cfg(windows)]
    {
        normalize_windows_project_folder(folder)
    }

    #[cfg(not(windows))]
    {
        folder.to_string()
    }
}

pub fn save_rule_template(
    settings: &mut AppSettings,
    draft: RuleTemplateDraft,
) -> Result<RuleTemplate, String> {
    let name = draft.name.trim();
    if name.is_empty() {
        return Err("Rule template name is required".to_string());
    }

    validate_rule_template_intend(&draft.intend)?;
    validate_rule_template_hook(&draft.hook)?;

    let now = timestamp_millis();
    let template = RuleTemplate {
        id: make_rule_template_id(name),
        name: name.to_string(),
        enabled: draft.enabled,
        intend: draft.intend,
        hook: draft.hook,
        prompt: draft.prompt,
        created_at: now.clone(),
        updated_at: now,
    };

    settings.rule_templates.push(template.clone());
    Ok(template)
}

pub fn delete_rule_template(settings: &mut AppSettings, template_id: &str) -> Result<(), String> {
    let initial_len = settings.rule_templates.len();
    settings
        .rule_templates
        .retain(|template| template.id != template_id);

    if settings.rule_templates.len() == initial_len {
        Err(format!("Unknown rule template id: {template_id}"))
    } else {
        Ok(())
    }
}

/// Validated instruction-file name: the given name when it is a plain file name inside its
/// folder, otherwise the default. Path separators are rejected so a name cannot escape.
pub fn sanitize_architecture_file_name(name: &str) -> String {
    let name = name.trim();
    if valid_instruction_file_name(name) {
        name.to_string()
    } else {
        DEFAULT_ARCHITECTURE_FILE_NAME.to_string()
    }
}

/// Resolves the instruction-file name for one project: the per-project override when valid,
/// otherwise the global default. The result is always a plain file name inside its folder.
pub fn architecture_file_name(settings: &AppSettings, project_id: &str) -> String {
    if let Some(name) = settings
        .architecture_projection
        .project_file_names
        .get(project_id)
    {
        if valid_instruction_file_name(name) {
            return name.trim().to_string();
        }
    }

    sanitize_architecture_file_name(&settings.architecture_projection.file_name)
}

/// Whether a project opted in to generated in-tree architecture projection.
pub fn architecture_projection_enabled(settings: &AppSettings, project_id: &str) -> bool {
    settings
        .architecture_projection
        .enabled_project_ids
        .iter()
        .any(|id| id == project_id)
}

/// A file name is accepted only when it stays inside the folder it is written to.
fn valid_instruction_file_name(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains(':')
}

fn normalize_architecture_projection(settings: &mut AppSettings) {
    let known_projects = settings
        .projects
        .iter()
        .map(|project| project.id.clone())
        .collect::<Vec<_>>();
    let projection = &mut settings.architecture_projection;

    projection.file_name = projection.file_name.trim().to_string();
    if !valid_instruction_file_name(&projection.file_name) {
        projection.file_name = DEFAULT_ARCHITECTURE_FILE_NAME.to_string();
    }

    projection
        .enabled_project_ids
        .retain(|id| known_projects.contains(id));
    projection.enabled_project_ids.sort();
    projection.enabled_project_ids.dedup();

    projection
        .project_file_names
        .retain(|id, name| known_projects.contains(id) && valid_instruction_file_name(name));
}

fn normalize(mut settings: AppSettings) -> AppSettings {
    if settings.window.width < 640 {
        settings.window.width = 1440;
    }

    if settings.window.height < 480 {
        settings.window.height = 940;
    }

    for project in &mut settings.projects {
        project.name = project.name.trim().to_string();
        project.folder = normalize_project_folder_text(&project.folder);
    }

    let last_id_is_valid = settings
        .last_active_project_id
        .as_ref()
        .map(|id| settings.projects.iter().any(|project| &project.id == id))
        .unwrap_or(false);

    if !last_id_is_valid {
        settings.last_active_project_id = None;
    }

    normalize_rule_templates(&mut settings.rule_templates);
    normalize_architecture_projection(&mut settings);
    settings
}

fn default_settings() -> AppSettings {
    AppSettings {
        window: WindowSettings::default(),
        last_active_project_id: None,
        projects: Vec::new(),
        rule_templates: Vec::new(),
        architecture_projection: ArchitectureProjectionSettings::default(),
    }
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

fn normalize_rule_templates(templates: &mut [RuleTemplate]) {
    for template in templates {
        template.name = template.name.trim().to_string();

        if template.id.trim().is_empty() {
            template.id = make_rule_template_id(&template.name);
        }

        if template.created_at.trim().is_empty() {
            template.created_at = timestamp_millis();
        }

        if template.updated_at.trim().is_empty() {
            template.updated_at = template.created_at.clone();
        }
    }
}

#[cfg(windows)]
fn normalize_windows_project_folder(folder: &str) -> String {
    let mut folder = folder.replace('/', "\\");

    if let Some(rest) = folder.strip_prefix(r"\\?\UNC\") {
        folder = format!(r"\\{rest}");
    } else if let Some(rest) = folder.strip_prefix(r"\\?\") {
        folder = rest.to_string();
    }

    collapse_windows_separators(&folder)
}

#[cfg(windows)]
fn collapse_windows_separators(folder: &str) -> String {
    let preserves_unc_prefix = folder.starts_with(r"\\");
    let mut normalized = String::with_capacity(folder.len());

    for character in folder.chars() {
        if character == '\\' {
            let can_preserve_unc_prefix = preserves_unc_prefix && normalized == r"\";
            if !normalized.ends_with('\\') || can_preserve_unc_prefix {
                normalized.push(character);
            }
        } else {
            normalized.push(character);
        }
    }

    normalized
}

fn make_rule_template_id(name: &str) -> String {
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
    let millis = timestamp_millis();

    if slug.is_empty() {
        format!("rule-template-{millis}")
    } else {
        format!("rule-template-{slug}-{millis}")
    }
}

fn timestamp_millis() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
        .to_string()
}

fn validate_rule_template_intend(intend: &str) -> Result<(), String> {
    if matches!(intend, "general" | "design" | "implementation") {
        Ok(())
    } else {
        Err(format!("Invalid intend '{intend}'"))
    }
}

fn validate_rule_template_hook(hook: &str) -> Result<(), String> {
    if matches!(hook, "run.start" | "task.start" | "task.end" | "run.end") {
        Ok(())
    } else {
        Err(format!("Invalid hook '{hook}'"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn architecture_file_name_rejects_paths_and_honours_overrides() {
        let mut settings = AppSettings {
            window: WindowSettings::default(),
            projects: vec![ProjectSettings {
                id: "p".to_string(),
                name: "P".to_string(),
                folder: "C:\\p".to_string(),
            }],
            last_active_project_id: Some("p".to_string()),
            rule_templates: Vec::new(),
            architecture_projection: ArchitectureProjectionSettings::default(),
        };

        assert_eq!(architecture_file_name(&settings, "p"), DEFAULT_ARCHITECTURE_FILE_NAME);
        assert!(!architecture_projection_enabled(&settings, "p"));

        settings.architecture_projection.file_name = "CLAUDE.md".to_string();
        assert_eq!(architecture_file_name(&settings, "p"), "CLAUDE.md");

        // A name that could escape its folder falls back instead of being written.
        for bad in [
            "../../etc/passwd",
            "..\\escape.md",
            "sub/dir.md",
            "C:\\absolute.md",
            "",
            "   ",
            "..",
            ".",
        ] {
            settings.architecture_projection.file_name = bad.to_string();
            assert_eq!(
                architecture_file_name(&settings, "p"),
                DEFAULT_ARCHITECTURE_FILE_NAME,
                "{bad:?} must fall back to the default"
            );
        }

        settings.architecture_projection.file_name = DEFAULT_ARCHITECTURE_FILE_NAME.to_string();
        settings
            .architecture_projection
            .project_file_names
            .insert("p".to_string(), "PROJECT.md".to_string());
        assert_eq!(architecture_file_name(&settings, "p"), "PROJECT.md");

        settings
            .architecture_projection
            .enabled_project_ids
            .push("p".to_string());
        assert!(architecture_projection_enabled(&settings, "p"));
    }

    #[test]
    fn normalize_drops_projection_configuration_for_unknown_projects() {
        let path = test_settings_path("projection-normalize");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"{
  "window": { "width": 1024, "height": 768, "x": null, "y": null },
  "projects": [{ "id": "adashi", "name": "Adashi", "folder": "C:\\src\\Adashi" }],
  "lastActiveProjectId": "adashi",
  "ruleTemplates": [],
  "architectureProjection": {
    "fileName": "   ",
    "enabledProjectIds": ["adashi", "gone", "adashi"],
    "projectFileNames": { "adashi": "PROJECT.md", "gone": "X.md", "adashi2": "a/b.md" }
  }
}"#,
        )
        .unwrap();

        let settings = load_or_init(&path).unwrap();
        assert_eq!(
            settings.architecture_projection.file_name,
            DEFAULT_ARCHITECTURE_FILE_NAME
        );
        assert_eq!(
            settings.architecture_projection.enabled_project_ids,
            vec!["adashi".to_string()]
        );
        assert_eq!(
            settings.architecture_projection.project_file_names.len(),
            1,
            "unknown and invalid overrides are dropped"
        );
        assert_eq!(architecture_file_name(&settings, "adashi"), "PROJECT.md");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn legacy_settings_load_without_rule_templates_normalizes_to_empty_list() {
        let path = test_settings_path("legacy-settings");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"{
  "window": { "width": 1024, "height": 768, "x": null, "y": null },
  "projects": [{ "id": "adashi", "name": "Adashi", "folder": "C:\\src\\Adashi" }],
  "lastActiveProjectId": "adashi"
}
"#,
        )
        .unwrap();

        let settings = load_or_init(&path).unwrap();

        assert!(settings.rule_templates.is_empty());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn missing_settings_loads_empty_project_list_for_onboarding() {
        let path = test_settings_path("missing-settings");
        let _ = fs::remove_file(&path);

        let settings = load_or_init(&path).unwrap();

        assert!(settings.projects.is_empty());
        assert_eq!(settings.last_active_project_id, None);
        assert!(path.exists());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn empty_project_list_stays_empty_for_onboarding() {
        let path = test_settings_path("empty-project-list");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"{
  "window": { "width": 1024, "height": 768, "x": null, "y": null },
  "projects": [],
  "lastActiveProjectId": null,
  "ruleTemplates": []
}
"#,
        )
        .unwrap();

        let settings = load_or_init(&path).unwrap();

        assert!(settings.projects.is_empty());
        assert_eq!(settings.last_active_project_id, None);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn invalid_active_project_id_is_cleared_for_onboarding() {
        let path = test_settings_path("invalid-active-project");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"{
  "window": { "width": 1024, "height": 768, "x": null, "y": null },
  "projects": [{ "id": "adashi", "name": "Adashi", "folder": "C:\\src\\Adashi" }],
  "lastActiveProjectId": "missing-project",
  "ruleTemplates": []
}
"#,
        )
        .unwrap();

        let settings = load_or_init(&path).unwrap();

        assert_eq!(settings.projects.len(), 1);
        assert_eq!(settings.last_active_project_id, None);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn save_and_delete_rule_template_updates_app_settings_only_model() {
        let mut settings = default_settings();
        let template = save_rule_template(
            &mut settings,
            RuleTemplateDraft {
                name: "  Implementation start  ".to_string(),
                enabled: true,
                intend: "implementation".to_string(),
                hook: "task.start".to_string(),
                prompt: "Use the design.".to_string(),
            },
        )
        .unwrap();

        assert_eq!(settings.rule_templates.len(), 1);
        assert_eq!(template.name, "Implementation start");
        assert_eq!(template.intend, "implementation");
        assert_eq!(template.hook, "task.start");
        assert!(!template.id.is_empty());
        assert!(!template.created_at.is_empty());
        assert_eq!(template.created_at, template.updated_at);

        delete_rule_template(&mut settings, &template.id).unwrap();
        assert!(settings.rule_templates.is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn normalizes_windows_verbatim_project_folder() {
        assert_eq!(
            normalize_project_folder_text(r"\\?\C:\Unreal\RaySplatter"),
            r"C:\Unreal\RaySplatter"
        );
    }

    #[cfg(windows)]
    #[test]
    fn normalizes_duplicate_windows_drive_separators() {
        assert_eq!(
            normalize_project_folder_text(r"C:\\src\\MyProject"),
            r"C:\src\MyProject"
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn linux_settings_path_prefers_xdg_config_home() {
        assert_eq!(
            linux_settings_path(
                Some(Path::new("/tmp/adashi-xdg")),
                Some(Path::new("/home/tester"))
            ),
            PathBuf::from("/tmp/adashi-xdg/adashi/settings.json")
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn linux_settings_path_falls_back_to_home_config() {
        assert_eq!(
            linux_settings_path(None, Some(Path::new("/home/tester"))),
            PathBuf::from("/home/tester/.config/adashi/settings.json")
        );
    }

    fn test_settings_path(label: &str) -> PathBuf {
        let millis = timestamp_millis();
        std::env::temp_dir()
            .join("adashi-settings-tests")
            .join(format!("{label}-{millis}.json"))
    }
}
