//! Prompt hygiene for stored instruction text.
//!
//! Stored rules and fixed-hook prompts survive upgrades, so they keep naming MCP tools that
//! a later capability grouping may have removed. That rot is silent: the agent is told to
//! call a tool that no longer exists. This module keeps one source of truth for tool names,
//! rewrites references it can map exactly, and reports the ones it cannot.

use std::collections::BTreeSet;

/// MCP tool names exposed to clients. One source of truth so prompt hygiene cannot drift
/// from the real surface; `mcp::tests::tool_name_registry_matches_the_router` enforces it.
pub(crate) const MCP_TOOL_NAMES: &[&str] = &[
    "adashi_design",
    "adashi_intents",
    "adashi_memory",
    "adashi_qa",
    "adashi_rules",
    "adashi_tasks",
];

/// Removed tool identifiers mapped to the capability phrasing that replaced them.
///
/// Rewrites are exact identifier substitutions, never content edits. Ordering is resolved by
/// length at match time, so a name that contains another name cannot be shadowed.
const REMOVED_TOOL_REFERENCES: &[(&str, &str)] = &[
    ("adashi_design_save", "the `adashi_design` operation `save`"),
    (
        "adashi_design_set_element_descriptions",
        "the `adashi_design` operation `set_element_descriptions`",
    ),
    (
        "adashi_design_get_overview",
        "the `adashi_design` operation `get_overview`",
    ),
    (
        "adashi_design_get_scope",
        "the `adashi_design` operation `get_scope`",
    ),
    (
        "adashi_design_get_by_ids",
        "the `adashi_design` operation `get_by_ids`",
    ),
    (
        "adashi_design_get_bindings",
        "the `adashi_design` operation `get_bindings`",
    ),
    (
        "adashi_design_search",
        "the `adashi_design` operation `search`",
    ),
    (
        "adashi_mockup_list_pending_revisions",
        "the `adashi_design` operation `mockup_list_pending_revisions`",
    ),
    (
        "adashi_mockup_get_revision_context",
        "the `adashi_design` operation `mockup_get_revision_context`",
    ),
    ("adashi_list_rules", "the `adashi_rules` operation `list`"),
    ("adashi_create_rule", "the `adashi_rules` operation `create`"),
    ("adashi_update_rule", "the `adashi_rules` operation `update`"),
    ("adashi_delete_rule", "the `adashi_rules` operation `delete`"),
    (
        "adashi_get_rule_injections",
        "the `adashi_rules` operation `get_rule_injections`",
    ),
    ("adashi_create_task", "the `adashi_tasks` operation `create`"),
    ("adashi_list_tasks", "the `adashi_tasks` operation `list`"),
    ("adashi_update_task", "the `adashi_tasks` operation `update`"),
    ("adashi_finish_task", "the `adashi_tasks` operation `finish`"),
    ("adashi_delete_task", "the `adashi_tasks` operation `delete`"),
    ("adashi_get_task", "the `adashi_tasks` operation `get`"),
    (
        "adashi_create_qa_job",
        "the `adashi_qa` operation `create_job`",
    ),
    (
        "adashi_update_qa_job",
        "the `adashi_qa` operation `update_job`",
    ),
    (
        "adashi_delete_qa_job",
        "the `adashi_qa` operation `delete_job`",
    ),
    ("adashi_list_qa_jobs", "the `adashi_qa` operation `list_jobs`"),
    ("adashi_run_qa_jobs", "the `adashi_qa` operation `run_jobs`"),
    ("adashi_list_qa_runs", "the `adashi_qa` operation `list_runs`"),
    ("adashi_get_qa_job", "the `adashi_qa` operation `get_job`"),
    ("adashi_get_memory", "the `adashi_memory` operation `get`"),
    (
        "adashi_append_memory_note",
        "the `adashi_memory` operation `append`",
    ),
    ("adashi_update_memory", "the `adashi_memory` operation `update`"),
    (
        "adashi_update_memory_rule",
        "the `adashi_memory` operation `update_rule`",
    ),
    (
        "adashi_publish_resource_intent",
        "the `adashi_intents` operation `publish`",
    ),
    (
        "adashi_list_resource_intents",
        "the `adashi_intents` operation `list`",
    ),
];

/// Identifier prefix that marks a token as an Adashi MCP tool reference.
const TOOL_REFERENCE_PREFIX: &str = "adashi_";

/// Rewrites removed tool identifiers to capability phrasing, or returns `None` unchanged.
///
/// Longest removed name wins, so `adashi_update_memory_rule` is rewritten as one unit rather
/// than being partially matched by `adashi_update_memory`.
pub(crate) fn rewrite_removed_tool_references(text: &str) -> Option<String> {
    let mut removed = REMOVED_TOOL_REFERENCES.to_vec();
    removed.sort_by_key(|(name, _)| std::cmp::Reverse(name.len()));

    let mut rewritten = text.to_string();
    let mut changed = false;
    for (name, replacement) in removed {
        if rewritten.contains(name) {
            rewritten = rewritten.replace(name, replacement);
            changed = true;
        }
    }
    changed.then_some(rewritten)
}

/// A stored prompt that names MCP tools which no longer exist.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptWarning {
    /// Stable source label, for example `rule:12` or `fixed-hook:design.run.start.authoring`.
    pub source: String,
    /// Human-readable name of the prompt.
    pub label: String,
    /// Identifiers that resolve to no tool and no known removed tool.
    pub unknown_tools: Vec<String>,
}

/// Reports a warning when `text` names tools that do not exist, so a human can repair it.
pub fn warning_for(source: &str, label: &str, text: &str) -> Option<PromptWarning> {
    let unknown_tools = unknown_tool_references(text);
    if unknown_tools.is_empty() {
        return None;
    }
    Some(PromptWarning {
        source: source.to_string(),
        label: label.to_string(),
        unknown_tools,
    })
}

/// Adashi-looking identifiers in `text` that are neither a current tool nor a known removed
/// tool, sorted and de-duplicated. These are references a human must resolve.
pub(crate) fn unknown_tool_references(text: &str) -> Vec<String> {
    let mut unknown = BTreeSet::new();
    let bytes = text.as_bytes();
    let mut index = 0;
    while let Some(offset) = text[index..].find(TOOL_REFERENCE_PREFIX) {
        let start = index + offset;
        let mut end = start + TOOL_REFERENCE_PREFIX.len();
        while end < bytes.len() {
            let character = bytes[end] as char;
            if character.is_ascii_alphanumeric() || character == '_' {
                end += 1;
            } else {
                break;
            }
        }
        let token = &text[start..end];
        if !MCP_TOOL_NAMES.contains(&token)
            && !REMOVED_TOOL_REFERENCES.iter().any(|(name, _)| *name == token)
        {
            unknown.insert(token.to_string());
        }
        index = end;
    }
    unknown.into_iter().collect()
}

/// Applies the exact-reference repair to every stored prompt for one project.
///
/// Returns the number of prompts rewritten. Versions are bumped so caches keyed on version
/// or contentVersion observe the change, and the project revision moves once.
pub fn repair_stored_prompts(db: &rusqlite::Connection, project_id: i64) -> Result<usize, String> {
    let mut repairs: Vec<(i64, String, String)> = Vec::new();

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
                repairs.push((id, prompt, rewritten));
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

    for (id, _, rewritten) in &repairs {
        db.execute(
            "UPDATE rules SET prompt = ?1 WHERE id = ?2",
            rusqlite::params![rewritten, id],
        )
        .map_err(|error| error.to_string())?;
        crate::concurrency::bump_version(db, project_id, "rule", &id.to_string())?;
    }
    for (key, rewritten) in &fixed_hook_repairs {
        db.execute(
            "UPDATE fixed_hook_prompts SET prompt = ?1, updated_at = CURRENT_TIMESTAMP
             WHERE project_id = ?2 AND key = ?3",
            rusqlite::params![rewritten, project_id, key],
        )
        .map_err(|error| error.to_string())?;
        crate::concurrency::bump_version(db, project_id, "fixed-hook", key)?;
    }

    crate::state::bump_project_revision(db, project_id)?;
    Ok(repairs.len() + fixed_hook_repairs.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn longest_removed_name_wins_so_prefixes_cannot_shadow() {
        let rewritten = rewrite_removed_tool_references(
            "Call adashi_update_memory_rule then adashi_update_memory.",
        )
        .unwrap();
        assert_eq!(
            rewritten,
            "Call the `adashi_memory` operation `update_rule` then the `adashi_memory` operation `update`."
        );
    }

    #[test]
    fn capability_phrasing_is_left_alone() {
        assert_eq!(
            rewrite_removed_tool_references("Use the adashi_design get_scope operation."),
            None
        );
    }

    #[test]
    fn unknown_references_are_reported_once_and_sorted() {
        let text = "adashi_zzz_tool and adashi_aaa_tool and adashi_zzz_tool, plus adashi_design.";
        assert_eq!(
            unknown_tool_references(text),
            vec!["adashi_aaa_tool".to_string(), "adashi_zzz_tool".to_string()]
        );
    }

    #[test]
    fn current_and_mapped_names_are_not_reported_as_unknown() {
        assert!(unknown_tool_references(
            "adashi_design adashi_qa adashi_design_save adashi_list_tasks"
        )
        .is_empty());
    }

    #[test]
    fn every_removed_name_rewrites_without_leaving_the_old_identifier() {
        for (name, _) in REMOVED_TOOL_REFERENCES {
            let rewritten = rewrite_removed_tool_references(name).expect("removed name rewrites");
            assert!(
                !rewritten.contains(name),
                "{name} survived its own rewrite as {rewritten}"
            );
            assert!(unknown_tool_references(&rewritten).is_empty());
        }
    }

    /// Manual verification against this workspace's real project database: the stored prompts
    /// really do carry removed tool names, and the repair clears all of them.
    #[test]
    #[ignore = "manual verification against this workspace's real project database"]
    fn repairs_removed_tool_references_in_the_real_workspace_project() {
        let source = std::path::Path::new(r"C:\src\Adashi\.adashi\adashi.sqlite3");
        assert!(source.is_file(), "real project database not available");

        let root = std::env::temp_dir().join("adashi-prompt-hygiene-real");
        let project_folder = root.join("project");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(project_folder.join(".adashi")).unwrap();
        std::fs::copy(source, project_folder.join(".adashi/adashi.sqlite3")).unwrap();

        let project = crate::settings::ProjectSettings {
            id: "adashi".into(),
            name: "Adashi".into(),
            folder: project_folder.to_string_lossy().into_owned(),
        };
        let db = crate::open_project_database(&project).unwrap();

        let stale_before: i64 = db
            .query_row(
                "SELECT (SELECT COUNT(*) FROM fixed_hook_prompts WHERE prompt LIKE '%adashi_design_%')
                      + (SELECT COUNT(*) FROM rules WHERE prompt LIKE '%adashi_design_%')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        println!("prompts naming a removed design tool before repair: {stale_before}");

        crate::fixed_hooks::ensure_fixed_hook_prompts(&db).unwrap();

        let mut remaining = Vec::new();
        {
            let mut statement = db
                .prepare(
                    "SELECT key, prompt FROM fixed_hook_prompts
                     UNION ALL SELECT 'rule:' || id, prompt FROM rules",
                )
                .unwrap();
            let rows = statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .unwrap();

            for row in rows {
                let (origin, prompt) = row.unwrap();
                for unknown in unknown_tool_references(&prompt) {
                    remaining.push(format!("{origin} -> {unknown}"));
                }
            }
        }

        println!("unknown tool references after repair: {remaining:?}");
        assert!(remaining.is_empty(), "{remaining:?}");
        assert!(
            stale_before > 0,
            "fixture expected stored prompts to reference removed tools"
        );

        drop(db);
        let _ = std::fs::remove_dir_all(&root);
    }
}
