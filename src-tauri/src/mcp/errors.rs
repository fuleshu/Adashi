//! Repairable tool failures. The operation schemas come from the same typed inputs used by
//! the handlers, so error help cannot quietly lose required fields when an input changes.
use super::*;
use rmcp::model::{JsonObject, Tool};
use serde_json::Value;

fn schema<T: rmcp::schemars::JsonSchema>() -> Value {
    serde_json::to_value(rmcp::schemars::schema_for!(T)).expect("input schema is JSON")
}

fn operation_schema(tool: &str, operation: &str) -> Option<Value> {
    Some(match (tool, operation) {
        ("adashi_design", "save") => schema::<DesignSaveParams>(),
        ("adashi_design", "get_scope") => schema::<DesignScopeParams>(),
        ("adashi_design", "get_by_ids" | "get_documents") => schema::<DesignByIdsParams>(),
        ("adashi_design", "search") => schema::<DesignSearchParams>(),
        ("adashi_design", "get_overview") => schema::<DesignOverviewParams>(),
        ("adashi_design", "get_bindings") => schema::<DesignBindingsParams>(),
        ("adashi_design", "set_element_descriptions") => schema::<SetElementDescriptionsParams>(),
        ("adashi_design", "health" | "mockup_list_pending_revisions") => schema::<ProjectParams>(),
        ("adashi_design", "mockup_get_revision_context") => schema::<MockupContextParams>(),
        ("adashi_tasks", "create") => schema::<CreateTaskParams>(),
        ("adashi_tasks", "list") => schema::<ListTasksParams>(),
        ("adashi_tasks", "update") => schema::<UpdateTaskParams>(),
        ("adashi_tasks", "finish") => schema::<FinishTaskParams>(),
        ("adashi_tasks", "close") => schema::<CloseTaskParams>(),
        ("adashi_tasks", "delete") => schema::<DeleteTaskParams>(),
        ("adashi_tasks", "get") => schema::<TaskIdParams>(),
        ("adashi_qa", "create_job") => schema::<CreateQaJobParams>(),
        ("adashi_qa", "update_job") => schema::<UpdateQaJobParams>(),
        ("adashi_qa", "delete_job") => schema::<DeleteQaJobParams>(),
        ("adashi_qa", "list_jobs") => schema::<ListQaJobsParams>(),
        ("adashi_qa", "run_jobs") => schema::<RunQaJobsParams>(),
        ("adashi_qa", "list_runs") => schema::<ListQaRunsParams>(),
        ("adashi_qa", "get_job") => schema::<QaJobIdParams>(),
        ("adashi_qa", "get_run") => schema::<QaRunIdParams>(),
        ("adashi_memory", "get") => schema::<GetMemoryParams>(),
        ("adashi_memory", "append") => {
            let mut value = schema::<AppendMemoryNoteParams>();
            // The public wrapper defaults runId to operationId before calling the handler.
            value["required"]
                .as_array_mut()
                .unwrap()
                .retain(|field| field != "runId");
            value["properties"]["runId"] = json!({"type":["string","null"], "description":"Defaults to operationId when omitted, null or blank."});
            value
        }
        ("adashi_memory", "update") => schema::<UpdateMemoryParams>(),
        ("adashi_memory", "update_rule") => schema::<UpdateMemoryRuleParams>(),
        ("adashi_rules", "get_rule_injections") => schema::<RuleInjectionParams>(),
        ("adashi_rules", "list") => schema::<ProjectParams>(),
        ("adashi_rules", "create") => schema::<CreateRuleParams>(),
        ("adashi_rules", "update") => schema::<UpdateRuleParams>(),
        ("adashi_rules", "delete") => schema::<DeleteRuleParams>(),
        ("adashi_intents", "publish") => schema::<PublishIntentParams>(),
        ("adashi_intents", "list") => schema::<ProjectParams>(),
        _ => return None,
    })
}

fn contract(tool: &Tool, arguments: &JsonObject) -> Value {
    let operation = arguments.get("operation").and_then(Value::as_str);
    let mut schema = operation
        .and_then(|operation| operation_schema(&tool.name, operation))
        .unwrap_or_else(|| json!(tool.input_schema));
    if let Some(operation) = operation.filter(|op| operation_schema(&tool.name, op).is_some()) {
        schema["properties"]["operation"] = json!({"type":"string", "const":operation});
        schema["required"]
            .as_array_mut()
            .unwrap()
            .insert(0, json!("operation"));
    }
    // Preserve field guidance from the published capability input where the internal input
    // only carries types. Nested definitions remain the exact operation-specific definitions.
    if let Some(properties) = schema["properties"].as_object_mut() {
        for (name, property) in properties {
            if property.get("description").is_none() {
                if let Some(description) = tool
                    .input_schema
                    .get("properties")
                    .and_then(|p| p.get(name))
                    .and_then(|p| p.get("description"))
                {
                    property["description"] = description.clone();
                }
            }
        }
    }
    schema
}

/// Convert only failures; successful reads/writes and their schemas remain unchanged.
pub(super) fn complete(
    _server: &AdashiMcpServer,
    tool: &Tool,
    arguments: &JsonObject,
    result: Result<CallToolResult, ErrorData>,
) -> CallToolResult {
    let (mut reason, details, protocol_code) = match result {
        Err(error) => (error.message.to_string(), error.data, Some(error.code)),
        Ok(result) => {
            let design_rejected = tool.name == "adashi_design"
                && result.structured_content.as_ref().and_then(|v| v.get("ok"))
                    == Some(&json!(false));
            if result.is_error != Some(true) && !design_rejected {
                return result;
            }
            let reason = if design_rejected {
                result.structured_content.as_ref().unwrap()["errors"].to_string()
            } else {
                result
                    .content
                    .iter()
                    .filter_map(|block| block.as_text())
                    .map(|text| text.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            (reason, result.structured_content, None)
        }
    };
    if matches!(
        reason.as_str(),
        "Adashi MCP request failed" | "Adashi MCP server failed"
    ) {
        if let Some(details) = &details {
            reason = details
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| details.to_string());
        }
    }
    let schema = contract(tool, arguments);
    let required = schema["required"].as_array().cloned().unwrap_or_default();
    let missing: Vec<_> = required
        .iter()
        .filter(|field| {
            field
                .as_str()
                .is_some_and(|name| arguments.get(name).is_none_or(Value::is_null))
        })
        .cloned()
        .collect();
    let unused: Vec<_> = arguments
        .keys()
        .filter(|name| schema["properties"].get(*name).is_none())
        .collect();
    let mut issues = Vec::new();
    if let Some(changes) = arguments.get("changes").and_then(Value::as_array) {
        for (index, change) in changes.iter().enumerate() {
            if let Err(error) = serde_json::from_value::<DesignChange>(change.clone()) {
                issues
                    .push(json!({"path":format!("changes[{index}]"),"message":error.to_string()}));
            }
        }
    }
    let operation = arguments
        .get("operation")
        .and_then(Value::as_str)
        .unwrap_or("");
    let mut report = json!({
        "code":"adashi.tool_error",
        "tool":tool.name,
        "operation":operation,
        "message":reason,
        "details":details,
        "protocolCode":protocol_code,
        "requiredParameters":required,
        "missingParameters":missing,
        "parametersNotUsedByOperation":unused,
        "parameterIssues":issues,
        "parameterSchema":schema,
        "guidance":[
            "Correct the reported parameters and submit a new tool call. This is a terminal tool error, not a transport timeout.",
            "parameterSchema is the complete schema for this operation: required fields, optional fields, types, enums and nested definitions. Do not add fields from a different operation; omit unused optional fields.",
            "Use a unique operationId per mutation; reuse it only for an identical retry. expectedVersion is the resource version returned by retrieval, never the project revision. Read and review the resource again before changing a stale expectedVersion."
        ]
    });
    if let Some(example) = example(&tool.name, operation) {
        report["exampleArguments"] = example;
        report["exampleNote"] = json!("Illustrative values only: replace project names, ids, content and versions with your intended values. The example is not an instruction to execute a different action.");
    }
    if tool.name == "adashi_design" && matches!(operation, "save" | "set_element_descriptions") {
        report["guidance"][2] = json!("Use a unique operationId for each design mutation. Copy readTokens from the documents you read; creates need no token. On an out_of_date response merge the intended changes into currentDocument and use a new operationId. An identical retry with the same operationId returns the original receipt.");
        report["readTokenInstructions"] = json!("Read complete documents with get_by_ids/get_scope/get_bindings/get_documents. Copy their documentId/readToken pairs into readTokens for existing targets, including documents removed by a cascade. New identities need no token. Adashi owns dependency checks; omit legacy guard/readSet/writeSet. On out_of_date, merge intended edits into currentDocument before retrying with its readToken and a new operationId. Nothing is automatically retried.");
        if let Some(details) = details.as_ref().filter(|value| {
            matches!(
                value["code"].as_str(),
                Some("out_of_date" | "read_required")
            )
        }) {
            // Full documents can be large. Keep the actionable snapshot exactly once.
            report.as_object_mut().unwrap().remove("details");
            for key in [
                "code",
                "message",
                "stored",
                "request",
                "conflicts",
                "documentId",
                "currentDocument",
                "readToken",
            ] {
                if let Some(value) = details.get(key) {
                    report[key] = value.clone();
                }
            }
        }
    }
    // Some clients use only text and others only structuredContent. Both get identical help.
    CallToolResult::structured_error(report)
}

fn example(tool: &str, operation: &str) -> Option<Value> {
    Some(match (tool, operation) {
        ("adashi_design", "save") => {
            json!({"projectName":"Your project","operation":"save","operationId":"design-add-container-001","changeIntent":"Add a container under an existing system","changes":[{"op":"upsert_element","externalId":"new-container","parentExternalId":"system-id","elementType":"Container","name":"New container"}]})
        }
        ("adashi_design", "set_element_descriptions") => {
            json!({"projectName":"Your project","operation":operation,"operationId":"describe-001","readTokens":[{"documentId":"element:element-id","readToken":"<copy readToken from the complete document read>"}],"updates":[{"externalId":"element-id","description":"Updated responsibility"}]})
        }
        ("adashi_tasks", "create") => {
            json!({"projectName":"Your project","operation":operation,"operationId":"task-create-001","title":"Implement the next milestone"})
        }
        ("adashi_tasks", "update") => {
            json!({"projectName":"Your project","operation":operation,"operationId":"task-update-001","taskId":1,"expectedVersion":1,"state":"active"})
        }
        ("adashi_tasks", "finish") => {
            json!({"projectName":"Your project","operation":operation,"operationId":"task-finish-001","taskId":1,"expectedVersion":2,"completionMemo":"Implementation and verification evidence"})
        }
        ("adashi_qa", "create_job") => {
            json!({"projectName":"Your project","operation":operation,"operationId":"qa-create-001","name":"Workspace tests","command":"cargo test --workspace"})
        }
        ("adashi_qa", "run_jobs") => {
            json!({"projectName":"Your project","operation":operation,"operationId":"qa-run-001","query":{"jobIds":[1]}})
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_advertised_operation_has_a_complete_parameter_contract() {
        for tool in AdashiMcpServer::tool_router().list_all() {
            if tool.name == "adashi_grep" {
                continue;
            }
            let published = json!(tool.input_schema);
            let reference = published["properties"]["operation"]["$ref"]
                .as_str()
                .unwrap();
            let operations = published.pointer(&reference[1..]).unwrap()["enum"]
                .as_array()
                .unwrap();
            for operation in operations {
                let operation = operation.as_str().unwrap();
                assert!(
                    operation_schema(&tool.name, operation).is_some(),
                    "{} {operation} has no error contract",
                    tool.name
                );
                let args = json!({"operation":operation,"projectName":"Fixture"});
                let schema = contract(&tool, args.as_object().unwrap());
                assert_eq!(schema["properties"]["operation"]["const"], operation);
                assert_eq!(schema["additionalProperties"], false);
                for field in schema["required"].as_array().unwrap() {
                    let name = field.as_str().unwrap();
                    assert!(schema["properties"].get(name).is_some());
                    assert!(published["properties"].get(name).is_some());
                }
            }
        }
    }

    #[test]
    fn missing_qa_parameters_are_reported_together_with_exact_schema_and_example() {
        let server = AdashiMcpServer::new("unused-settings.json".into());
        let tool = AdashiMcpServer::tool_router()
            .get("adashi_qa")
            .unwrap()
            .clone();
        let args = json!({"operation":"run_jobs","projectName":"Fixture","limit":10});
        let result = complete(
            &server,
            &tool,
            args.as_object().unwrap(),
            Err(missing_field("operationId")),
        );
        assert_eq!(result.is_error, Some(true));
        let report = result.structured_content.as_ref().unwrap();
        assert_eq!(report["missingParameters"], json!(["operationId", "query"]));
        assert_eq!(report["parametersNotUsedByOperation"], json!(["limit"]));
        assert_eq!(
            report["parameterSchema"]["required"],
            json!(["operation", "projectName", "operationId", "query"])
        );
        assert_eq!(report["exampleArguments"]["query"]["jobIds"], json!([1]));
        let text: Value = serde_json::from_str(&result.content[0].as_text().unwrap().text).unwrap();
        assert_eq!(&text, report);
    }

    #[test]
    fn structured_conflicts_keep_their_evidence_and_successes_stay_unchanged() {
        let server = AdashiMcpServer::new("unused-settings.json".into());
        let tool = AdashiMcpServer::tool_router()
            .get("adashi_tasks")
            .unwrap()
            .clone();
        let args = json!({"operation":"update","projectName":"Fixture"});
        let evidence = json!({"code":"resource.conflict","conflicts":[{"resourceKind":"task","resourceId":"1","expectedVersion":1,"currentVersion":2}]});
        let result = complete(
            &server,
            &tool,
            args.as_object().unwrap(),
            Err(tool_error(evidence.to_string())),
        );
        let report = result.structured_content.unwrap();
        assert_eq!(report["details"], evidence);
        assert!(report["message"]
            .as_str()
            .unwrap()
            .contains("resource.conflict"));
        let success = CallToolResult::structured(json!({"task":{"id":1,"version":2}}));
        let before = serde_json::to_value(&success).unwrap();
        let after = complete(&server, &tool, args.as_object().unwrap(), Ok(success));
        assert_eq!(serde_json::to_value(after).unwrap(), before);
    }
}
