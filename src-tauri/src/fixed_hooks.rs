use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

pub const DESIGN_AUTHORING_HOOK_KEY: &str = "design.run.start.authoring";
pub const IMPLEMENTATION_GUIDANCE_HOOK_KEY: &str = "implementation.run.start.design-guide";

const LEGACY_DESIGN_RULE_NAME: &str = "Design MCP API Protocol";

const LEGACY_DESIGN_AUTHORING_PROMPT: &str = r#"# Formal Design Authoring Hook

You are modifying or discussing formal design. The generated design context in this run.start injection is already loaded.

- Use the injected revision, C4 index, UML artifact types, attached artifacts, and bindings first.
- Retrieve additional design scope only when the injected context is insufficient for this design task.
- If the design changes, persist the coherent C4/UML/binding changes with one `adashi_design_save` call.
- Do not store design conclusions as chat notes."#;

const LEGACY_IMPLEMENTATION_GUIDANCE_PROMPT: &str = r#"# Formal Design Implementation Guide

Use the injected formal design as implementation guidance.

- Align touched code with the injected C4 ids, UML artifacts, and file/symbol bindings.
- If code touches a designed component, preserve the intended responsibilities and relationships unless the user explicitly asks to redesign them.
- Retrieve narrower design scope or bindings only when the injected implementation guide is insufficient for the files, symbols, or component being changed.
- If implementation discovers the design is stale, report the mismatch instead of silently drifting away from the formal design."#;

/// Previous defaults that referenced the pre-grouping tool names; migrated on load.
const LEGACY_V2_DESIGN_AUTHORING_PROMPT: &str = r#"# Formal Design Authoring Hook

The startup design index identifies retrieval entry points, not full design guidance.
- Select relevant ids with the index or adashi_design_search; read their scopes, artifacts and file/symbol bindings before changing formal design.
- Preserve intended responsibilities and relationships unless the user authorizes a redesign.
- Persist coherent C4/UML/binding changes with adashi_design_save. Do not store design conclusions as chat notes."#;

const LEGACY_V2_IMPLEMENTATION_GUIDANCE_PROMPT: &str = r#"# Formal Design Implementation Guide

The startup design index identifies retrieval entry points, not full implementation guidance.
- Retrieve design bound to touched files/symbols with adashi_design_get_bindings, then relevant scopes/artifacts by explicit ids.
- Align code with those responsibilities and relationships unless the user authorizes a redesign.
- If implementation discovers stale design, report the mismatch instead of silently drifting away from it."#;

const LEGACY_V3_DESIGN_AUTHORING_PROMPT: &str = r#"# Formal Design Authoring Hook

The startup design index identifies retrieval entry points, not full design guidance.
- Select relevant ids with the index or the adashi_design search operation; read their scopes, artifacts and file/symbol bindings before changing formal design.
- Preserve intended responsibilities and relationships unless the user authorizes a redesign.
- Persist coherent C4/UML/binding changes with the adashi_design save operation. Do not store design conclusions as chat notes."#;

const LEGACY_V3_IMPLEMENTATION_GUIDANCE_PROMPT: &str = r#"# Formal Design Implementation Guide

The startup design index identifies retrieval entry points, not full implementation guidance.
- Retrieve design bound to touched files/symbols with the adashi_design get_bindings operation, then relevant scopes/artifacts by explicit ids.
- Align code with those responsibilities and relationships unless the user authorizes a redesign.
- If implementation discovers stale design, report the mismatch instead of silently drifting away from it."#;

// Shared workflow now belongs to agents_template.md and on-demand operation help.
// Empty defaults leave these slots for optional project-specific instructions.
pub const DEFAULT_DESIGN_AUTHORING_PROMPT: &str = "";
pub const DEFAULT_IMPLEMENTATION_GUIDANCE_PROMPT: &str = "";

// Historical full authoring default, also found after tool-name repair in installed projects.
const LEGACY_FULL_DESIGN_AUTHORING_PROMPT: &str = r#"# Formal Design Authoring Hook

For every `design` intend run, treat the generated formal design context in this run.start injection as already loaded. Do not store design conclusions as chat notes.

Hook-specific workflow:
- Use the injected revision, C4 index, UML artifact types, attached artifacts, and bindings before calling any additional design retrieval tools.
- Call deterministic retrieval such as `adashi_design search`, `adashi_design get_scope`, `adashi_design get_by_ids`, or `adashi_design get_bindings` only when the injected context is insufficient for the specific design question or changed scope.
- Keep all design reasoning in the agent. The MCP retrieves explicit scopes, ids, tags, source, and stored bindings; it must not infer design context from a natural-language task.
- For component or relationship detail, inspect `umlArtifactTypes` and existing diagram metadata (`diagramType`, `artifactRole`, `artifactLabel`, `artifactRank`, `attachedToExternalId`, `attachedToTargetType`) before choosing or creating a UML artifact. Prefer class/package/component-style structure for architecture and contracts, sequence for interactions, flow/activity for workflows, and state for lifecycle behavior.
- Save finished design work with one transactional `adashi_design save` call. Include an `expectedRevision`, a clear `changeIntent`, explicit parent ids or UML artifact attachments, and every C4/UML/binding change needed for a coherent model.
- If `adashi_design save` returns `ok: false`, correct the formal source or structure and retry. Do not treat a rejected save as persisted design.

`adashi_design save` is the validation and persistence boundary. It rejects stale revisions, missing parents, invalid C4 containment, duplicate ids, unresolved relationships, orphan internal elements, invalid UML syntax, and incomplete source/semantic round trips.

Store C4 as canonical Structurizr DSL/JSON generated from validated semantic rows. Store UML as explicit typed Mermaid artifacts attached to a C4 element or relationship. The agent decides which artifact type to read or write from the task intent and existing artifact inventory; the MCP exposes facts and validation, not intelligent task inference."#;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(rmcp::schemars::JsonSchema)]
pub struct FixedHookPrompt {
    pub version: i64,
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
            migrate_builtin_prompt(db, project_id, definition)?;
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

        // Repair runs last: it rewrites identifiers, so it must not change stored text before
        // the exact-text legacy comparisons above have had their chance to match.
        crate::prompt_hygiene::repair_stored_prompts(db, project_id)?;
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
            "SELECT f.key, f.title, f.intend, f.hook, f.prompt, f.updated_at, COALESCE(rv.version, 0)
             FROM fixed_hook_prompts f
             LEFT JOIN resource_versions rv
               ON rv.project_id=f.project_id AND rv.resource_kind='fixed-hook' AND rv.resource_id=f.key
             WHERE f.project_id = ?1
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
                version: row.get(6)?,
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

fn migrate_builtin_prompt(
    db: &Connection,
    project_id: i64,
    definition: FixedHookDefinition,
) -> Result<(), String> {
    let legacy_prompts: &[&str] = if definition.key == DESIGN_AUTHORING_HOOK_KEY {
        &[
            LEGACY_DESIGN_AUTHORING_PROMPT,
            LEGACY_V2_DESIGN_AUTHORING_PROMPT,
            LEGACY_V3_DESIGN_AUTHORING_PROMPT,
            LEGACY_FULL_DESIGN_AUTHORING_PROMPT,
        ]
    } else {
        &[
            LEGACY_IMPLEMENTATION_GUIDANCE_PROMPT,
            LEGACY_V2_IMPLEMENTATION_GUIDANCE_PROMPT,
            LEGACY_V3_IMPLEMENTATION_GUIDANCE_PROMPT,
        ]
    };
    db.execute_batch("SAVEPOINT fixed_prompt_migration")
        .map_err(|e| e.to_string())?;
    let result = (|| {
        let current: String = db
            .query_row(
                "SELECT prompt FROM fixed_hook_prompts WHERE project_id=?1 AND key=?2",
                params![project_id, definition.key],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        let changed = if legacy_prompts
            .iter()
            .any(|legacy| builtin_key(&current) == builtin_key(legacy))
        {
            db.execute(
                "UPDATE fixed_hook_prompts SET prompt=?1, updated_at=CURRENT_TIMESTAMP WHERE project_id=?2 AND key=?3",
                params![definition.default_prompt, project_id, definition.key],
            ).map_err(|e| e.to_string())?
        } else {
            0
        };
        if changed > 0 {
            crate::concurrency::bump_version(db, project_id, "fixed-hook", definition.key)?;
            crate::state::bump_project_revision(db, project_id)?;
        }
        Ok::<_, String>(())
    })();
    if result.is_err() {
        let _ =
            db.execute_batch("ROLLBACK TO fixed_prompt_migration; RELEASE fixed_prompt_migration");
        return result;
    }
    db.execute_batch("RELEASE fixed_prompt_migration")
        .map_err(|e| e.to_string())
}

// Normalize only known identifier rewrites and line endings, never project-specific prose.
fn builtin_key(prompt: &str) -> String {
    let normalized = prompt.replace("\r\n", "\n").trim().to_string();
    crate::prompt_hygiene::rewrite_removed_tool_references(&normalized).unwrap_or(normalized)
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn full_revision_prompt_and_repaired_defaults_retire_once_but_custom_text_survives() {
        let mut db = Connection::open_in_memory().unwrap();
        crate::schema::migrate(&mut db).unwrap();
        db.execute("INSERT INTO projects(name,slug) VALUES('P','p')", [])
            .unwrap();
        crate::state::ensure_project_state(&db).unwrap();
        ensure_fixed_hook_prompts(&db).unwrap();
        for text in [
            LEGACY_DESIGN_AUTHORING_PROMPT,
            LEGACY_V2_DESIGN_AUTHORING_PROMPT,
            LEGACY_V3_DESIGN_AUTHORING_PROMPT,
            LEGACY_FULL_DESIGN_AUTHORING_PROMPT,
        ] {
            for variant in [
                text.to_string(),
                builtin_key(text),
                text.replace("adashi_design save", "adashi_design_save")
                    .replace('\n', "\r\n"),
            ] {
                db.execute(
                    "UPDATE fixed_hook_prompts SET prompt=?1 WHERE key=?2",
                    params![variant, DESIGN_AUTHORING_HOOK_KEY],
                )
                .unwrap();
                let before = crate::concurrency::load_version(
                    &db,
                    1,
                    "fixed-hook",
                    DESIGN_AUTHORING_HOOK_KEY,
                )
                .unwrap();
                ensure_fixed_hook_prompts(&db).unwrap();
                assert_eq!(
                    load_prompt(&db, 1, DESIGN_AUTHORING_HOOK_KEY)
                        .unwrap()
                        .as_deref(),
                    Some("")
                );
                assert_eq!(
                    crate::concurrency::load_version(
                        &db,
                        1,
                        "fixed-hook",
                        DESIGN_AUTHORING_HOOK_KEY
                    )
                    .unwrap(),
                    before + 1
                );
            }
        }
        let custom = format!("{LEGACY_FULL_DESIGN_AUTHORING_PROMPT}\nProject-specific addition: preserve the audit trail.");
        let result =
            update_fixed_hook_prompt(&db, 1, DESIGN_AUTHORING_HOOK_KEY.into(), custom.clone())
                .unwrap();
        assert_eq!(result.prompt, custom); // Similar is not the same as a known built-in.
        assert_eq!(
            update_fixed_hook_prompt(&db, 1, DESIGN_AUTHORING_HOOK_KEY.into(), "".into())
                .unwrap()
                .prompt,
            ""
        );
    }

    #[test]
    fn builtin_migration_is_versioned_once_and_preserves_custom_prompts() {
        let mut db = Connection::open_in_memory().unwrap();
        crate::schema::migrate(&mut db).unwrap();
        db.execute("INSERT INTO projects(name,slug) VALUES('P','p')", [])
            .unwrap();
        crate::state::ensure_project_state(&db).unwrap();
        ensure_fixed_hook_prompts(&db).unwrap();
        db.execute(
            "UPDATE fixed_hook_prompts SET prompt=?1 WHERE key=?2",
            params![
                LEGACY_IMPLEMENTATION_GUIDANCE_PROMPT,
                IMPLEMENTATION_GUIDANCE_HOOK_KEY
            ],
        )
        .unwrap();
        db.execute("UPDATE fixed_hook_prompts SET prompt='Required custom authoring guidance.' WHERE key=?1",
            [DESIGN_AUTHORING_HOOK_KEY]).unwrap();
        ensure_fixed_hook_prompts(&db).unwrap();
        assert_eq!(
            load_prompt(&db, 1, IMPLEMENTATION_GUIDANCE_HOOK_KEY)
                .unwrap()
                .unwrap(),
            DEFAULT_IMPLEMENTATION_GUIDANCE_PROMPT
        );
        assert_eq!(
            load_prompt(&db, 1, DESIGN_AUTHORING_HOOK_KEY)
                .unwrap()
                .unwrap(),
            "Required custom authoring guidance."
        );
        let revision = crate::state::load_project_revision(&db, 1)
            .unwrap()
            .revision;
        let version = crate::concurrency::load_version(
            &db,
            1,
            "fixed-hook",
            IMPLEMENTATION_GUIDANCE_HOOK_KEY,
        )
        .unwrap();
        assert!(version > 0);
        ensure_fixed_hook_prompts(&db).unwrap();
        assert_eq!(
            crate::state::load_project_revision(&db, 1)
                .unwrap()
                .revision,
            revision
        );
    }
}
