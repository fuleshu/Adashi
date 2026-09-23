use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DocumentReadToken {
    /// Copy documentId from a documents entry returned by get_by_ids, get_scope,
    /// get_bindings or get_documents. This is an opaque document identity.
    pub document_id: String,
    /// Copy readToken unchanged from the same complete document snapshot.
    /// On out_of_date, merge with currentDocument before using its new token.
    pub read_token: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesignDocument {
    pub document_id: String,
    /// Complete canonical editable content; null means the document no longer exists.
    pub document: Value,
    pub read_token: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentGuard {
    pub operation_id: String,
    pub read_tokens: Vec<DocumentReadToken>,
}

pub fn hash(value: &impl Serialize) -> Result<String, String> {
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

/// Matches the existing v1 design document token, including absent documents.
pub fn document_token(document_id: &str, document: &Value) -> Result<String, String> {
    hash(&serde_json::json!([
        "adashi.design-document.v1",
        document_id,
        document
    ]))
}
