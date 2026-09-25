//! Content preconditions over resource aggregates, independent of branch counters.
use super::*;
use api::{coordination::ResourceVersion, ChangeOutcome, CommitResult, PreparedMutation};
use rusqlite::{params, Connection};

pub(super) struct Versions {
    pub exposed: Vec<ResourceVersion>,
    pub raw_by_token: BTreeMap<i64, i64>,
    hashes: BTreeMap<i64, String>,
}

fn text(data: &engine::Data, key: &str) -> String {
    match data.get(key) {
        Some(Value::String(v)) => v.clone(),
        Some(v) => v.to_string(),
        None => String::new(),
    }
}

pub(super) fn calculate(rows: &engine::Rows) -> StorageResult<Versions> {
    let mut groups = BTreeMap::<(String, String), Vec<Value>>::new();
    let markdown=rows.iter().filter(|((t,_),_)|t=="markdown_design_documents").map(|(_,d)|(text(d,"id"),text(d,"external_id"))).collect::<BTreeMap<_,_>>();
    let mockups = rows
        .iter()
        .filter(|((t, _), _)| t == "ui_mockups")
        .map(|(_, d)| (text(d, "id"), text(d, "external_id")))
        .collect::<BTreeMap<_, _>>();
    for ((table, _), data) in rows {
        let mut owners = Vec::<(&str, String)>::new();
        match table.as_str() {
            "markdown_design_documents" => owners.push(("design.markdown", text(data,"external_id"))),
            "markdown_design_links" => {
                if let Some(id)=markdown.get(&text(data,"document_id")) { owners.push(("design.markdown",id.clone())); }
            }
            "c4_elements" => owners.push(("design.element", text(data, "external_id"))),
            "c4_relationships" => owners.push(("design.relationship", text(data, "external_id"))),
            "diagrams" if data["kind"] == "mermaid" => {
                owners.push(("design.uml", text(data, "key")))
            }
            "design_bindings" => owners.push((
                "design.binding",
                format!(
                    "{}|{}|{}",
                    text(data, "design_external_id"),
                    text(data, "target_type"),
                    text(data, "target")
                ),
            )),
            "agent_tasks" => owners.push(("task", text(data, "id"))),
            "task_design_specification_links" => owners.push(("task", text(data, "task_id"))),
            "qa_jobs" => owners.push(("qa.job", text(data, "id"))),
            "qa_job_design_links" | "qa_job_task_links" | "qa_job_tags" => {
                owners.push(("qa.job", text(data, "qa_job_id")))
            }
            "qa_runs" => owners.push(("qa.run", text(data, "id"))),
            "qa_job_runs" => {
                owners.push(("qa.job-run", text(data, "id")));
                owners.push(("qa.run", text(data, "qa_run_id")));
            }
            "rules" => owners.push(("rule", text(data, "id"))),
            "fixed_hook_prompts" => owners.push(("fixed-hook", text(data, "key"))),
            "ui_mockups" => {
                let id = text(data, "external_id");
                let accepted = data
                    .iter()
                    .filter(|(k, _)| {
                        !["working_svg", "base_revision", "status", "updated_at"]
                            .contains(&k.as_str())
                    })
                    .collect::<BTreeMap<_, _>>();
                groups
                    .entry(("mockup.accepted".into(), id.clone()))
                    .or_default()
                    .push(serde_json::json!([table, accepted]));
                owners.push(("mockup.working", id));
            }
            "ui_mockup_edit_operations" | "ui_mockup_annotations" | "ui_mockup_proposals" => {
                if let Some(id) = mockups.get(&text(data, "mockup_id")) {
                    owners.push(("mockup.working", id.clone()));
                }
            }
            "project_memory" => {
                groups
                    .entry(("memory.protocol".into(), "protocol".into()))
                    .or_default()
                    .push(data["protocol_rule"].clone());
                groups
                    .entry(("memory.canonical".into(), "canonical".into()))
                    .or_default()
                    .push(data["memory_body"].clone());
            }
            "project_memory_notes" => {
                owners.push(("memory.note", text(data, "note_id")));
                owners.push(("memory.canonical", "canonical".into()));
            }
            "project_memory_note_resolutions" => {
                owners.push(("memory.canonical", "canonical".into()))
            }
            "design_health_waivers" => owners.push(("health.waiver", text(data, "id"))),
            "coding_guidelines" | "post_task_commands" | "qa_checks" | "task_qa_entries" => {
                owners.push(("legacy", "content".into()))
            }
            "project_computers" => owners.push(("computer", text(data, "computer_id"))),
            "resource_versions" => owners.push((
                data["resource_kind"].as_str().unwrap(),
                text(data, "resource_id"),
            )),
            _ => {}
        }
        for (kind, id) in owners {
            groups
                .entry((kind.into(), id))
                .or_default()
                .push(serde_json::json!([table, data]));
        }
    }
    let mut exposed = Vec::new();
    let mut raw_by_token = BTreeMap::new();
    let mut hashes = BTreeMap::new();
    for ((table, _), data) in rows.iter().filter(|((t, _), _)| t == "resource_versions") {
        let key = (text(data, "resource_kind"), text(data, "resource_id"));
        let hash = digest(&codec::bytes(&serde_json::json!([key, groups.get(&key)]))?);
        let version = numeric_digest(&hash);
        if hashes
            .insert(version, hash.clone())
            .is_some_and(|prior| prior != hash)
        {
            return Err(invalid(table,"ambiguous content-version fingerprint; use full document tokens or repair the collision before writing"));
        }
        raw_by_token.insert(version, data["version"].as_i64().unwrap());
        exposed.push(ResourceVersion {
            resource_kind: key.0,
            resource_id: key.1,
            version,
        });
    }
    let known = exposed
        .iter()
        .map(|v| (&v.resource_kind, &v.resource_id))
        .collect::<BTreeSet<_>>();
    for (kind, id) in groups.keys() {
        if kind != "computer" && !known.contains(&(kind, id)) {
            return Err(invalid(
                &format!("{kind}:{id}"),
                "missing resource version/provenance record",
            ));
        }
    }
    Ok(Versions {
        exposed,
        raw_by_token,
        hashes,
    })
}

/// Retain observed full hashes locally so revisiting a branch cannot alias an
/// earlier integer guard even in the exceptional truncated-hash collision case.
pub(super) fn remember(local: &Path, versions: &Versions) -> StorageResult<()> {
    let path = local.join("content-versions.json");
    let old = journal::read_optional(&path)?;
    let mut known: BTreeMap<i64, String> = old
        .as_ref()
        .map(|b| codec::parse(b, "local/content-versions.json"))
        .transpose()?
        .unwrap_or_default();
    let mut changed = false;
    for (token, hash) in &versions.hashes {
        match known.get(token) {
            Some(prior) if prior!=hash=>return Err(invalid("content version","ambiguous integer fingerprint for different full hashes; full content does not match")),
            Some(_)=>{},None=>{known.insert(*token,hash.clone());changed=true;},
        }
    }
    if changed {
        journal::atomic_write(&path, &codec::bytes(&known)?)?;
    }
    Ok(())
}

pub(super) fn install(db: &Connection, project: i64, versions: &Versions) -> StorageResult<()> {
    for v in &versions.exposed {
        db.execute("UPDATE resource_versions SET version=?1 WHERE project_id=?2 AND resource_kind=?3 AND resource_id=?4",params![v.version,project,v.resource_kind,v.resource_id]).map_err(StorageError::backend)?;
    }
    Ok(())
}

pub(super) fn translate(
    prepared: &PreparedMutation,
    versions: &Versions,
) -> StorageResult<PreparedMutation> {
    api::check_versions(&prepared.expected_versions(), &versions.exposed)?;
    fn replace(value: &mut Value, map: &BTreeMap<i64, i64>) -> StorageResult<()> {
        match value {
            Value::Object(object) => {
                for (k, v) in object {
                    if k.starts_with("expected")
                        && (k.ends_with("Version") || k.ends_with("_version"))
                    {
                        let token = v
                            .as_i64()
                            .ok_or_else(|| invalid("mutation", "invalid version"))?;
                        if token != 0 {
                            *v = (*map.get(&token).ok_or_else(|| {
                                invalid("mutation", "unknown content version; reload the resource")
                            })?)
                            .into();
                        }
                    } else {
                        replace(v, map)?;
                    }
                }
            }
            Value::Array(items) => {
                for v in items {
                    replace(v, map)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    let mut mutation = serde_json::to_value(prepared.mutation()).map_err(StorageError::backend)?;
    replace(&mut mutation, &versions.raw_by_token)?;
    api::prepare_mutation(serde_json::from_value(mutation).map_err(StorageError::backend)?)
}

pub(super) fn result(
    result: &mut CommitResult,
    versions: &Versions,
    cursor: api::ChangeCursor,
    revision: i64,
) {
    let token = |kind: &str, id: String| {
        versions
            .exposed
            .iter()
            .find(|v| v.resource_kind == kind && v.resource_id == id)
            .map(|v| v.version)
            .unwrap_or(0)
    };
    for v in &mut result.versions {
        v.version = token(&v.resource_kind, v.resource_id.clone());
    }
    for out in &mut result.outcomes {
        match out {
            ChangeOutcome::Task(Some(t)) => t.version = token("task", t.id.to_string()),
            ChangeOutcome::Rule(Some(r)) => r.version = token("rule", r.id.to_string()),
            ChangeOutcome::QaJob(Some(j)) => j.version = token("qa.job", j.id.to_string()),
            ChangeOutcome::FixedPrompt(p) => p.version = token("fixed-hook", p.key.clone()),
            ChangeOutcome::Memory(m) => {
                m.memory_version = token("memory.canonical", "canonical".into());
                m.protocol_version = token("memory.protocol", "protocol".into());
            }
            ChangeOutcome::Mockup(Some(m)) => {
                m.accepted_version = token("mockup.accepted", m.external_id.clone());
                m.working_version = token("mockup.working", m.external_id.clone());
            }
            ChangeOutcome::Design(d) => d.revision = revision,
            _ => {}
        }
    }
    result.cursor = cursor;
    result.revision = revision;
}
