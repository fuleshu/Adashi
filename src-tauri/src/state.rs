use rusqlite::{params, Connection};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRevision {
    pub revision: i64,
    pub updated_at: String,
}

pub fn ensure_project_state(db: &Connection) -> rusqlite::Result<()> {
    db.execute(
        "INSERT OR IGNORE INTO project_state(project_id)
         SELECT id FROM projects",
        [],
    )?;
    Ok(())
}

pub fn load_project_revision(db: &Connection, project_id: i64) -> Result<ProjectRevision, String> {
    db.query_row(
        "SELECT revision, updated_at FROM project_state WHERE project_id = ?1",
        params![project_id],
        |row| {
            Ok(ProjectRevision {
                revision: row.get(0)?,
                updated_at: row.get(1)?,
            })
        },
    )
    .map_err(|err| err.to_string())
}

pub fn bump_project_revision(db: &Connection, project_id: i64) -> Result<ProjectRevision, String> {
    db.execute(
        "INSERT OR IGNORE INTO project_state(project_id) VALUES (?1)",
        params![project_id],
    )
    .map_err(|err| err.to_string())?;

    db.execute(
        "UPDATE project_state
         SET revision = revision + 1,
             updated_at = CURRENT_TIMESTAMP
         WHERE project_id = ?1",
        params![project_id],
    )
    .map_err(|err| err.to_string())?;

    load_project_revision(db, project_id)
}
