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
    "adashi_grep",
    "adashi_intents",
    "adashi_memory",
    "adashi_qa",
    "adashi_rules",
    "adashi_tasks",
];

/// Removed tool identifiers mapped to the capability that replaced them.
///
/// Each entry is `(removed identifier, owning tool, operation)`. Replacements render as plain
/// `tool operation` text with **no backticks of their own**: a removed name almost always sits
/// inside an existing code span, and a replacement that carried its own spans nested them and
/// produced malformed markdown. Leaving the phrasing bare lets the surrounding span, when there
/// is one, wrap the phrase correctly.
const REMOVED_TOOL_REFERENCES: &[(&str, &str, &str)] = &[
    ("adashi_design_save", "adashi_design", "save"),
    (
        "adashi_design_set_element_descriptions",
        "adashi_design",
        "set_element_descriptions",
    ),
    ("adashi_design_get_overview", "adashi_design", "get_overview"),
    ("adashi_design_get_scope", "adashi_design", "get_scope"),
    ("adashi_design_get_by_ids", "adashi_design", "get_by_ids"),
    ("adashi_design_get_bindings", "adashi_design", "get_bindings"),
    ("adashi_design_search", "adashi_design", "search"),
    (
        "adashi_mockup_list_pending_revisions",
        "adashi_design",
        "mockup_list_pending_revisions",
    ),
    (
        "adashi_mockup_get_revision_context",
        "adashi_design",
        "mockup_get_revision_context",
    ),
    ("adashi_list_rules", "adashi_rules", "list"),
    ("adashi_create_rule", "adashi_rules", "create"),
    ("adashi_update_rule", "adashi_rules", "update"),
    ("adashi_delete_rule", "adashi_rules", "delete"),
    (
        "adashi_get_rule_injections",
        "adashi_rules",
        "get_rule_injections",
    ),
    ("adashi_create_task", "adashi_tasks", "create"),
    ("adashi_list_tasks", "adashi_tasks", "list"),
    ("adashi_update_task", "adashi_tasks", "update"),
    ("adashi_finish_task", "adashi_tasks", "finish"),
    ("adashi_delete_task", "adashi_tasks", "delete"),
    ("adashi_get_task", "adashi_tasks", "get"),
    ("adashi_create_qa_job", "adashi_qa", "create_job"),
    ("adashi_update_qa_job", "adashi_qa", "update_job"),
    ("adashi_delete_qa_job", "adashi_qa", "delete_job"),
    ("adashi_list_qa_jobs", "adashi_qa", "list_jobs"),
    ("adashi_run_qa_jobs", "adashi_qa", "run_jobs"),
    ("adashi_list_qa_runs", "adashi_qa", "list_runs"),
    ("adashi_get_qa_job", "adashi_qa", "get_job"),
    ("adashi_get_memory", "adashi_memory", "get"),
    ("adashi_append_memory_note", "adashi_memory", "append"),
    ("adashi_update_memory", "adashi_memory", "update"),
    ("adashi_update_memory_rule", "adashi_memory", "update_rule"),
    ("adashi_publish_resource_intent", "adashi_intents", "publish"),
    ("adashi_list_resource_intents", "adashi_intents", "list"),
];

/// Identifier prefix that marks a token as an Adashi MCP tool reference.
const TOOL_REFERENCE_PREFIX: &str = "adashi_";

/// Rewrites removed tool identifiers to capability phrasing, or returns `None` unchanged.
///
/// Longest removed name wins, so `adashi_update_memory_rule` is rewritten as one unit rather
/// than being partially matched by `adashi_update_memory`.
///
/// Also repairs phrases written by an earlier release whose replacements carried their own code
/// spans, which nested inside the span the removed name already sat in. Both the malformed and
/// the clean form are derived from the same table, so they cannot drift apart.
pub(crate) fn rewrite_removed_tool_references(text: &str) -> Option<String> {
    let mut ordered = REMOVED_TOOL_REFERENCES.to_vec();
    ordered.sort_by_key(|(name, _, _)| std::cmp::Reverse(name.len()));

    let mut rewritten = text.to_string();
    let mut changed = false;

    // Malformed forms first: the bare form is a substring of the wrapped one, so repairing the
    // wrapped form first keeps the shorter pattern from consuming it.
    for (_, tool, operation) in &ordered {
        let malformed_wrapped = format!("`the `{tool}` operation `{operation}``");
        let malformed_bare = format!("the `{tool}` operation `{operation}`");

        if rewritten.contains(&malformed_wrapped) {
            rewritten = rewritten.replace(&malformed_wrapped, &format!("`{tool} {operation}`"));
            changed = true;
        }
        if rewritten.contains(&malformed_bare) {
            rewritten = rewritten.replace(&malformed_bare, &format!("{tool} {operation}"));
            changed = true;
        }
    }

    for (name, tool, operation) in &ordered {
        if rewritten.contains(name) {
            rewritten = rewritten.replace(name, &format!("{tool} {operation}"));
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
            && !REMOVED_TOOL_REFERENCES
                .iter()
                .any(|(name, _, _)| *name == token)
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

    /// Backticks stay balanced, and no code span is left empty or nested.
    fn assert_balanced_code_spans(text: &str) {
        assert_eq!(
            text.matches('`').count() % 2,
            0,
            "backticks must stay balanced: {text}"
        );
        assert!(
            !text.contains("``"),
            "code spans must not nest or collapse: {text}"
        );
    }

    #[test]
    fn longest_removed_name_wins_so_prefixes_cannot_shadow() {
        let rewritten = rewrite_removed_tool_references(
            "Call adashi_update_memory_rule then adashi_update_memory.",
        )
        .unwrap();
        assert_eq!(
            rewritten,
            "Call adashi_memory update_rule then adashi_memory update."
        );
    }

    #[test]
    fn capability_phrasing_is_left_alone() {
        assert_eq!(
            rewrite_removed_tool_references("Use the adashi_design get_scope operation."),
            None
        );
    }

    /// The defect this guards: a replacement that carried its own code span nested inside the
    /// span the removed name already sat in.
    #[test]
    fn rewritten_text_never_nests_code_spans() {
        let rewritten = rewrite_removed_tool_references(
            "Call `adashi_design_search`, `adashi_design_get_bindings(files)` and adashi_design_save.",
        )
        .unwrap();
        assert_balanced_code_spans(&rewritten);
        assert!(rewritten.contains("`adashi_design search`"));
        assert!(rewritten.contains("`adashi_design get_bindings(files)`"));
        assert!(rewritten.contains("adashi_design save"));
    }

    #[test]
    fn malformed_spans_from_the_earlier_release_are_repaired() {
        let stored = "Retrieval such as `the `adashi_design` operation `search`` and transactional \
                     `the `adashi_design` operation `save`` calls, plus a bare \
                     the `adashi_memory` operation `update` mention.";
        let repaired = rewrite_removed_tool_references(stored).unwrap();
        assert_balanced_code_spans(&repaired);
        assert!(repaired.contains("`adashi_design search`"));
        assert!(repaired.contains("`adashi_design save`"));
        assert!(repaired.contains("adashi_memory update"));
        assert!(!repaired.contains("operation `"));
    }

    #[test]
    fn repairing_malformed_text_is_idempotent() {
        let stored = "See `the `adashi_design` operation `search``.";
        let once = rewrite_removed_tool_references(stored).unwrap();
        assert_eq!(rewrite_removed_tool_references(&once), None);
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
        for (name, _, _) in REMOVED_TOOL_REFERENCES {
            let rewritten = rewrite_removed_tool_references(name).expect("removed name rewrites");
            assert!(
                !rewritten.contains(name),
                "{name} survived its own rewrite as {rewritten}"
            );
            assert!(unknown_tool_references(&rewritten).is_empty());
            assert_balanced_code_spans(&rewritten);
        }
    }

    #[test]
    fn every_removed_name_repairs_the_malformed_spans_it_once_produced() {
        for (name, tool, operation) in REMOVED_TOOL_REFERENCES {
            let malformed = format!("see `the `{tool}` operation `{operation}`` here");
            let repaired = rewrite_removed_tool_references(&malformed).unwrap_or_default();
            assert!(
                repaired.contains(&format!("`{tool} {operation}`")),
                "{name} did not repair its malformed form: {repaired}"
            );
            assert_balanced_code_spans(&repaired);
        }
    }

    /// Manual verification against this workspace's real project database: the stored prompts
    /// really do carry removed tool names and malformed spans, and the repair clears both.
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

        let needing_repair_before: i64 = db
            .query_row(
                "SELECT (SELECT COUNT(*) FROM fixed_hook_prompts
                          WHERE prompt LIKE '%adashi_design_%' OR prompt LIKE '%`the `%')
                      + (SELECT COUNT(*) FROM rules
                          WHERE prompt LIKE '%adashi_design_%' OR prompt LIKE '%`the `%')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        println!("prompts needing repair before: {needing_repair_before}");

        crate::fixed_hooks::ensure_fixed_hook_prompts(&db).unwrap();
        // A second pass proves the repair is idempotent on real data.
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
                    remaining.push(format!("{origin} -> unknown {unknown}"));
                }
                if prompt.contains("``") || prompt.matches('`').count() % 2 != 0 {
                    remaining.push(format!("{origin} -> malformed code spans"));
                }
                if prompt.contains("`the `") {
                    remaining.push(format!("{origin} -> un-repaired malformed phrase"));
                }
            }
        }

        println!("problems after repair: {remaining:?}");
        assert!(remaining.is_empty(), "{remaining:?}");
        assert!(
            needing_repair_before > 0,
            "fixture expected stored prompts to need repair"
        );

        let repaired_prompt: String = db
            .query_row(
                "SELECT prompt FROM fixed_hook_prompts WHERE key = 'design.run.start.authoring'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        println!("\n--- repaired design hook (excerpt) ---");
        for line in repaired_prompt.lines().filter(|line| line.contains("adashi_design")) {
            println!("{line}");
        }

        drop(db);
        let _ = std::fs::remove_dir_all(&root);
    }
}
