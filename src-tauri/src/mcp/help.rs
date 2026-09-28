//! On-demand contracts, shared with error recovery. Reading help never opens a project.
use super::*;
use rmcp::model::Tool;
use std::collections::BTreeSet;

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct HelpParams {
    /// Exact tool name. Omit operation to list its operations without loading their schemas.
    /// Omit both tool and skill for the tool and skill catalog.
    #[serde(default)]
    #[schemars(schema_with = "optional_tool_name_schema")]
    pub tool: Option<String>,
    /// Select one operation. adashi_grep has no operation; omit it for grep help.
    pub operation: Option<String>,
    /// Only for adashi_design/save: return just these changes[].op variants, e.g. ["upsert_uml"]. Omit for all variants.
    pub change_types: Option<Vec<String>>,
    /// Read one on-demand skill (for example design-authoring) instead of an operation contract.
    #[serde(default)]
    #[schemars(schema_with = "skill_name_schema")]
    pub skill: Option<String>,
}

fn tool_alias_schema(names: &[&str]) -> rmcp::schemars::Schema {
    let mut values = names.iter().map(|name| json!(name)).collect::<Vec<Value>>();
    values.push(Value::Null);
    rmcp::schemars::json_schema!({"type":["string","null"], "enum":values})
}

fn optional_tool_name_schema(_: &mut rmcp::schemars::SchemaGenerator) -> rmcp::schemars::Schema {
    tool_alias_schema(crate::prompt_hygiene::MCP_TOOL_NAMES)
}

fn skill_name_schema(_: &mut rmcp::schemars::SchemaGenerator) -> rmcp::schemars::Schema {
    tool_alias_schema(&crate::skills::names())
}

pub(super) fn operations(tool: &Tool) -> Vec<String> {
    let schema = json!(tool.input_schema);
    let property = &schema["properties"]["operation"];
    let definition = property
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|reference| schema.pointer(&reference[1..]))
        .unwrap_or(property);
    definition["enum"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn invalid(message: String, details: Value) -> ErrorData {
    ErrorData::invalid_params(message, Some(details))
}

pub(super) fn get(params: HelpParams) -> Result<CallToolResult, ErrorData> {
    if params.tool.is_some() && params.skill.is_some() {
        return Err(invalid(
            "Pass either tool or skill, not both.".into(),
            json!({
                "availableTools":crate::prompt_hygiene::MCP_TOOL_NAMES,
                "availableSkills":crate::skills::names(),
            }),
        ));
    }
    if let Some(skill) = params.skill.as_deref() {
        return skill_help(skill);
    }
    let Some(tool_name) = params.tool.clone() else {
        return Ok(CallToolResult::structured(json!({
            "tools":crate::prompt_hygiene::MCP_TOOL_NAMES,
            "skills":crate::skills::catalog(),
            "next":"Call adashi_help with tool and operation for one exact contract, or with skill for the on-demand workflow that matches the work."
        })));
    };
    let router = AdashiMcpServer::tool_router();
    let tool = router.get(&tool_name).ok_or_else(|| {
        invalid(
            format!("Unknown tool '{tool_name}'. Choose one of availableTools."),
            json!({"availableTools":crate::prompt_hygiene::MCP_TOOL_NAMES}),
        )
    })?;
    let available = operations(tool);
    let operation = params.operation.as_deref();
    if let Some(operation) = operation {
        if !available.iter().any(|candidate| candidate == operation) {
            return Err(invalid(
                format!(
                    "Unknown operation '{operation}' for {tool_name}. {}",
                    if available.is_empty() {
                        "Omit operation for this tool."
                    } else {
                        "Choose one of availableOperations."
                    }
                ),
                json!({"availableOperations":available}),
            ));
        }
    }
    if params.change_types.is_some()
        && !(tool_name == "adashi_design" && operation == Some("save"))
    {
        return Err(invalid(
            "changeTypes is only supported for tool=adashi_design, operation=save.".into(),
            json!({"exampleArguments":{"tool":"adashi_design","operation":"save","changeTypes":["upsert_uml"]}}),
        ));
    }
    if operation.is_none() && !available.is_empty() {
        return Ok(CallToolResult::structured(json!({
            "tool":tool_name, "operations":available,
            "next":"Call adashi_help with tool and one operation for its exact schema, examples and workflow."
        })));
    }
    let arguments = operation
        .map(|op| json!({"operation":op}))
        .unwrap_or_else(|| json!({}));
    let mut schema = errors::contract(tool, arguments.as_object().unwrap());
    let mut change_examples = Vec::new();
    if tool_name == "adashi_design" && operation == Some("save") {
        // The variant definition is generated from DesignChange, never a second handwritten schema.
        let variants = schema["$defs"]["DesignChange"]["oneOf"]
            .as_array_mut()
            .expect("DesignChange is a tagged enum");
        let allowed: Vec<String> = variants
            .iter()
            .map(|v| variant_name(v).to_owned())
            .collect();
        if let Some(selected) = &params.change_types {
            if selected.is_empty() || selected.iter().any(|name| !allowed.contains(name)) {
                return Err(invalid(
                    "changeTypes must be a nonempty subset of availableChangeTypes.".into(),
                    json!({"availableChangeTypes":allowed}),
                ));
            }
            variants.retain(|v| selected.iter().any(|name| name == variant_name(v)));
        }
        for variant in variants.iter() {
            change_examples.push(change_example(variant_name(variant)));
        }
        prune_definitions(&mut schema);
    }
    let operation = operation.unwrap_or("");
    let mut example = example(&tool_name, operation);
    if let Some(change) = change_examples.first() {
        example["operationId"] = json!("design-change-001");
        example["changeIntent"] = json!("Apply the requested formal design changes");
        example["changes"] = json!([change]);
        if change["op"]
            .as_str()
            .is_some_and(|op| op.starts_with("delete_") || op == "upsert_mockup_proposal")
        {
            example["readTokens"] = json!([{"documentId":"<copy documentId from the complete read>","readToken":"<copy readToken from the same read>"}]);
        }
    }
    let required = schema.get("required").cloned().unwrap_or_else(|| json!([]));
    let mut result = json!({
        "tool":tool_name, "operation":params.operation,
        "requiredParameters":required,
        "parameterSchema":schema,
        "exampleArguments":example,
        "exampleNote":"Examples are templates, not commands to execute. Replace project names, ids, content, versions and tokens with values for your intended action. Copy opaque ids/tokens from reads; do not invent them.",
        "workflow":workflow(&tool_name, operation),
        "cache":"Reuse this selected contract while it remains in active context. Fetch it again after losing it or reconnecting to a different server version. Help is never suppressed based on an earlier call."
    });
    if !change_examples.is_empty() {
        result["changeExamples"] = json!(change_examples);
        result["schemaScope"] = json!(if params.change_types.is_some() {
            "Only requested changeTypes and their referenced definitions."
        } else {
            "All change types."
        });
    }
    if tool_name == "adashi_design" {
        if let Some(schema) = markdown::response_schema(operation) { result["responseSchema"] = schema; }
    }
    if tool_name == "adashi_rules" && operation == "get_rule_injections" {
        result["agentWorkflow"] = json!(crate::projection::AGENT_WORKFLOW);
        result["skills"] = crate::skills::catalog();
    }
    result["contentVersion"] = json!(context::content_version(&result.to_string()));
    Ok(CallToolResult::structured(result))
}

/// One on-demand skill, fetched without a project so it works before setup.
fn skill_help(name: &str) -> Result<CallToolResult, ErrorData> {
    let skill = crate::skills::get(name).ok_or_else(|| {
        invalid(
            format!("Unknown skill '{name}'. Choose one of availableSkills."),
            json!({
                "availableSkills":crate::skills::names(),
                "skills":crate::skills::catalog(),
            }),
        )
    })?;
    Ok(CallToolResult::structured(json!({
        "skill":skill.name,
        "title":skill.title,
        "when":skill.when,
        "body":skill.body,
        "contentVersion":context::content_version(skill.body),
        "cache":"Read once when the work matches. Fetch it again after context loss or a server upgrade; skills are never suppressed based on an earlier call."
    })))
}

fn variant_name(variant: &Value) -> &str {
    variant["properties"]["op"]["const"]
        .as_str()
        .expect("tagged DesignChange variant")
}

/// Retain the transitive closure of local refs so filtered help cannot contain dangling refs
/// or pay the context cost of unrelated mockup definitions.
fn prune_definitions(schema: &mut Value) {
    fn refs(value: &Value, into: &mut BTreeSet<String>) {
        match value {
            Value::Object(object) => {
                if let Some(reference) = object
                    .get("$ref")
                    .and_then(Value::as_str)
                    .and_then(|s| s.strip_prefix("#/$defs/"))
                {
                    into.insert(reference.to_owned());
                }
                for child in object.values() {
                    refs(child, into);
                }
            }
            Value::Array(array) => {
                for child in array {
                    refs(child, into);
                }
            }
            _ => {}
        }
    }
    let Some(definitions) = schema.as_object_mut().unwrap().remove("$defs") else {
        return;
    };
    let mut reachable = BTreeSet::new();
    refs(schema, &mut reachable);
    loop {
        let previous = reachable.clone();
        for name in &previous {
            refs(&definitions[name], &mut reachable);
        }
        if previous == reachable {
            break;
        }
    }
    schema["$defs"] = Value::Object(
        definitions
            .as_object()
            .unwrap()
            .iter()
            .filter(|(name, _)| reachable.contains(*name))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
    );
}

fn example(tool: &str, operation: &str) -> Value {
    if let Some(example) = errors::example(tool, operation) {
        return example;
    }
    let fields = match (tool, operation) {
        ("adashi_help", _) => return json!({"tool":"adashi_qa","operation":"create_job"}),
        ("adashi_grep", _) => {
            return json!({"projectName":"Your project","pattern":"concurrency in:design"})
        }
        ("adashi_design", "get_scope") => json!({"elementId":"element-id"}),
        ("adashi_design", "list_markdown") => json!({"markdownQuery":{"limit":25}}),
        ("adashi_design", "get_by_ids") => json!({"ids":["element-id"]}),
        ("adashi_design", "get_documents") => json!({"ids":["<documentId from a complete read>"]}),
        ("adashi_design", "search") => json!({"query":"authentication"}),
        ("adashi_design", "get_bindings") => json!({"files":["src/main.rs"]}),
        ("adashi_design", "mockup_get_revision_context") => json!({"externalId":"mockup-id"}),
        ("adashi_tasks", "get") => json!({"taskNumber":5}),
        ("adashi_tasks", "close" | "delete") => {
            json!({"operationId":"task-mutation-001","taskId":1,"expectedVersion":1})
        }
        ("adashi_qa", "get_job") => json!({"qaJobId":1}),
        ("adashi_qa", "get_run") => json!({"qaRunId":1}),
        ("adashi_qa", "cancel_run") => json!({"qaRunId":1}),
        ("adashi_qa", "update_job") => {
            json!({"operationId":"qa-update-001","qaJobId":1,"expectedVersion":1,"name":"Updated test job"})
        }
        ("adashi_qa", "delete_job") => {
            json!({"operationId":"qa-delete-001","qaJobId":1,"expectedVersion":1})
        }
        ("adashi_memory", "append") => {
            json!({"noteId":"handover-auth-001","operationId":"memory-append-001","runId":"run-auth-001","body":"A durable decision and its reason; next step if unresolved."})
        }
        ("adashi_memory", "update") => {
            json!({"operationId":"memory-summary-001","expectedVersion":1,"memory":"Current reviewed project constraints","supersededNoteIds":[]})
        }
        ("adashi_memory", "update_rule") => {
            json!({"operationId":"memory-rule-001","expectedVersion":1,"rule":"Record unresolved authentication migration constraints."})
        }
        ("adashi_rules", "get_rule_injections") => {
            json!({"intend":"implementation","hook":"run.start"})
        }
        ("adashi_rules", "create" | "update") => {
            let mut fields = json!({"operationId":"project-rule-001","name":"Project validation","enabled":true,"intend":"implementation","hook":"task.end","prompt":"Run this project's relevant validation before reporting completion."});
            if operation == "update" {
                fields["expectedVersion"] = json!(1);
                fields["ruleId"] = json!(1);
            }
            fields
        }
        ("adashi_rules", "delete") => {
            json!({"operationId":"rule-delete-001","ruleId":1,"expectedVersion":1})
        }
        ("adashi_intents", "publish") => {
            json!({"agentRunId":"run-auth-001","resourceKind":"design.element","resourceId":"element-id","ttlSeconds":300})
        }
        _ => json!({}),
    };
    let mut result = json!({"projectName":"Your project","operation":operation});
    result
        .as_object_mut()
        .unwrap()
        .extend(fields.as_object().unwrap().clone());
    result
}

fn change_example(operation: &str) -> Value {
    let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100\" height=\"60\" viewBox=\"0 0 100 60\"><rect data-adashi-id=\"background\" width=\"100\" height=\"60\" fill=\"white\"/></svg>";
    let mut fields = match operation {
        "upsert_markdown" => {
            json!({"externalId":"decision-id","title":"Design decision","body":"# Decision\n\nRationale.","designLinks":[]})
        }
        "upsert_element" => {
            json!({"externalId":"new-container","parentExternalId":"system-id","elementType":"Container","name":"New container"})
        }
        "upsert_relationship" => {
            json!({"externalId":"new-link","sourceExternalId":"source-id","destinationExternalId":"destination-id","description":"Calls"})
        }
        "upsert_uml" => {
            json!({"key":"new-flow","title":"Request flow","language":"mermaid","diagramType":"flow","attachedToExternalId":"element-id","source":"flowchart TD\n  Request --> Response"})
        }
        "upsert_binding" | "delete_binding" => {
            json!({"designExternalId":"element-id","targetType":"file","target":"src/main.rs"})
        }
        "upsert_mockup" => {
            json!({"externalId":"new-screen","title":"Example screen","attachedToExternalId":"element-id","viewportWidth":100,"viewportHeight":60,"screen":"Home","mockupState":"default","fidelity":"wireframe","acceptedSvg":svg})
        }
        "upsert_mockup_proposal" => {
            json!({"externalId":"mockup-id","baseRevision":1,"proposedSvg":svg,"proposedManifest":{"schemaVersion":1,"key":"mockup-id","attachedToExternalId":"element-id","viewportWidth":100,"viewportHeight":60,"screen":"Home","state":"default","fidelity":"wireframe"}})
        }
        "delete_uml" => json!({"key":"diagram-key"}),
        "delete_element" | "delete_relationship" | "delete_mockup" | "delete_markdown" => {
            json!({"externalId":"existing-id"})
        }
        _ => unreachable!("DesignChange example missing: {operation}"),
    };
    fields["op"] = json!(operation);
    fields
}

fn workflow(tool: &str, operation: &str) -> Vec<&'static str> {
    let mut notes = vec!["parameterSchema lists the selected operation's required and optional fields, types, enums and nested definitions. Omit fields belonging to other operations. Omitted optional values use server defaults; do not supply guessed values."];
    if tool != "adashi_design" && matches!(
        operation,
        "save"
            | "set_element_descriptions"
            | "create"
            | "update"
            | "finish"
            | "close"
            | "delete"
            | "create_job"
            | "update_job"
            | "delete_job"
            | "run_jobs"
            | "append"
            | "update_rule"
    ) {
        notes.push("Use a unique operationId per mutation; reuse it only for an identical retry. For expectedVersion, copy the resource's version from its read response, never the project revision. Reread and reconcile a stale resource before retrying with a new operationId.");
    }
    match tool {
        "adashi_design" => {
            notes.push("Overview, list_markdown, search and the startup index are navigation only. get_by_ids/get_scope/get_bindings return complete editable documents with documentId/readToken; get_documents takes those documentIds, including markdown:<externalId>. A Markdown scope needs no C4 parent. A C4 scope includes ancestors by default; childrenDepth omitted is unlimited and includeSource defaults false. Search kinds are element, relationship, uml, source, mockup, markdown; limit defaults 20 (maximum 100). Markdown metadata pages include totalCount and nextAfterId; use list_markdown with markdownQuery.afterId to continue. Full bodies are never truncated in documents entries.");
            notes.push("Official Markdown designs are canonical storage artefacts. Agents author them only with upsert_markdown/delete_markdown through this API. Supply externalId, title, exact body and ordered designLinks (empty for project-level prose); targetType is element, relationship, uml, mockup or markdown. Titles and generated paths are not identity. Referenced documents cannot be deleted until guarded dependencies are explicitly unlinked. Generated Markdown is discovery output, never an editable input.");
            if operation == "set_element_descriptions" {
                notes.push("This narrow operation requires projectName, operation, operationId, readTokens and updates. Each update contains externalId and a nonempty description; it preserves other element fields. Read the complete elements first and copy their documentId/readToken pairs. Use a unique operationId; reuse it only for an identical retry. On out_of_date nothing is saved: merge the intended descriptions with currentDocument, then retry using the new tokens and a new operationId. Do not send changes, changeIntent, expectedRevision or guard to this operation.");
            }
            if operation == "save" {
                notes.push(crate::design::documents::WRITE_PROTOCOL);
                notes.push("Each changes item has an op tag. Upserts replace whole documents: preserve fields you intend to keep. C4 elementType is Person, Software System, Container or Component. Container requires a Software System parent; Component requires a Container parent. Relationship endpoints, diagram attachments and each binding's designExternalId must exist in the resulting model. Binding targetType is file or symbol. Save validates the complete resulting model atomically; ok=false means nothing was stored.");
                notes.push("UML uses language=mermaid and diagramType=class, sequence, flow or state with a matching header (classDiagram, sequenceDiagram, flowchart, stateDiagram-v2). Attach it to an existing C4 element or relationship. Source must parse and round-trip through the supported semantic model; rejected source includes correction details. Mockups are separate SVG artifacts; viewport dimensions must be positive, visual elements need stable data-adashi-id attributes, and external resources/scripts are forbidden. Before proposing a mockup revision, read mockup_get_revision_context and copy mockup.acceptedRevision into baseRevision.");
            }
        }
        "adashi_tasks" => notes.push("Task.number is the current desktop Task #, ordered by creation time then permanent id across ALL retained tasks before filtering/pagination. Numbers may change after Git updates or deletions. Resolve a user-supplied # with get taskNumber (optionally expectedRevision from the observed list); use the returned permanent taskId and version for every mutation and history/reference. Never pass a displayed number as taskId. create starts in todo. update may set active from any state, including reopening closed work. Returning to todo is forbidden. finish records active -> finished with a nonempty completionMemo; close records finished -> closed after review. A state may be re-set to itself. Lists default to todo/active/finished, limit 25 (1..100); states=[] selects nothing. Copy version from get/list. get returns full evidence; linked scopes are opt-in with includeDesignScopes and include complete Markdown documents/tokens. Design links use targetType element, relationship, uml, mockup or markdown and an existing target id."),
        "adashi_qa" => {
            notes.push("designSpecificationLinks supports targetType element, relationship, uml, mockup or markdown with an existing stable designExternalId. Markdown titles resolve from canonical storage; designExternalIds filters use those identities, never generated file paths.");
            notes.push("create_job stores a definition; run_jobs executes it. name and command must be nonempty. kind and scope are required: kind is one of lint, unit, integration, e2e, smoke, build, release and scope states the ONE behavior the job verifies. Every job must link at least one design specification or task it verifies. timeoutSeconds defaults 120 and is capped by kind (lint 120, smoke 300, unit 600, integration 900, e2e 1800, build 1800, release 3600); never raise a timeout to make an oversized job fit, split it instead. Only kind=release may package or bundle (tauri build, assemble-release-bundle, installer/.msi/.dmg/.appimage); a test job that builds an installer is rejected. workingDirectory defaults to the project folder; relative paths resolve under it. shell defaults to the platform shell. enabled defaults true. Lists return metadata; get_job/get_run return full evidence. Job lists default limit 25 (1..100); preserve query when following a cursor. Copy expectedVersion from job.version.");
            if matches!(operation,"run_jobs"|"list_jobs") { notes.push("query is an object, not a string. Query filters combine with AND: jobIds is membership (an empty list matches nothing); states is any listed derived state (green, red, running, needs-rerun); tags and taskIds/designExternalIds must all be present on the job (empty lists impose no restriction). States and tags are case-insensitive. enabled filters definitions but run_jobs always excludes disabled jobs."); }
            if operation == "run_jobs" {
                notes.push("run_jobs must name what to run: at least one of jobIds, states, tags, taskIds or designExternalIds. An empty query matching every enabled job is rejected unless allowBroadRun=true, and no run may exceed 12 jobs. Selected jobs whose latest evidence is already green are skipped unless force=true. maxDurationSeconds bounds the whole run (default 900, maximum 3600); a job may not outlive the remaining budget and unfinished jobs are recorded as skipped. A job with a live run cannot be reserved twice. Jobs that are running or interrupted can be stopped with cancel_run.");
            }
        }
        "adashi_memory" => notes.push("get supports exact noteId/runId/taskId filters and a case-insensitive literal query. Historical notes are dated evidence; superseded notes are hidden unless includeSuperseded=true. Append only useful durable handovers, at most 1000 characters; runId defaults to operationId when omitted/null/blank. Retention is at most 20 notes and 12000 total characters; oldest notes are removed first. Only an authorized coordinator may replace the summary (at most 4000 characters) and supersede reviewed note ids, using memory.memoryVersion. update_rule changes optional project-specific instructions using memory.protocolVersion; an empty rule disables them."),
        "adashi_rules" => notes.push("intend is exactly general, design or implementation. hook is exactly run.start, task.start, task.end or run.end. Call every lifecycle hook and apply each nonempty injectionPrompt even when rules is empty; rules and sections are metadata. status=empty means no instructions. Startup includes project-specific rules/context; shared Adashi instructions belong in agents_template.md. memoryContext defaults summary; protocolOnly omits the summary. update requires a full rule body and the rule version from list."),
        "adashi_intents" => notes.push("publish creates/renews an expiring advisory marker, never a lock or write authority. agentRunId, resourceKind and resourceId must be nonempty; ttlSeconds is 1..86400. list returns live intents."),
        "adashi_grep" => notes.push("Searches design, tasks and memory, not QA or rules. Pattern is case-insensitive; whitespace-separated terms are AND; quoted phrases are exact substrings. in:, file:, type:, state: and limit: filter; unknown keys are literal text. Empty text returns a top-layer overview. Pair file: with a search term. Results are bounded snippets: drill into design:<externalId> with design get_scope, task:<id> with tasks get, memory:<noteId> with memory get/noteId before acting."),
        "adashi_help" => notes.push("Omit tool and skill for the tools+skills catalog. Pass skill to read one on-demand skill; the always-on workflow indexes them, so read only the skill that matches the work. Pass tool (and optionally operation) for one operation's exact contract. changeTypes only narrows adashi_design save. Help opens no project and writes nothing."),
        _ => {}
    }
    notes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(tool: &str, operation: Option<&str>, changes: Option<Vec<&str>>) -> Value {
        get(HelpParams {
            tool: Some(tool.into()),
            operation: operation.map(str::to_owned),
            change_types: changes.map(|v| v.into_iter().map(str::to_owned).collect()),
            skill: None,
        })
        .unwrap()
        .structured_content
        .unwrap()
    }

    fn check_refs(schema: &Value, value: &Value) {
        match value {
            Value::Object(object) => {
                if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
                    assert!(
                        schema.pointer(&reference[1..]).is_some(),
                        "dangling {reference}"
                    );
                }
                for child in object.values() {
                    check_refs(schema, child);
                }
            }
            Value::Array(array) => {
                for child in array {
                    check_refs(schema, child);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn every_operation_has_shared_contract_and_a_deserializable_example_without_a_project() {
        for tool in AdashiMcpServer::tool_router().list_all() {
            let mut operations = operations(&tool);
            if operations.is_empty() {
                operations.push(String::new());
            }
            for operation in operations {
                let result = read(
                    &tool.name,
                    (!operation.is_empty()).then_some(operation.as_str()),
                    None,
                );
                let schema = &result["parameterSchema"];
                let sample = result["exampleArguments"].clone();
                for name in result["requiredParameters"].as_array().unwrap() {
                    assert!(
                        sample.get(name.as_str().unwrap()).is_some(),
                        "{} {operation}: {name}",
                        tool.name
                    );
                }
                check_refs(schema, schema);
                macro_rules! parses {
                    ($ty:ty) => {
                        serde_json::from_value::<$ty>(sample).unwrap();
                    };
                }
                match tool.name.as_ref() {
                    "adashi_design" => {
                        parses!(DesignParams);
                    }
                    "adashi_tasks" => {
                        parses!(TasksParams);
                    }
                    "adashi_qa" => {
                        parses!(QaParams);
                    }
                    "adashi_rules" => {
                        parses!(RulesParams);
                    }
                    "adashi_memory" => {
                        parses!(MemoryParams);
                    }
                    "adashi_intents" => {
                        parses!(IntentsParams);
                    }
                    "adashi_grep" => {
                        parses!(GrepParams);
                    }
                    "adashi_help" => {
                        parses!(HelpParams);
                    }
                    _ => panic!("Missing example verification for {}", tool.name),
                }
                let expected =
                    errors::contract(&tool, json!({"operation":operation}).as_object().unwrap());
                assert_eq!(schema, &expected);
            }
        }
    }

    #[test]
    fn filtered_design_help_keeps_only_requested_variants_and_transitive_definitions() {
        let full = read("adashi_design", Some("save"), None);
        for change in full["changeExamples"].as_array().unwrap() {
            let op = change["op"].as_str().unwrap();
            serde_json::from_value::<DesignChange>(change.clone()).unwrap();
            let filtered = read("adashi_design", Some("save"), Some(vec![op]));
            assert_eq!(
                filtered,
                read("adashi_design", Some("save"), Some(vec![op]))
            );
            let schema = &filtered["parameterSchema"];
            let variants = schema["$defs"]["DesignChange"]["oneOf"].as_array().unwrap();
            assert_eq!(variants.len(), 1);
            assert_eq!(variant_name(&variants[0]), op);
            assert_eq!(filtered["exampleArguments"]["changes"], json!([change]));
            check_refs(schema, schema);
            assert!(filtered.to_string().len() < full.to_string().len());
            assert_ne!(filtered["contentVersion"], full["contentVersion"]);
        }
        let uml = read("adashi_design", Some("save"), Some(vec!["upsert_uml"]));
        assert!(uml["parameterSchema"]["$defs"]
            .get("MockupManifest")
            .is_none());
    }

    #[test]
    fn catalogs_are_small_and_invalid_selectors_explain_the_valid_choices() {
        let catalog = read("adashi_qa", None, None);
        assert!(catalog.get("parameterSchema").is_none());
        assert!(catalog["operations"]
            .as_array()
            .unwrap()
            .contains(&json!("create_job")));
        for (tool, op, changes, detail) in [
            ("wrong", None, None, "availableTools"),
            ("adashi_qa", Some("wrong"), None, "availableOperations"),
            ("adashi_grep", Some("search"), None, "availableOperations"),
            (
                "adashi_design",
                Some("save"),
                Some(vec![]),
                "availableChangeTypes",
            ),
            (
                "adashi_design",
                Some("save"),
                Some(vec!["wrong"]),
                "availableChangeTypes",
            ),
            (
                "adashi_qa",
                Some("create_job"),
                Some(vec!["upsert_uml"]),
                "exampleArguments",
            ),
        ] {
            let error = get(HelpParams {
                tool: Some(tool.into()),
                operation: op.map(str::to_owned),
                change_types: changes.map(|v| v.into_iter().map(str::to_owned).collect()),
                skill: None,
            })
            .unwrap_err();
            assert!(error.data.unwrap().get(detail).is_some());
        }
    }

    #[test]
    fn skills_are_listed_and_fetched_on_demand_without_a_project() {
        let catalog = get(HelpParams {
            tool: None,
            operation: None,
            change_types: None,
            skill: None,
        })
        .unwrap()
        .structured_content
        .unwrap();
        assert!(catalog["tools"].as_array().unwrap().contains(&json!("adashi_help")));
        let names = catalog["skills"].as_array().unwrap();
        assert_eq!(names.len(), crate::skills::SKILLS.len());

        let skill = get(HelpParams {
            tool: None,
            operation: None,
            change_types: None,
            skill: Some("design-authoring".into()),
        })
        .unwrap()
        .structured_content
        .unwrap();
        assert_eq!(skill["skill"], json!("design-authoring"));
        assert!(skill["body"].as_str().unwrap().len() > 200);
        assert!(skill.get("parameterSchema").is_none());

        let error = get(HelpParams {
            tool: None,
            operation: None,
            change_types: None,
            skill: Some("wrong".into()),
        })
        .unwrap_err();
        assert!(error.data.unwrap().get("availableSkills").is_some());

        let both = get(HelpParams {
            tool: Some("adashi_qa".into()),
            operation: None,
            change_types: None,
            skill: Some("qa-jobs".into()),
        })
        .unwrap_err();
        assert!(both.data.unwrap().get("availableSkills").is_some());
    }
}
