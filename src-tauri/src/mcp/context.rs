//! Lifecycle v2: one executable body with addressable, content-versioned sections.
use crate::{fixed_hooks, rules, settings::ProjectSettings};
use adashi_storage_api::ReadSnapshot;
use serde::{Deserialize, Serialize};

pub const SUMMARY_BUDGET: usize = 2_000;
pub const DESIGN_INDEX_BUDGET: usize = 3_000;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum MemoryContext {
    #[default]
    Summary,
    ProtocolOnly,
}

#[derive(Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuleMetadata {
    id: i64,
    version: i64,
}

#[derive(Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Section {
    id: String,
    kind: String,
    /// Resource version where one exists. contentVersion identifies the exact projection.
    version: Option<i64>,
    content_version: String,
    /// UTF-8 byte offsets in injectionPrompt; end is exclusive.
    start_byte: u32,
    end_byte: u32,
}

#[derive(Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuleInjectionResult {
    contract_version: u32,
    project_id: String,
    project_name: String,
    intend: String,
    hook: String,
    status: String,
    rules: Vec<RuleMetadata>,
    sections: Vec<Section>,
    injection_prompt: String,
}

impl RuleInjectionResult {
    fn push(&mut self, id: &str, kind: &str, version: Option<i64>, body: &str) {
        let body = body.trim();
        if body.is_empty() {
            return;
        }
        if !self.injection_prompt.is_empty() {
            self.injection_prompt.push_str("\n\n");
        }
        let start = self.injection_prompt.len();
        self.injection_prompt.push_str(body);
        self.sections.push(Section {
            id: id.into(),
            kind: kind.into(),
            version,
            content_version: content_version(body),
            start_byte: start as u32,
            end_byte: self.injection_prompt.len() as u32,
        });
        self.status = "apply".into();
    }
}

/// Stable across processes/platforms; an exact-content cache key, not a security hash.
pub(super) fn content_version(body: &str) -> String {
    let hash = body.bytes().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    });
    format!("v2-fnv1a64-{hash:016x}")
}

pub fn build(
    db: &dyn ReadSnapshot,
    project: ProjectSettings,
    intend: &str,
    hook: &str,
    memory_context: MemoryContext,
) -> Result<RuleInjectionResult, String> {
    rules::validate_intend(intend)?;
    rules::validate_hook(hook)?;
    let applicable = db
        .rules()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|rule| rule.enabled && rule.intend == intend && rule.hook == hook);
    let mut result = RuleInjectionResult {
        contract_version: 2,
        project_id: project.id,
        project_name: project.name,
        intend: intend.into(),
        hook: hook.into(),
        status: "empty".into(),
        rules: Vec::new(),
        sections: Vec::new(),
        injection_prompt: String::new(),
    };
    for rule in applicable {
        result.push(
            &format!("rule:{}", rule.id),
            "rule",
            Some(rule.version),
            &rule.prompt,
        );
        result.rules.push(RuleMetadata {
            id: rule.id,
            version: rule.version,
        });
    }
    if hook != "run.start" {
        return Ok(result);
    }
    let memory = db.memory().map_err(|e| e.to_string())?;
    // Custom project instructions never compete with the informational context budget.
    result.push(
        "memory.protocol",
        "protocol",
        Some(memory.protocol_version),
        &memory.rule,
    );
    if matches!(memory_context, MemoryContext::Summary) {
        let summary = if memory.memory.trim().is_empty() {
            "No current summary is recorded.".to_string()
        } else if memory.memory.chars().count() <= SUMMARY_BUDGET {
            format!("Current summary:\n{}", memory.memory.trim())
        } else {
            format!("Current summary omitted in full: exceeds the {SUMMARY_BUDGET}-character startup budget.")
        };
        let body = format!("# Project memory\n{summary}");
        result.push(
            "memory.summary",
            "memory",
            Some(memory.memory_version),
            &body,
        );
    }
    if matches!(intend, "design" | "implementation") {
        let key = if intend == "design" {
            fixed_hooks::DESIGN_AUTHORING_HOOK_KEY
        } else {
            fixed_hooks::IMPLEMENTATION_GUIDANCE_HOOK_KEY
        };
        if let Some(prompt) = db
            .fixed_prompts()
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|p| p.key == key)
        {
            result.push(key, "fixedPrompt", Some(prompt.version), &prompt.prompt);
        }
        result.push("design.index", "designIndex", None, &design_index(db)?);
    }
    Ok(result)
}

/// Metadata-only SQL projection; never hydrates descriptions, DSL, relationships or artifacts.
fn design_index(db: &dyn ReadSnapshot) -> Result<String, String> {
    let mut elements = db.design_inventory().map_err(|e| e.to_string())?.elements;
    let total = elements.len();
    elements.sort_by_key(|e| {
        (
            e.value.parent_external_id.is_some(),
            match e.value.element_type.as_str() {
                "Software System" => 0,
                "Container" => 1,
                _ => 2,
            },
            e.id,
        )
    });
    let mut output = String::from("# Formal design index\n");
    let footer = "\nIndex only: descriptions, relationships, artifacts, bindings and source require explicit retrieval.";
    let rows = elements.into_iter().take(32);
    let mut included = 0;
    for row in rows {
        let e = row.value;
        let (id, parent, kind, name, version) = (
            e.external_id,
            e.parent_external_id,
            e.element_type,
            e.name,
            e.version,
        );
        // JSON quoting preserves exact ids and prevents embedded newlines becoming index entries.
        let line = format!(
            "\n- {} {} {} parent={} v{}",
            serde_json::to_string(&id).unwrap(),
            kind,
            serde_json::to_string(&name).unwrap(),
            serde_json::to_string(&parent).unwrap(),
            version
        );
        if output.len() + line.len() + footer.len() + 100 > DESIGN_INDEX_BUDGET {
            continue;
        }
        output.push_str(&line);
        included += 1;
    }
    output.push_str(&format!("\nShowing {included} of {total} elements."));
    output.push_str(footer);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    #[test]
    fn startup_contains_project_context_and_custom_instructions_without_builtin_manuals() {
        let mut db = Connection::open_in_memory().unwrap();
        crate::schema::migrate(&mut db).unwrap();
        let project = || ProjectSettings {
            id: "p".into(),
            name: "P".into(),
            folder: "unused".into(),
        };
        crate::seed::seed_initial_data(&mut db, &project()).unwrap();
        crate::fixed_hooks::ensure_fixed_hook_prompts(&db).unwrap();
        for intent in ["general", "design", "implementation"] {
            let result = build(
                crate::storage::sqlite::test_snapshot(&db, 1).as_ref(),
                project(),
                intent,
                "run.start",
                MemoryContext::Summary,
            )
            .unwrap();
            assert!(result
                .sections
                .iter()
                .all(|s| !matches!(s.kind.as_str(), "protocol" | "fixedPrompt")));
            assert!(!result.injection_prompt.contains("expectedRevision"));
            assert!(!result.injection_prompt.contains("adashi_"));
        }
        let empty = build(
            crate::storage::sqlite::test_snapshot(&db, 1).as_ref(),
            project(),
            "general",
            "run.start",
            MemoryContext::ProtocolOnly,
        )
        .unwrap();
        assert_eq!(empty.status, "empty");
        db.execute("UPDATE project_memory SET protocol_rule='Project-specific memory rule.',memory_body='Current project constraint.'", []).unwrap();
        fixed_hooks::update_fixed_hook_prompt(
            &db,
            1,
            fixed_hooks::DESIGN_AUTHORING_HOOK_KEY.into(),
            "Project-specific design constraint.".into(),
        )
        .unwrap();
        db.execute("INSERT INTO rules(project_id,name,enabled,intend,hook,prompt) VALUES(1,'Custom',1,'design','run.start','Project-specific lifecycle rule.')", []).unwrap();
        let result = build(
            crate::storage::sqlite::test_snapshot(&db, 1).as_ref(),
            project(),
            "design",
            "run.start",
            MemoryContext::Summary,
        )
        .unwrap();
        for body in [
            "Project-specific memory rule.",
            "Current project constraint.",
            "Project-specific design constraint.",
            "Project-specific lifecycle rule.",
        ] {
            assert_eq!(result.injection_prompt.matches(body).count(), 1);
        }
        assert_eq!(result.sections.len(), 5);
    }

    #[test]
    fn sections_address_one_exact_unicode_body_and_stable_versions() {
        let mut result = RuleInjectionResult {
            contract_version: 2,
            project_id: "p".into(),
            project_name: "P".into(),
            intend: "general".into(),
            hook: "task.start".into(),
            status: "empty".into(),
            rules: vec![],
            sections: vec![],
            injection_prompt: String::new(),
        };
        result.push("a", "rule", Some(1), "Required 🦀 instruction.");
        result.push("b", "memory", Some(1), "Unique memory.");
        for section in &result.sections {
            let body =
                &result.injection_prompt[section.start_byte as usize..section.end_byte as usize];
            assert_eq!(section.content_version, content_version(body));
        }
        let value = serde_json::to_string(&result).unwrap();
        assert_eq!(value.matches("Unique memory.").count(), 1);
        assert_ne!(content_version("a"), content_version("b"));
    }
}
