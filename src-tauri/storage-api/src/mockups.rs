use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MockupManifest {
    pub schema_version: i64,
    pub key: String,
    pub attached_to_external_id: String,
    pub viewport_width: i64,
    pub viewport_height: i64,
    pub screen: String,
    pub state: String,
    pub fidelity: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MockupEditOperation {
    pub sequence: i64,
    pub kind: String,
    pub target_element_id: Option<String>,
    pub payload_json: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MockupAnnotation {
    pub external_id: String,
    pub svg_path: String,
    pub optional_text: String,
    pub sort_order: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MockupProposal {
    pub base_revision: i64,
    pub proposed_svg: String,
    pub proposed_manifest: MockupManifest,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiMockup {
    pub id: i64,
    pub external_id: String,
    pub title: String,
    pub manifest: MockupManifest,
    pub accepted_svg: String,
    pub accepted_revision: i64,
    pub accepted_version: i64,
    pub working_version: i64,
    pub working_svg: Option<String>,
    pub base_revision: Option<i64>,
    pub status: String,
    pub edit_operations: Vec<MockupEditOperation>,
    pub annotations: Vec<MockupAnnotation>,
    pub proposal: Option<MockupProposal>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MockupSummary {
    pub external_id: String,
    pub title: String,
    pub attached_to_external_id: String,
    pub viewport_width: i64,
    pub viewport_height: i64,
    pub screen: String,
    pub state: String,
    pub fidelity: String,
    pub accepted_revision: i64,
    pub accepted_version: i64,
    pub working_version: i64,
    pub status: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateMockupInput {
    pub external_id: String,
    pub title: String,
    pub attached_to_external_id: String,
    pub viewport_width: i64,
    pub viewport_height: i64,
    pub screen: String,
    pub state: String,
    pub fidelity: String,
    pub schema_version: Option<i64>,
    pub accepted_svg: String,
    pub operation_id: String,
    pub expected_accepted_version: i64,
    pub expected_working_version: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveDraftInput {
    pub external_id: String,
    pub working_svg: String,
    pub base_revision: i64,
    pub operation_id: String,
    pub expected_accepted_version: i64,
    pub expected_working_version: i64,
    pub edit_operations: Vec<MockupEditOperation>,
    pub annotations: Vec<MockupAnnotation>,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MockupMutationInput {
    pub external_id: String,
    pub operation_id: String,
    pub expected_accepted_version: i64,
    pub expected_working_version: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProposeMockupInput {
    pub external_id: String,
    pub base_revision: i64,
    pub proposed_svg: String,
    pub proposed_manifest: MockupManifest,
    pub operation_id: String,
    pub expected_accepted_version: i64,
    pub expected_working_version: i64,
}
