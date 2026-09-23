use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceExpectation {
    /// Design resource kinds: design.element, design.relationship, design.uml,
    /// design.binding, mockup.accepted, mockup.working. The kind is not "design".
    pub resource_kind: String,
    /// externalId for elements/relationships/mockups, key for UML, or
    /// designExternalId|targetType|target for bindings.
    pub resource_id: String,
    /// Resource version from retrieval, or 0 when creating a resource that does not exist.
    /// Never use the project revision. Review the current resource before resolving a conflict.
    pub expected_version: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MutationGuard {
    /// Unique id for one mutation; reuse only for an identical retry.
    pub operation_id: String,
    /// Referenced parents, relationship endpoints and attachments, with their observed
    /// versions. Omit resources already in writeSet; each identity appears only once.
    #[serde(default)]
    pub read_set: Vec<ResourceExpectation>,
    /// Every resource being created, updated or deleted, including cascading deletes.
    /// Upsert/delete mockup writes both mockup.accepted and mockup.working; a proposal
    /// writes mockup.working and reads mockup.accepted.
    #[serde(default)]
    pub write_set: Vec<ResourceExpectation>,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResourceVersion {
    pub resource_kind: String,
    pub resource_id: String,
    pub version: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResourceConflict {
    pub resource_kind: String,
    pub resource_id: String,
    pub expected_version: i64,
    pub current_version: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResourceIntent {
    pub agent_run_id: String,
    pub resource_kind: String,
    pub resource_id: String,
    pub expires_at: String,
}
