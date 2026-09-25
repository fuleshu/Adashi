use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;

fn document_id(kind: &str, id: &str) -> String {
    let kind = match kind {
        "mockup.accepted" | "mockup.working" => "mockup",
        other => other.strip_prefix("design.").unwrap_or(other),
    };
    format!("{kind}:{id}")
}

pub fn with_documents(
    db: &dyn adashi_storage_api::ReadSnapshot,
    result: impl Serialize,
) -> Result<Value, String> {
    let mut result = serde_json::to_value(result).map_err(|error| error.to_string())?;
    let mut ids = BTreeSet::new();
    for record in result["markdown"]["documents"].as_array().into_iter().flatten() {
        if let Some(id) = record["externalId"].as_str() { ids.insert(format!("markdown:{id}")); }
    }
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
    result["documents"] = serde_json::to_value(
        db.design_documents(&ids.into_iter().collect::<Vec<_>>())
            .map_err(|e| e.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    Ok(result)
}
