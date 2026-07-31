use std::fs;

use rusqlite::Connection;

use crate::fixed_hooks;
use crate::schema;
use crate::seed;
use crate::settings::{self, AppSettings, ProjectSettings};

pub(crate) fn resolve_project_from_settings(
    settings: &AppSettings,
    project_ref: Option<&str>,
) -> Result<ProjectSettings, String> {
    let project_ref = project_ref
        .map(str::trim)
        .filter(|project_ref| !project_ref.is_empty())
        .ok_or_else(|| {
            "projectId is required and must be a configured project id or project name".to_string()
        })?;

    settings
        .projects
        .iter()
        .find(|project| project.id == project_ref || project.name.eq_ignore_ascii_case(project_ref))
        .cloned()
        .ok_or_else(|| format!("Unknown project id or name: {project_ref}"))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{RuleTemplate, WindowSettings};

    fn settings_with_projects() -> AppSettings {
        AppSettings {
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
            rule_templates: Vec::<RuleTemplate>::new(),
        }
    }

    #[test]
    fn project_reference_is_required() {
        let error = resolve_project_from_settings(&settings_with_projects(), Some("   "))
            .expect_err("blank references must fail");
        assert_eq!(
            error,
            "projectId is required and must be a configured project id or project name"
        );
    }

    #[test]
    fn project_reference_matches_id_or_case_insensitive_name() {
        let settings = settings_with_projects();
        assert_eq!(
            resolve_project_from_settings(&settings, Some("raysplatter-12345"))
                .unwrap()
                .name,
            "RaySplatter"
        );
        assert_eq!(
            resolve_project_from_settings(&settings, Some("raysplatter"))
                .unwrap()
                .id,
            "raysplatter-12345"
        );
    }
}
