use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

pub const DESIGN_AUTHORING_HOOK_KEY: &str = "design.run.start.authoring";
pub const IMPLEMENTATION_GUIDANCE_HOOK_KEY: &str = "implementation.run.start.design-guide";

const LEGACY_DESIGN_RULE_NAME: &str = "Design MCP API Protocol";

pub const DEFAULT_DESIGN_AUTHORING_PROMPT: &str = r#"# Formal Design Authoring Hook

You are modifying or discussing formal design. The generated design context in this run.start injection is already loaded.

- Use the injected revision, C4 index, UML artifact types, attached artifacts, and bindings first.
- Retrieve additional design scope only when the injected context is insufficient for this design task.
- If the design changes, persist the coherent C4/UML/binding changes with one `adashi_design_save` call.
- Do not store design conclusions as chat notes."#;

pub const DEFAULT_IMPLEMENTATION_GUIDANCE_PROMPT: &str = r#"# Formal Design Implementation Guide

Use the injected formal design as implementation guidance.

- Align touched code with the injected C4 ids, UML artifacts, and file/symbol bindings.
- If code touches a designed component, preserve the intended responsibilities and relationships unless the user explicitly asks to redesign them.
- Retrieve narrower design scope or bindings only when the injected implementation guide is insufficient for the files, symbols, or component being changed.
- If implementation discovers the design is stale, report the mismatch instead of silently drifting away from the formal design."#;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(rmcp::schemars::JsonSchema)]
pub struct FixedHookPrompt {
    pub key: String,
    pub title: String,
    pub intend: String,
    pub hook: String,
    pub prompt: String,
    pub updated_at: String,
}

#[derive(Clone, Copy)]
struct FixedHookDefinition {
    key: &'static str,
    title: &'static str,
    intend: &'static str,
    hook: &'static str,
    default_prompt: &'static str,
}

pub fn ensure_fixed_hook_prompts(db: &Connection) -> Result<(), String> {
    let mut statement = db
        .prepare("SELECT id FROM projects")
        .map_err(|err| err.to_string())?;
    let project_ids = statement
        .query_map([], |row| row.get::<_, i64>(0))
        .map_err(|err| err.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| err.to_string())?;

    for project_id in project_ids {
        let legacy_design_prompt = db
            .query_row(
                "SELECT prompt
                 FROM rules
                 WHERE project_id = ?1
                   AND name = ?2
                   AND intend = 'design'
                   AND hook = 'run.start'
                 ORDER BY id
                 LIMIT 1",
                params![project_id, LEGACY_DESIGN_RULE_NAME],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|err| err.to_string())?;

        for definition in fixed_hook_definitions() {
            let default_prompt = if definition.key == DESIGN_AUTHORING_HOOK_KEY {
                legacy_design_prompt
                    .as_deref()
                    .filter(|prompt| !prompt.trim().is_empty())
                    .unwrap_or(definition.default_prompt)
            } else {
                definition.default_prompt
            };
            insert_default_prompt(db, project_id, definition, default_prompt)?;
        }

        db.execute(
            "DELETE FROM rules
             WHERE project_id = ?1
               AND name = ?2
               AND intend = 'design'
               AND hook = 'run.start'",
            params![project_id, LEGACY_DESIGN_RULE_NAME],
        )
        .map_err(|err| err.to_string())?;
    }

    Ok(())
}

pub fn load_fixed_hook_prompts(
    db: &Connection,
    project_id: i64,
) -> Result<Vec<FixedHookPrompt>, String> {
    ensure_fixed_hook_prompts(db)?;

    let mut statement = db
        .prepare(
            "SELECT key, title, intend, hook, prompt, updated_at
             FROM fixed_hook_prompts
             WHERE project_id = ?1
             ORDER BY
                CASE key
                    WHEN 'design.run.start.authoring' THEN 1
                    WHEN 'implementation.run.start.design-guide' THEN 2
                    ELSE 3
                END,
                key",
        )
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map(params![project_id], |row| {
            Ok(FixedHookPrompt {
                key: row.get(0)?,
                title: row.get(1)?,
                intend: row.get(2)?,
                hook: row.get(3)?,
                prompt: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })
        .map_err(|err| err.to_string())?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| err.to_string())
}

pub fn load_prompt(db: &Connection, project_id: i64, key: &str) -> Result<Option<String>, String> {
    ensure_fixed_hook_prompts(db)?;

    db.query_row(
        "SELECT prompt
         FROM fixed_hook_prompts
         WHERE project_id = ?1 AND key = ?2",
        params![project_id, key],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(|err| err.to_string())
}

pub fn update_fixed_hook_prompt(
    db: &Connection,
    project_id: i64,
    key: String,
    prompt: String,
) -> Result<FixedHookPrompt, String> {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err("Fixed hook prompt is required".to_string());
    }

    ensure_fixed_hook_prompts(db)?;

    let affected = db
        .execute(
            "UPDATE fixed_hook_prompts
             SET prompt = ?1,
                 updated_at = CURRENT_TIMESTAMP
             WHERE project_id = ?2 AND key = ?3",
            params![prompt, project_id, key],
        )
        .map_err(|err| err.to_string())?;

    if affected == 0 {
        return Err(format!("Unknown fixed hook prompt key: {key}"));
    }

    load_fixed_hook_prompts(db, project_id)?
        .into_iter()
        .find(|prompt| prompt.key == key)
        .ok_or_else(|| format!("Unknown fixed hook prompt key: {key}"))
}

fn insert_default_prompt(
    db: &Connection,
    project_id: i64,
    definition: FixedHookDefinition,
    prompt: &str,
) -> Result<(), String> {
    db.execute(
        "INSERT INTO fixed_hook_prompts(project_id, key, title, intend, hook, prompt)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(project_id, key) DO NOTHING",
        params![
            project_id,
            definition.key,
            definition.title,
            definition.intend,
            definition.hook,
            prompt
        ],
    )
    .map_err(|err| err.to_string())?;
    Ok(())
}

fn fixed_hook_definitions() -> [FixedHookDefinition; 2] {
    [
        FixedHookDefinition {
            key: DESIGN_AUTHORING_HOOK_KEY,
            title: "Design Authoring Hook",
            intend: "design",
            hook: "run.start",
            default_prompt: DEFAULT_DESIGN_AUTHORING_PROMPT,
        },
        FixedHookDefinition {
            key: IMPLEMENTATION_GUIDANCE_HOOK_KEY,
            title: "Implementation Guidance Hook",
            intend: "implementation",
            hook: "run.start",
            default_prompt: DEFAULT_IMPLEMENTATION_GUIDANCE_PROMPT,
        },
    ]
}
