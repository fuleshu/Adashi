use crate::{documents, mockups};
use serde::{Deserialize, Serialize};

/// Publishes non-negative counts using portable JSON Schema validation keywords.
///
/// Schemars otherwise emits a custom `uint` format for `usize`, which MCP
/// clients are permitted to ignore.
fn nonnegative_count_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": "integer",
        "minimum": 0,
    })
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct DesignElementRecord {
    pub version: i64,
    pub external_id: String,
    pub parent_external_id: Option<String>,
    pub element_type: String,
    pub name: String,
    pub description: String,
    pub technology: String,
    pub tags: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct DesignRelationshipRecord {
    pub version: i64,
    pub external_id: String,
    pub source_external_id: String,
    pub destination_external_id: String,
    pub description: String,
    pub technology: String,
    pub tags: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct DesignDiagramRecord {
    pub version: i64,
    pub key: String,
    pub language: String,
    pub title: String,
    pub diagram_type: String,
    pub artifact_role: String,
    pub artifact_label: String,
    pub artifact_rank: i64,
    pub attached_to_external_id: Option<String>,
    pub attached_to_target_type: Option<String>,
    pub sort_order: i64,
    pub source: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct DesignArtifactTypeRecord {
    pub diagram_type: String,
    pub artifact_role: String,
    pub artifact_label: String,
    pub artifact_rank: i64,
    pub mermaid_header: String,
    pub description: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct DesignBindingRecord {
    pub version: i64,
    pub design_external_id: String,
    pub target_type: String,
    pub target: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(
    tag = "op",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum DesignChange {
    UpsertMarkdown {
        external_id: String,
        title: String,
        body: String,
        #[serde(default)]
        design_links: Vec<crate::markdown::DesignAssociation>,
    },
    DeleteMarkdown {
        external_id: String,
    },
    UpsertElement {
        external_id: String,
        parent_external_id: Option<String>,
        element_type: String,
        name: String,
        description: Option<String>,
        technology: Option<String>,
        tags: Option<String>,
    },
    UpsertRelationship {
        external_id: String,
        source_external_id: String,
        destination_external_id: String,
        description: String,
        technology: Option<String>,
        tags: Option<String>,
    },
    UpsertUml {
        key: String,
        title: String,
        language: MermaidLanguage,
        diagram_type: UmlDiagramType,
        attached_to_external_id: String,
        source: String,
    },
    UpsertBinding {
        design_external_id: String,
        target_type: String,
        target: String,
    },
    UpsertMockup {
        external_id: String,
        title: String,
        attached_to_external_id: String,
        viewport_width: i64,
        viewport_height: i64,
        screen: String,
        mockup_state: String,
        fidelity: String,
        schema_version: Option<i64>,
        accepted_svg: String,
    },
    UpsertMockupProposal {
        external_id: String,
        base_revision: i64,
        proposed_svg: String,
        proposed_manifest: mockups::MockupManifest,
    },
    DeleteElement {
        external_id: String,
    },
    DeleteRelationship {
        external_id: String,
    },
    DeleteUml {
        key: String,
    },
    DeleteBinding {
        design_external_id: String,
        target_type: String,
        target: String,
    },
    DeleteMockup {
        external_id: String,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MermaidLanguage {
    Mermaid,
}

impl MermaidLanguage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mermaid => "mermaid",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum UmlDiagramType {
    Class,
    Sequence,
    Flow,
    State,
}

impl UmlDiagramType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Class => "class",
            Self::Sequence => "sequence",
            Self::Flow => "flow",
            Self::State => "state",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ElementDescriptionUpdate {
    pub external_id: String,
    pub description: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct DesignOverviewResult {
    pub markdown: crate::markdown::MarkdownPage,
    pub revision: i64,
    pub workspace_name: String,
    pub workspace_description: String,
    pub structurizr_dsl: String,
    pub uml_artifact_types: Vec<DesignArtifactTypeRecord>,
    pub elements: Vec<DesignElementRecord>,
    pub relationships: Vec<DesignRelationshipRecord>,
    pub diagrams: Vec<DesignDiagramRecord>,
    pub bindings: Vec<DesignBindingRecord>,
    pub mockups: Vec<mockups::MockupSummary>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct DesignScopeResult {
    pub markdown: crate::markdown::MarkdownPage,
    pub backlinks: Vec<crate::markdown::MarkdownBacklink>,
    pub revision: i64,
    pub root_external_id: String,
    pub uml_artifact_types: Vec<DesignArtifactTypeRecord>,
    pub ancestors: Vec<DesignElementRecord>,
    pub elements: Vec<DesignElementRecord>,
    pub relationships: Vec<DesignRelationshipRecord>,
    pub diagrams: Vec<DesignDiagramRecord>,
    pub bindings: Vec<DesignBindingRecord>,
    pub mockups: Vec<mockups::MockupSummary>,
    pub structurizr_dsl: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct DesignSearchResult {
    pub total_count: usize,
    pub truncated: bool,
    pub revision: i64,
    pub hits: Vec<DesignSearchHit>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct DesignSearchHit {
    pub kind: String,
    pub id: String,
    pub title: String,
    pub summary: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct DesignByIdsResult {
    pub markdown: crate::markdown::MarkdownPage,
    pub revision: i64,
    pub uml_artifact_types: Vec<DesignArtifactTypeRecord>,
    pub elements: Vec<DesignElementRecord>,
    pub relationships: Vec<DesignRelationshipRecord>,
    pub diagrams: Vec<DesignDiagramRecord>,
    pub bindings: Vec<DesignBindingRecord>,
    pub mockups: Vec<mockups::MockupSummary>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct DesignBindingsResult {
    pub markdown: crate::markdown::MarkdownPage,
    pub revision: i64,
    pub uml_artifact_types: Vec<DesignArtifactTypeRecord>,
    pub bindings: Vec<DesignBindingRecord>,
    pub elements: Vec<DesignElementRecord>,
    pub relationships: Vec<DesignRelationshipRecord>,
    pub diagrams: Vec<DesignDiagramRecord>,
    pub mockups: Vec<mockups::MockupSummary>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct DesignSaveResult {
    pub ok: bool,
    pub stored: bool,
    pub correction_required: bool,
    pub revision: i64,
    #[schemars(schema_with = "nonnegative_count_schema")]
    pub changed_count: usize,
    pub errors: Vec<DesignCorrection>,
    #[serde(default)]
    pub read_tokens: Vec<documents::DocumentReadToken>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct DesignCorrection {
    pub code: String,
    pub message: String,
    pub request: String,
}
