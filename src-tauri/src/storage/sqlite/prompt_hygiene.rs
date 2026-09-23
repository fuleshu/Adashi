use crate::prompt_hygiene::rewrite_removed_tool_references;
pub fn repair_stored_prompts(db: &rusqlite::Connection, project_id: i64) -> Result<usize, String> {
    let mut repairs: Vec<(i64, String)> = Vec::new();

    {
        let mut statement = db
            .prepare("SELECT id, prompt FROM rules WHERE project_id = ?1")
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([project_id], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| error.to_string())?;
        for row in rows {
            let (id, prompt) = row.map_err(|error| error.to_string())?;
            if let Some(rewritten) = rewrite_removed_tool_references(&prompt) {
                repairs.push((id, rewritten));
            }
        }
    }

    let mut fixed_hook_repairs: Vec<(String, String)> = Vec::new();
    {
        let mut statement = db
            .prepare("SELECT key, prompt FROM fixed_hook_prompts WHERE project_id = ?1")
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([project_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| error.to_string())?;
        for row in rows {
            let (key, prompt) = row.map_err(|error| error.to_string())?;
            if let Some(rewritten) = rewrite_removed_tool_references(&prompt) {
                fixed_hook_repairs.push((key, rewritten));
            }
        }
    }

    if repairs.is_empty() && fixed_hook_repairs.is_empty() {
        return Ok(0);
    }

    for (id, rewritten) in &repairs {
        db.execute(
            "UPDATE rules SET prompt = ?1 WHERE id = ?2",
            rusqlite::params![rewritten, id],
        )
        .map_err(|error| error.to_string())?;
        super::concurrency::bump_version(db, project_id, "rule", &id.to_string())?;
    }
    for (key, rewritten) in &fixed_hook_repairs {
        db.execute(
            "UPDATE fixed_hook_prompts SET prompt = ?1, updated_at = CURRENT_TIMESTAMP
             WHERE project_id = ?2 AND key = ?3",
            rusqlite::params![rewritten, project_id, key],
        )
        .map_err(|error| error.to_string())?;
        super::concurrency::bump_version(db, project_id, "fixed-hook", key)?;
    }

    super::state::bump_project_revision(db, project_id)?;
    Ok(repairs.len() + fixed_hook_repairs.len())
}
