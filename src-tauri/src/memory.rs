use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

pub const DEFAULT_MEMORY_RULE: &str = r#"LONG-TERM MEMORY PROTOCOL:
- Before starting any task, silently call `adashi_get_memory` to understand the current project state, recent architectural decisions, and ongoing bugs.
- At the end of every successful task or major discussion, you MUST call `adashi_update_memory`.
- Summarize what you just built, any API quirks you discovered, and what the next logical steps are. Do not ask for permission to do this, just update the memory through Adashi."#;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(rmcp::schemars::JsonSchema)]
pub struct ProjectMemory {
    pub rule: String,
    pub memory: String,
    pub updated_at: String,
}

pub fn ensure_project_memory(db: &Connection, project_id: i64) -> Result<(), String> {
    db.execute(
        "INSERT OR IGNORE INTO project_memory(project_id, protocol_rule, memory_body)
         VALUES (?1, ?2, '')",
        params![project_id, DEFAULT_MEMORY_RULE],
    )
    .map_err(|err| err.to_string())?;

    Ok(())
}

pub fn load_memory(db: &Connection, project_id: i64) -> Result<ProjectMemory, String> {
    ensure_project_memory(db, project_id)?;

    db.query_row(
        "SELECT protocol_rule, memory_body, updated_at
         FROM project_memory
         WHERE project_id = ?1",
        params![project_id],
        |row| {
            Ok(ProjectMemory {
                rule: row.get(0)?,
                memory: row.get(1)?,
                updated_at: row.get(2)?,
            })
        },
    )
    .map_err(|err| err.to_string())
}

pub fn update_memory_rule(
    db: &Connection,
    project_id: i64,
    rule: String,
) -> Result<ProjectMemory, String> {
    let rule = rule.trim();
    if rule.is_empty() {
        return Err("Memory rule is required".to_string());
    }

    ensure_project_memory(db, project_id)?;

    db.execute(
        "UPDATE project_memory
         SET protocol_rule = ?1,
             updated_at = CURRENT_TIMESTAMP
         WHERE project_id = ?2",
        params![rule, project_id],
    )
    .map_err(|err| err.to_string())?;

    load_memory(db, project_id)
}

pub fn update_memory(
    db: &Connection,
    project_id: i64,
    memory: String,
) -> Result<ProjectMemory, String> {
    ensure_project_memory(db, project_id)?;

    db.execute(
        "UPDATE project_memory
         SET memory_body = ?1,
             updated_at = CURRENT_TIMESTAMP
         WHERE project_id = ?2",
        params![memory, project_id],
    )
    .map_err(|err| err.to_string())?;

    load_memory(db, project_id)
}
