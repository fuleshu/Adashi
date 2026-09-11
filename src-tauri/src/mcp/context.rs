//! Lifecycle v2: one executable body with addressable, content-versioned sections.
use crate::{fixed_hooks, memory, rules, settings::ProjectSettings};
use rusqlite::{params, Connection};
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
fn content_version(body: &str) -> String {
    let hash = body.bytes().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    });
    format!("v2-fnv1a64-{hash:016x}")
}

pub fn build(
    db: &Connection,
    project_row_id: i64,
    project: ProjectSettings,
    intend: &str,
    hook: &str,
    memory_context: MemoryContext,
) -> Result<RuleInjectionResult, String> {
    let applicable = rules::load_rule_injections(db, intend, hook)?;
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
    let memory = memory::load_memory(db, project_row_id)?;
    // Required instructions never compete with the informational context budget.
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
            format!("Current summary omitted in full: exceeds the {SUMMARY_BUDGET}-character startup budget. Retrieve it with adashi_memory operation get before work that needs project constraints.")
        };
        let body = format!("# Project memory\n{summary}\nHistorical handovers are not current state and are not injected. Use adashi_memory operation get with query, runId or taskId when relevant; superseded notes require includeSuperseded=true.");
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
        if let Some(prompt) = fixed_hooks::load_fixed_hook_prompts(db, project_row_id)?
            .into_iter()
            .find(|p| p.key == key)
        {
            result.push(key, "fixedPrompt", Some(prompt.version), &prompt.prompt);
        }
        result.push(
            "design.index",
            "designIndex",
            None,
            &design_index(db, project_row_id)?,
        );
    }
    Ok(result)
}

/// Metadata-only SQL projection; never hydrates descriptions, DSL, relationships or artifacts.
fn design_index(db: &Connection, project_id: i64) -> Result<String, String> {
    let total: i64 = db.query_row(
        "SELECT COUNT(*) FROM c4_elements e JOIN design_workspaces w ON w.id=e.workspace_id WHERE w.project_id=?1",
        [project_id], |row| row.get(0),
    ).map_err(|e| e.to_string())?;
    let mut output = String::from("# Formal design index\nRetrieve relevant guidance with adashi_design operations get_bindings(files/symbols), get_scope(elementId) or get_by_ids(ids). Use the search operation(query) for entries absent here. UML types: class, sequence, flow, state; UI mockups are separate.\n");
    let footer = "\nIndex only: descriptions, relationships, artifacts, bindings and source require explicit retrieval.";
    let mut statement = db.prepare(
        "SELECT e.external_id, e.parent_external_id, e.element_type, e.name, COALESCE(rv.version,0)
         FROM c4_elements e JOIN design_workspaces w ON w.id=e.workspace_id
         LEFT JOIN resource_versions rv ON rv.project_id=w.project_id AND rv.resource_kind='design.element' AND rv.resource_id=e.external_id
         WHERE w.project_id=?1 ORDER BY e.parent_external_id IS NOT NULL,
         CASE e.element_type WHEN 'Software System' THEN 0 WHEN 'Container' THEN 1 ELSE 2 END, e.id LIMIT 32"
    ).map_err(|e| e.to_string())?;
    let rows = statement
        .query_map(params![project_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut included = 0;
    for row in rows {
        let (id, parent, kind, name, version) = row.map_err(|e| e.to_string())?;
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
