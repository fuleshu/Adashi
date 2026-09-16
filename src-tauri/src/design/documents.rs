//! Canonical editable document snapshots and optimistic concurrency for the MCP API.
//! All mutation checks run inside the same immediate transaction as the eventual write.
use super::*;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const WRITE_PROTOCOL: &str = "# Current design write contract\nThe current API supersedes any older expectedRevision or guard/readSet/writeSet examples in saved guidance. Read existing documents with adashi_design get_by_ids, get_scope, get_bindings, or get_documents. Each documents entry includes documentId, full document, and readToken. Save using operationId, changeIntent, changes, and readTokens copied as {documentId,readToken} pairs for all existing documents being changed or deleted. New identities need no token. Referenced parents/endpoints need no token unless modified. Adashi checks dependencies atomically. On out_of_date nothing is saved: merge intended edits into the returned currentDocument and retry with its readToken and a new operationId. Never only replace the token on an old payload. Deletes also protect dependent documents they remove. Overview/search/index entries are navigation, not editable snapshots.";

#[derive(Clone, Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DocumentReadToken {
    /// Copy documentId from a documents entry returned by get_by_ids, get_scope,
    /// get_bindings or get_documents. This is an opaque document identity.
    pub document_id: String,
    /// Copy readToken unchanged from the same complete document snapshot.
    /// On out_of_date, merge with currentDocument before using its new token.
    pub read_token: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesignDocument {
    pub document_id: String,
    /// Complete canonical editable content; null means the document no longer exists.
    pub document: Value,
    pub read_token: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentGuard {
    pub operation_id: String,
    pub read_tokens: Vec<DocumentReadToken>,
}

pub(super) fn hash(value: &impl Serialize) -> Result<String, String> {
    // Sort recursively even when another dependency enables serde_json/preserve_order.
    let canonical = canonicalize(serde_json::to_value(value).map_err(|error| error.to_string())?);
    let bytes = serde_json::to_vec(&canonical).map_err(|error| error.to_string())?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn canonicalize(value: Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .into_iter()
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .map(|(key, value)| (key, canonicalize(value)))
                .collect(),
        ),
        Value::Array(array) => Value::Array(array.into_iter().map(canonicalize).collect()),
        value => value,
    }
}

pub(super) fn document_id(kind: &str, id: &str) -> String {
    let kind = match kind {
        "mockup.accepted" | "mockup.working" => "mockup",
        other => other.strip_prefix("design.").unwrap_or(other),
    };
    format!("{kind}:{id}")
}

fn editable(value: impl Serialize, omit: &[&str]) -> Result<Value, String> {
    let mut value = serde_json::to_value(value).map_err(|error| error.to_string())?;
    if let Some(object) = value.as_object_mut() {
        for field in omit {
            object.remove(*field);
        }
    }
    Ok(value)
}

/// The caller owns a read or write transaction, keeping the content and token in one snapshot.
pub fn load_documents(
    db: &Connection,
    project_id: i64,
    ids: &[String],
) -> Result<Vec<DesignDocument>, String> {
    let workspace = load_workspace(db)?;
    let requested = ids.iter().map(String::as_str).collect::<BTreeSet<_>>();
    let mut documents = BTreeMap::new();
    for element in load_elements(db, workspace.id)? {
        let id = document_id("design.element", &element.external_id);
        if requested.contains(id.as_str()) {
            documents.insert(id, editable(element, &["version"])?);
        }
    }
    for relationship in load_relationships(db, workspace.id)? {
        let id = document_id("design.relationship", &relationship.external_id);
        if requested.contains(id.as_str()) {
            documents.insert(id, editable(relationship, &["version"])?);
        }
    }
    for diagram in load_diagrams(db, workspace.id)? {
        let id = document_id("design.uml", &diagram.key);
        if requested.contains(id.as_str()) && diagram.language == "mermaid" {
            documents.insert(
                id,
                editable(
                    diagram,
                    &[
                        "version",
                        "artifactRole",
                        "artifactLabel",
                        "artifactRank",
                        "attachedToTargetType",
                        "sortOrder",
                    ],
                )?,
            );
        }
    }
    for binding in load_bindings(db, workspace.id)? {
        let id = document_id(
            "design.binding",
            &format!(
                "{}|{}|{}",
                binding.design_external_id, binding.target_type, binding.target
            ),
        );
        if requested.contains(id.as_str()) {
            documents.insert(id, editable(binding, &["version"])?);
        }
    }
    for mockup in mockups::load_summaries(db, project_id)? {
        let id = document_id("mockup.accepted", &mockup.external_id);
        if requested.contains(id.as_str()) {
            let mut content = editable(
                mockups::load_mockup(db, project_id, &mockup.external_id)?,
                &[
                    "id",
                    "acceptedVersion",
                    "workingVersion",
                    "createdAt",
                    "updatedAt",
                ],
            )?;
            if let Some(proposal) = content.get_mut("proposal").and_then(Value::as_object_mut) {
                proposal.remove("createdAt");
            }
            documents.insert(id, content);
        }
    }
    requested
        .into_iter()
        .map(|id| {
            let (kind, identity) = id.split_once(':').ok_or_else(|| {
                format!("Invalid documentId '{id}'. Copy documentId from a design read response.")
            })?;
            if identity.is_empty()
                || !matches!(
                    kind,
                    "element" | "relationship" | "uml" | "binding" | "mockup"
                )
            {
                return Err(format!(
                    "Invalid documentId '{id}'. Copy documentId from a design read response."
                ));
            }
            let document = documents.remove(id).unwrap_or(Value::Null);
            let read_token = hash(&json!(["adashi.design-document.v1", id, document]))?;
            Ok(DesignDocument {
                document_id: id.to_string(),
                document,
                read_token,
            })
        })
        .collect()
}

/// Add full editable snapshots only to explicit retrieval, never to search/startup snippets.
pub fn with_documents(
    db: &Connection,
    project_id: i64,
    result: impl Serialize,
) -> Result<Value, String> {
    let mut result = serde_json::to_value(result).map_err(|error| error.to_string())?;
    let mut ids = BTreeSet::new();
    for (field, kind, identity) in [
        ("elements", "design.element", "externalId"),
        ("ancestors", "design.element", "externalId"),
        ("relationships", "design.relationship", "externalId"),
        ("diagrams", "design.uml", "key"),
        ("mockups", "mockup.accepted", "externalId"),
    ] {
        for record in result
            .get(field)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if field == "diagrams" && record["language"] != "mermaid" {
                continue;
            }
            if let Some(id) = record[identity].as_str() {
                ids.insert(document_id(kind, id));
            }
        }
    }
    for binding in result
        .get("bindings")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let id = format!(
            "{}|{}|{}",
            binding["designExternalId"].as_str().unwrap_or(""),
            binding["targetType"].as_str().unwrap_or(""),
            binding["target"].as_str().unwrap_or("")
        );
        ids.insert(document_id("design.binding", &id));
    }
    result["documents"] = serde_json::to_value(load_documents(
        db,
        project_id,
        &ids.into_iter().collect::<Vec<_>>(),
    )?)
    .map_err(|error| error.to_string())?;
    Ok(result)
}

fn check_tokens(
    db: &Connection,
    project_id: i64,
    input: &DocumentGuard,
    guard: &concurrency::MutationGuard,
) -> Result<(), String> {
    let mut submitted = BTreeMap::new();
    for token in &input.read_tokens {
        if token.read_token.is_empty()
            || submitted
                .insert(token.document_id.as_str(), token.read_token.as_str())
                .is_some()
        {
            return Err("Each readTokens entry needs a non-empty readToken and a unique documentId copied from retrieval.".into());
        }
    }
    let ids = guard
        .write_set
        .iter()
        .map(|target| document_id(&target.resource_kind, &target.resource_id))
        .collect::<Vec<_>>();
    let current = load_documents(db, project_id, &ids)?;
    let mut conflicts = Vec::new();
    for document in current {
        let supplied = submitted.get(document.document_id.as_str()).copied();
        let code = match supplied {
            Some(token) if token != document.read_token => "out_of_date",
            None if !document.document.is_null() => "read_required",
            _ => continue,
        };
        conflicts.push(json!({"code":code,"documentId":document.document_id,
            "currentDocument":document.document,"readToken":document.read_token}));
    }
    if conflicts.is_empty() {
        return Ok(());
    }
    let stale = conflicts.iter().any(|item| item["code"] == "out_of_date");
    let request = "Merge your intended changes into each returned currentDocument, then retry with its readToken in readTokens and a new operationId. Do not only replace the token on an old payload. A null currentDocument means it was deleted; reconsider the edit before explicitly recreating it. Every retry is checked again.";
    let mut error = json!({"code":if stale {"out_of_date"} else {"read_required"},
        "stored":false,"message":if stale {"The design document changed since you read it. Nothing was saved."}
            else {"Existing documents require the readToken from a complete read. Nothing was saved. The response includes every target missing a token, including dependent documents a delete would remove."},
        "request":request});
    if conflicts.len() == 1 {
        for key in ["documentId", "currentDocument", "readToken"] {
            error[key] = conflicts[0][key].clone();
        }
    } else {
        error["conflicts"] = json!(conflicts);
    }
    Err(error.to_string())
}

impl DesignGuardInput for &DocumentGuard {
    fn operation_id(&self) -> Option<&str> {
        Some(&self.operation_id)
    }
    fn resolve_for_changes(
        self,
        db: &Connection,
        project_id: i64,
        changes: &[DesignChange],
    ) -> Result<concurrency::MutationGuard, String> {
        let guard = required_guard(db, project_id, &self.operation_id, changes)?;
        check_tokens(db, project_id, self, &guard)?;
        Ok(guard)
    }
    fn resolve_for_descriptions(
        self,
        db: &Connection,
        project_id: i64,
        updates: &[ElementDescriptionUpdate],
    ) -> Result<concurrency::MutationGuard, String> {
        let write_set = updates
            .iter()
            .map(|update| {
                let resource_id = update.external_id.trim().to_string();
                Ok(concurrency::ResourceExpectation {
                    expected_version: concurrency::load_version(
                        db,
                        project_id,
                        "design.element",
                        &resource_id,
                    )?,
                    resource_kind: "design.element".into(),
                    resource_id,
                })
            })
            .collect::<Result<_, String>>()?;
        let guard = concurrency::MutationGuard {
            operation_id: self.operation_id.clone(),
            read_set: vec![],
            write_set,
        };
        check_tokens(db, project_id, self, &guard)?;
        Ok(guard)
    }
}

pub(super) fn result_tokens(
    db: &Connection,
    project_id: i64,
    targets: &[ResourceChangeTarget],
) -> Result<Vec<DocumentReadToken>, String> {
    let ids = targets
        .iter()
        .map(|target| document_id(&target.kind, &target.id))
        .collect::<Vec<_>>();
    Ok(load_documents(db, project_id, &ids)?
        .into_iter()
        .map(|doc| DocumentReadToken {
            document_id: doc.document_id,
            read_token: doc.read_token,
        })
        .collect())
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct OperationReceipt {
    pub request_hash: String,
    pub result: DesignSaveResult,
}

pub(super) fn replay(
    db: &Connection,
    project_id: i64,
    operation_id: &str,
    request_hash: &str,
) -> Result<Option<DesignSaveResult>, String> {
    let Some(value) = concurrency::load_operation::<Value>(db, project_id, operation_id)? else {
        return Ok(None);
    };
    let receipt: OperationReceipt = serde_json::from_value(value).map_err(|_| {
        "operationId was already used by another or older mutation. Submit this new mutation with a new operationId.".to_string()
    })?;
    if receipt.request_hash != request_hash {
        return Err("operationId was already used with different arguments. Use a new operationId for a changed or merged request; reuse it only for an identical retry.".into());
    }
    Ok(Some(receipt.result))
}

pub(super) fn record(
    db: &Connection,
    project_id: i64,
    operation_id: &str,
    request_hash: &str,
    result: &DesignSaveResult,
) -> Result<(), String> {
    concurrency::record_no_op(
        db,
        project_id,
        operation_id,
        &OperationReceipt {
            request_hash: request_hash.into(),
            result: result.clone(),
        },
    )
}
