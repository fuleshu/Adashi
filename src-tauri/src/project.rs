#[cfg(test)]
use rusqlite::Connection;

use crate::settings::{AppSettings, ProjectSettings};
use crate::storage::{ProjectStore, StorageResult};

#[cfg(test)]
use {
    crate::{fixed_hooks, settings},
    std::fs,
};

pub(crate) fn resolve_project_from_settings(
    settings: &AppSettings,
    project_ref: Option<&str>,
) -> Result<ProjectSettings, String> {
    let project_ref = project_ref
        .map(str::trim)
        .filter(|project_ref| !project_ref.is_empty())
        .ok_or_else(|| {
            "projectName is required and must be a configured project id or project name"
                .to_string()
        })?;

    let matches = settings
        .projects
        .iter()
        .filter(|project| {
            project.id == project_ref || project.name.eq_ignore_ascii_case(project_ref)
        })
        .cloned()
        .collect::<Vec<_>>();

    match matches.len() {
        0 => Err(format!("Unknown project id or name: {project_ref}")),
        1 => Ok(matches.into_iter().next().expect("one match")),
        // Ambiguity must fail loudly: silently choosing the first match would make the same
        // reference resolve to different projects depending on settings order.
        _ => Err(format!(
            "Ambiguous project name '{project_ref}': {} configured projects share it",
            matches.len()
        )),
    }
}

#[cfg(test)]
pub(crate) fn open_project_database(
    project: &ProjectSettings,
) -> Result<Connection, Box<dyn std::error::Error>> {
    Ok(crate::storage::sqlite::open_test_database(
        project,
        crate::computer::id()?,
    )?)
}

/// Shared backend-neutral project entry point for desktop and MCP.
/// Raw database access exists only in test fixtures.
pub(crate) fn open_project_store(project: &ProjectSettings) -> StorageResult<ProjectStore> {
    ProjectStore::open(project)
}

#[cfg(test)]
fn open_project_database_for_computer(
    project: &ProjectSettings,
    computer_id: &str,
) -> Result<Connection, Box<dyn std::error::Error>> {
    Ok(crate::storage::sqlite::open_test_database(
        project,
        computer_id,
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{RuleTemplate, WindowSettings};

    fn fixture(label: &str) -> ProjectSettings {
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target/project-storage-tests")
            .join(format!(
                "{label}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        ProjectSettings {
            id: "storage-fixture".into(),
            name: "Storage fixture".into(),
            folder: folder.to_string_lossy().into_owned(),
        }
    }

    #[test]
    fn copied_database_keeps_each_computers_folder_and_shared_identity() {
        let first = fixture("first");
        drop(open_project_database_for_computer(&first, "windows:first").unwrap());
        let mut second = fixture("second");
        second.id = "different-local-registration".into();
        second.name = "Local alias".into();
        fs::create_dir_all(settings::project_data_dir(&second)).unwrap();
        fs::copy(
            settings::project_database_path(&first),
            settings::project_database_path(&second),
        )
        .unwrap();
        let db = open_project_database_for_computer(&second, "linux:second").unwrap();
        let folders: Vec<(String, String)> = db
            .prepare(
                "SELECT computer_id, repository_path FROM project_computers ORDER BY computer_id",
            )
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(
            folders,
            vec![
                ("linux:second".into(), second.folder.clone()),
                ("windows:first".into(), first.folder.clone())
            ]
        );
        let header: (String, String, Option<String>) = db
            .query_row(
                "SELECT name, slug, repository_path FROM projects",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(header, (first.name.clone(), first.id.clone(), None));
        drop(db);

        // Git brings both records back to the first computer; opening does not rewrite them.
        fs::copy(
            settings::project_database_path(&second),
            settings::project_database_path(&first),
        )
        .unwrap();
        let before = fs::read(settings::project_database_path(&first)).unwrap();
        drop(open_project_database_for_computer(&first, "windows:first").unwrap());
        assert_eq!(
            before,
            fs::read(settings::project_database_path(&first)).unwrap()
        );

        // Moving this computer's checkout changes only its own mapping.
        let moved = fixture("moved");
        fs::create_dir_all(settings::project_data_dir(&moved)).unwrap();
        fs::copy(
            settings::project_database_path(&first),
            settings::project_database_path(&moved),
        )
        .unwrap();
        let db = open_project_database_for_computer(&moved, "windows:first").unwrap();
        assert_eq!(
            db.query_row(
                "SELECT repository_path FROM project_computers WHERE computer_id='linux:second'",
                [],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
            second.folder
        );
        assert_eq!(
            db.query_row(
                "SELECT repository_path FROM project_computers WHERE computer_id='windows:first'",
                [],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
            moved.folder
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM project_computers", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            2
        );
        drop(db);
        for project in [first, second, moved] {
            fs::remove_dir_all(project.folder).unwrap();
        }
    }

    #[test]
    fn repeated_reads_leave_database_bytes_timestamp_and_revision_unchanged() {
        for demo in [false, true] {
            let mut project = fixture("read-only");
            if demo {
                project.id = "adashi".into();
                project.name = "Adashi".into();
            }
            let db = open_project_database_for_computer(&project, "test:computer").unwrap();
            let revision = crate::state::load_project_revision(&db, 1)
                .unwrap()
                .revision;
            drop(db);
            let path = settings::project_database_path(&project);
            let before = fs::read(&path).unwrap();
            let modified = fs::metadata(&path).unwrap().modified().unwrap();
            for _ in 0..3 {
                let db = open_project_database_for_computer(&project, "test:computer").unwrap();
                crate::memory::load_memory(&db, 1).unwrap();
                fixed_hooks::load_fixed_hook_prompts(&db, 1).unwrap();
                crate::rules::load_rules(&db).unwrap();
                crate::tasks::load_tasks(&db, 1, &crate::tasks::ALL_TASK_STATES).unwrap();
                crate::qa::load_jobs(&db, 1, None).unwrap();
                assert_eq!(db.total_changes(), 0);
                assert_eq!(
                    crate::state::load_project_revision(&db, 1)
                        .unwrap()
                        .revision,
                    revision
                );
                drop(db);
                assert_eq!(fs::read(&path).unwrap(), before);
                assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
            }
            fs::remove_dir_all(project.folder).unwrap();
        }
    }

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
            architecture_projection: Default::default(),
        }
    }

    #[test]
    fn project_reference_is_required() {
        let error = resolve_project_from_settings(&settings_with_projects(), Some("   "))
            .expect_err("blank references must fail");
        assert_eq!(
            error,
            "projectName is required and must be a configured project id or project name"
        );
    }

    #[test]
    fn ambiguous_project_names_fail_loudly_instead_of_picking_one() {
        let mut settings = settings_with_projects();
        settings.projects.push(ProjectSettings {
            id: "other-adashi".to_string(),
            name: "adashi".to_string(),
            folder: "C:\\src\\Other".to_string(),
        });

        let error = resolve_project_from_settings(&settings, Some("ADASHI"))
            .expect_err("a name shared by two projects must not resolve");
        assert!(error.contains("Ambiguous project name"), "{error}");
        assert!(error.contains("ADASHI"), "{error}");

        // An exact id stays unambiguous even when the display name is not.
        assert_eq!(
            resolve_project_from_settings(&settings, Some("other-adashi"))
                .unwrap()
                .name,
            "adashi"
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
