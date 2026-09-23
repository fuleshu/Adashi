//! Backend-neutral models and shared domain helpers.
pub mod documents;
pub use adashi_storage_api::design::*;

pub fn supported_uml_artifact_types() -> Vec<DesignArtifactTypeRecord> {
    vec![
        artifact_type_record(
            "class",
            "primary-structure",
            "Structure",
            10,
            "classDiagram",
            "Static classes, interfaces, packages, domain contracts, and internal component structure.",
        ),
        artifact_type_record(
            "sequence",
            "interaction",
            "Sequence",
            20,
            "sequenceDiagram",
            "Time-ordered interactions between components, APIs, actors, and storage.",
        ),
        artifact_type_record(
            "flow",
            "workflow",
            "Flow",
            30,
            "flowchart",
            "Workflow, process, decision, and activity-style behavior.",
        ),
        artifact_type_record(
            "state",
            "lifecycle",
            "State",
            40,
            "stateDiagram-v2",
            "Lifecycle states and transitions for stateful components or entities.",
        ),
    ]
}

pub fn diagram_artifact_role(diagram_type: &str) -> &'static str {
    match normalized_diagram_type(diagram_type).as_str() {
        "class" | "classdiagram" | "package" | "component" | "structure" => "primary-structure",
        "sequence" | "sequencediagram" => "interaction",
        "flow" | "flowchart" | "graph" | "activity" => "workflow",
        "state" | "statediagram" | "statediagram-v2" => "lifecycle",
        "structurizr" | "c4" => "architecture-source",
        _ => "reference",
    }
}

pub fn diagram_artifact_label(diagram_type: &str) -> &'static str {
    match diagram_artifact_role(diagram_type) {
        "primary-structure" => "Structure",
        "interaction" => "Sequence",
        "workflow" => "Flow",
        "lifecycle" => "State",
        "architecture-source" => "C4 Source",
        _ => "Artifact",
    }
}

pub fn diagram_artifact_rank(diagram_type: &str) -> i64 {
    match diagram_artifact_role(diagram_type) {
        "primary-structure" => 10,
        "interaction" => 20,
        "workflow" => 30,
        "lifecycle" => 40,
        "architecture-source" => 90,
        _ => 80,
    }
}
#[cfg(test)]
pub(crate) use crate::storage::sqlite::design::*;

pub(crate) fn artifact_type_record(
    diagram_type: &str,
    artifact_role: &str,
    artifact_label: &str,
    artifact_rank: i64,
    mermaid_header: &str,
    description: &str,
) -> DesignArtifactTypeRecord {
    DesignArtifactTypeRecord {
        diagram_type: diagram_type.to_string(),
        artifact_role: artifact_role.to_string(),
        artifact_label: artifact_label.to_string(),
        artifact_rank,
        mermaid_header: mermaid_header.to_string(),
        description: description.to_string(),
    }
}

pub(crate) fn normalized_diagram_type(diagram_type: &str) -> String {
    diagram_type
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || *character == '-')
        .collect()
}
