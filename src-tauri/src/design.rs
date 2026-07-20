use crate::{mockups, state};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(rmcp::schemars::JsonSchema)]
pub struct DesignElementRecord {
    pub external_id: String,
    pub parent_external_id: Option<String>,
    pub element_type: String,
    pub name: String,
    pub description: String,
    pub technology: String,
    pub tags: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(rmcp::schemars::JsonSchema)]
pub struct DesignRelationshipRecord {
    pub external_id: String,
    pub source_external_id: String,
    pub destination_external_id: String,
    pub description: String,
    pub technology: String,
    pub tags: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(rmcp::schemars::JsonSchema)]
pub struct DesignDiagramRecord {
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
#[serde(rename_all = "camelCase")]
#[derive(rmcp::schemars::JsonSchema)]
pub struct DesignArtifactTypeRecord {
    pub diagram_type: String,
    pub artifact_role: String,
    pub artifact_label: String,
    pub artifact_rank: i64,
    pub mermaid_header: String,
    pub description: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(rmcp::schemars::JsonSchema)]
pub struct DesignBindingRecord {
    pub design_external_id: String,
    pub target_type: String,
    pub target: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(rmcp::schemars::JsonSchema)]
pub struct DesignChange {
    pub op: String,
    pub external_id: Option<String>,
    pub parent_external_id: Option<String>,
    pub element_type: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub technology: Option<String>,
    pub tags: Option<String>,
    pub source_external_id: Option<String>,
    pub destination_external_id: Option<String>,
    pub key: Option<String>,
    pub title: Option<String>,
    pub language: Option<String>,
    pub diagram_type: Option<String>,
    pub attached_to_external_id: Option<String>,
    pub source: Option<String>,
    pub design_external_id: Option<String>,
    pub target_type: Option<String>,
    pub target: Option<String>,
    pub viewport_width: Option<i64>,
    pub viewport_height: Option<i64>,
    pub screen: Option<String>,
    pub mockup_state: Option<String>,
    pub fidelity: Option<String>,
    pub schema_version: Option<i64>,
    pub accepted_svg: Option<String>,
    pub base_revision: Option<i64>,
    pub proposed_svg: Option<String>,
    pub proposed_manifest: Option<mockups::MockupManifest>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(rmcp::schemars::JsonSchema)]
pub struct DesignOverviewResult {
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(rmcp::schemars::JsonSchema)]
pub struct DesignScopeResult {
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(rmcp::schemars::JsonSchema)]
pub struct DesignSearchResult {
    pub revision: i64,
    pub hits: Vec<DesignSearchHit>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(rmcp::schemars::JsonSchema)]
pub struct DesignSearchHit {
    pub kind: String,
    pub id: String,
    pub title: String,
    pub summary: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(rmcp::schemars::JsonSchema)]
pub struct DesignByIdsResult {
    pub revision: i64,
    pub uml_artifact_types: Vec<DesignArtifactTypeRecord>,
    pub elements: Vec<DesignElementRecord>,
    pub relationships: Vec<DesignRelationshipRecord>,
    pub diagrams: Vec<DesignDiagramRecord>,
    pub bindings: Vec<DesignBindingRecord>,
    pub mockups: Vec<mockups::MockupSummary>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(rmcp::schemars::JsonSchema)]
pub struct DesignBindingsResult {
    pub revision: i64,
    pub uml_artifact_types: Vec<DesignArtifactTypeRecord>,
    pub bindings: Vec<DesignBindingRecord>,
    pub elements: Vec<DesignElementRecord>,
    pub relationships: Vec<DesignRelationshipRecord>,
    pub diagrams: Vec<DesignDiagramRecord>,
    pub mockups: Vec<mockups::MockupSummary>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(rmcp::schemars::JsonSchema)]
pub struct DesignSaveResult {
    pub ok: bool,
    pub stored: bool,
    pub correction_required: bool,
    pub revision: i64,
    pub errors: Vec<DesignCorrection>,
    pub structurizr_dsl: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(rmcp::schemars::JsonSchema)]
pub struct DesignCorrection {
    pub code: String,
    pub message: String,
    pub request: String,
}

pub fn load_overview(
    db: &Connection,
    project_id: i64,
    max_depth: Option<usize>,
) -> Result<DesignOverviewResult, String> {
    let workspace = load_workspace(db)?;
    let revision = state::load_project_revision(db, project_id)?.revision;
    let elements = filter_elements_by_depth(load_elements(db, workspace.id)?, max_depth);

    Ok(DesignOverviewResult {
        revision,
        workspace_name: workspace.name,
        workspace_description: workspace.description,
        structurizr_dsl: workspace.structurizr_dsl,
        uml_artifact_types: supported_uml_artifact_types(),
        relationships: load_relationships(db, workspace.id)?,
        diagrams: load_diagrams(db, workspace.id)?,
        bindings: load_bindings(db, workspace.id)?,
        mockups: mockups::load_summaries(db, project_id)?,
        elements,
    })
}

pub fn load_scope(
    db: &Connection,
    project_id: i64,
    element_id: &str,
    include_ancestors: bool,
    children_depth: Option<usize>,
    include_source: bool,
) -> Result<DesignScopeResult, String> {
    let workspace = load_workspace(db)?;
    let revision = state::load_project_revision(db, project_id)?.revision;
    let elements = load_elements(db, workspace.id)?;
    let relationships = load_relationships(db, workspace.id)?;
    let diagrams = load_diagrams(db, workspace.id)?;
    let bindings = load_bindings(db, workspace.id)?;
    let element_by_id = elements
        .iter()
        .map(|element| (element.external_id.as_str(), element))
        .collect::<HashMap<_, _>>();

    if !element_by_id.contains_key(element_id) {
        return Err(format!("Unknown design element id: {element_id}"));
    }

    let scope_ids = collect_descendants(&elements, element_id, children_depth);
    let mut scoped_elements = elements
        .iter()
        .filter(|element| scope_ids.contains(element.external_id.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    scoped_elements.sort_by_key(|element| element.external_id.clone());

    let mut ancestor_ids = HashSet::new();
    if include_ancestors {
        let mut current = element_by_id
            .get(element_id)
            .and_then(|element| element.parent_external_id.as_deref());
        while let Some(parent_id) = current {
            ancestor_ids.insert(parent_id.to_string());
            current = element_by_id
                .get(parent_id)
                .and_then(|element| element.parent_external_id.as_deref());
        }
    }

    let ancestors = elements
        .iter()
        .filter(|element| ancestor_ids.contains(&element.external_id))
        .cloned()
        .collect::<Vec<_>>();
    let scoped_relationships = relationships
        .into_iter()
        .filter(|relationship| {
            scope_ids.contains(relationship.source_external_id.as_str())
                || scope_ids.contains(relationship.destination_external_id.as_str())
        })
        .collect::<Vec<_>>();
    let scoped_diagrams = diagrams
        .into_iter()
        .filter(|diagram| {
            diagram
                .attached_to_external_id
                .as_deref()
                .map(|id| scope_ids.contains(id))
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    let scoped_bindings = bindings
        .into_iter()
        .filter(|binding| scope_ids.contains(binding.design_external_id.as_str()))
        .collect::<Vec<_>>();
    let scoped_mockups = mockups::load_summaries(db, project_id)?
        .into_iter()
        .filter(|mockup| scope_ids.contains(mockup.attached_to_external_id.as_str()))
        .collect::<Vec<_>>();

    Ok(DesignScopeResult {
        revision,
        root_external_id: element_id.to_string(),
        uml_artifact_types: supported_uml_artifact_types(),
        ancestors,
        elements: scoped_elements,
        relationships: scoped_relationships,
        diagrams: scoped_diagrams,
        bindings: scoped_bindings,
        mockups: scoped_mockups,
        structurizr_dsl: if include_source {
            Some(workspace.structurizr_dsl)
        } else {
            None
        },
    })
}

pub fn search(
    db: &Connection,
    project_id: i64,
    query: &str,
    kinds: &[String],
    limit: usize,
) -> Result<DesignSearchResult, String> {
    let workspace = load_workspace(db)?;
    let revision = state::load_project_revision(db, project_id)?.revision;
    let terms = query
        .to_lowercase()
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let allowed = kinds
        .iter()
        .map(|kind| kind.as_str())
        .collect::<HashSet<_>>();
    let any_kind = allowed.is_empty();
    let mut hits = Vec::new();

    if any_kind || allowed.contains("element") {
        for element in load_elements(db, workspace.id)? {
            let haystack = format!(
                "{} {} {} {} {}",
                element.external_id,
                element.name,
                element.description,
                element.technology,
                element.tags
            )
            .to_lowercase();
            if matches_terms(&haystack, &terms) {
                hits.push(DesignSearchHit {
                    kind: "element".to_string(),
                    id: element.external_id,
                    title: element.name,
                    summary: element.description,
                });
            }
        }
    }

    if any_kind || allowed.contains("relationship") {
        for relationship in load_relationships(db, workspace.id)? {
            let haystack = format!(
                "{} {} {} {} {} {}",
                relationship.external_id,
                relationship.source_external_id,
                relationship.destination_external_id,
                relationship.description,
                relationship.technology,
                relationship.tags
            )
            .to_lowercase();
            if matches_terms(&haystack, &terms) {
                hits.push(DesignSearchHit {
                    kind: "relationship".to_string(),
                    id: relationship.external_id,
                    title: format!(
                        "{} -> {}",
                        relationship.source_external_id, relationship.destination_external_id
                    ),
                    summary: relationship.description,
                });
            }
        }
    }

    if any_kind || allowed.contains("uml") || allowed.contains("source") {
        for diagram in load_diagrams(db, workspace.id)? {
            let haystack = format!(
                "{} {} {} {} {}",
                diagram.key, diagram.language, diagram.title, diagram.diagram_type, diagram.source
            )
            .to_lowercase();
            if matches_terms(&haystack, &terms) {
                hits.push(DesignSearchHit {
                    kind: "uml".to_string(),
                    id: diagram.key,
                    title: diagram.title,
                    summary: diagram
                        .attached_to_external_id
                        .unwrap_or_else(|| "unattached".to_string()),
                });
            }
        }
    }

    if any_kind || allowed.contains("mockup") {
        for mockup in mockups::load_summaries(db, project_id)? {
            let haystack = format!(
                "{} {} {} {} {} {}",
                mockup.external_id,
                mockup.title,
                mockup.attached_to_external_id,
                mockup.screen,
                mockup.state,
                mockup.fidelity
            )
            .to_lowercase();
            if matches_terms(&haystack, &terms) {
                hits.push(DesignSearchHit {
                    kind: "mockup".to_string(),
                    id: mockup.external_id,
                    title: mockup.title,
                    summary: format!("{} - {}", mockup.status, mockup.attached_to_external_id),
                });
            }
        }
    }

    hits.truncate(limit.max(1));
    Ok(DesignSearchResult { revision, hits })
}

pub fn load_by_ids(
    db: &Connection,
    project_id: i64,
    ids: &[String],
) -> Result<DesignByIdsResult, String> {
    let workspace = load_workspace(db)?;
    let revision = state::load_project_revision(db, project_id)?.revision;
    let ids = ids.iter().map(String::as_str).collect::<HashSet<_>>();

    Ok(DesignByIdsResult {
        revision,
        uml_artifact_types: supported_uml_artifact_types(),
        elements: load_elements(db, workspace.id)?
            .into_iter()
            .filter(|element| ids.contains(element.external_id.as_str()))
            .collect(),
        relationships: load_relationships(db, workspace.id)?
            .into_iter()
            .filter(|relationship| ids.contains(relationship.external_id.as_str()))
            .collect(),
        diagrams: load_diagrams(db, workspace.id)?
            .into_iter()
            .filter(|diagram| ids.contains(diagram.key.as_str()))
            .collect(),
        bindings: load_bindings(db, workspace.id)?
            .into_iter()
            .filter(|binding| ids.contains(binding.design_external_id.as_str()))
            .collect(),
        mockups: mockups::load_summaries(db, project_id)?
            .into_iter()
            .filter(|mockup| ids.contains(mockup.external_id.as_str()))
            .collect(),
    })
}

pub fn load_by_bindings(
    db: &Connection,
    project_id: i64,
    files: &[String],
    symbols: &[String],
) -> Result<DesignBindingsResult, String> {
    let workspace = load_workspace(db)?;
    let revision = state::load_project_revision(db, project_id)?.revision;
    let file_set = files.iter().map(String::as_str).collect::<HashSet<_>>();
    let symbol_set = symbols.iter().map(String::as_str).collect::<HashSet<_>>();
    let bindings = load_bindings(db, workspace.id)?
        .into_iter()
        .filter(|binding| {
            (binding.target_type == "file" && file_set.contains(binding.target.as_str()))
                || (binding.target_type == "symbol" && symbol_set.contains(binding.target.as_str()))
        })
        .collect::<Vec<_>>();
    let design_ids = bindings
        .iter()
        .map(|binding| binding.design_external_id.as_str())
        .collect::<HashSet<_>>();

    Ok(DesignBindingsResult {
        revision,
        uml_artifact_types: supported_uml_artifact_types(),
        elements: load_elements(db, workspace.id)?
            .into_iter()
            .filter(|element| design_ids.contains(element.external_id.as_str()))
            .collect(),
        relationships: load_relationships(db, workspace.id)?
            .into_iter()
            .filter(|relationship| design_ids.contains(relationship.external_id.as_str()))
            .collect(),
        diagrams: load_diagrams(db, workspace.id)?
            .into_iter()
            .filter(|diagram| design_ids.contains(diagram.key.as_str()))
            .collect(),
        mockups: mockups::load_summaries(db, project_id)?
            .into_iter()
            .filter(|mockup| design_ids.contains(mockup.external_id.as_str()))
            .collect(),
        bindings,
    })
}

pub fn save_changes(
    db: &mut Connection,
    project_id: i64,
    expected_revision: i64,
    change_intent: &str,
    changes: &[DesignChange],
) -> Result<DesignSaveResult, String> {
    let current_revision = state::load_project_revision(db, project_id)?.revision;
    if current_revision != expected_revision {
        return Ok(failed_save(
            current_revision,
            "revision.stale",
            format!("Expected revision {expected_revision}, but current revision is {current_revision}."),
            "Reload the design overview and resubmit the full changeset against the current revision.",
        ));
    }

    if change_intent.trim().is_empty() {
        return Ok(failed_save(
            current_revision,
            "save.missing_intent",
            "Design save requires a non-empty changeIntent.",
            "Describe the design change intent and resubmit the full changeset.",
        ));
    }

    if changes.is_empty() {
        return Ok(failed_save(
            current_revision,
            "save.empty_changeset",
            "Design save requires at least one change.",
            "Submit the C4, UML, or binding changes that make up the completed design update.",
        ));
    }

    let tx = db.transaction().map_err(|err| err.to_string())?;
    let workspace = load_workspace(&tx)?;

    for change in changes {
        if let Err(message) = apply_change(&tx, project_id, workspace.id, change) {
            return Ok(failed_save(
                current_revision,
                "save.invalid_changeset",
                message,
                "Correct the design_save changeset fields and resubmit the full changeset.",
            ));
        }
    }

    let mut errors = validate_workspace(&tx, workspace.id)?;
    if errors.is_empty() {
        let dsl = build_structurizr_dsl(&tx, workspace.id)?;
        let json_source = build_structurizr_json_source(&tx, workspace.id)?;
        tx.execute(
            "UPDATE design_workspaces
             SET structurizr_dsl = ?1,
                 structurizr_json = ?2,
                 updated_at = CURRENT_TIMESTAMP
             WHERE id = ?3",
            params![dsl, json_source, workspace.id],
        )
        .map_err(|err| err.to_string())?;
        tx.execute(
            "UPDATE diagrams
             SET source = ?1,
                 updated_at = CURRENT_TIMESTAMP
             WHERE workspace_id = ?2 AND kind = 'structurizr'",
            params![json_source, workspace.id],
        )
        .map_err(|err| err.to_string())?;
        state::bump_project_revision(&tx, project_id)?;
        let revision = state::load_project_revision(&tx, project_id)?.revision;
        tx.commit().map_err(|err| err.to_string())?;

        Ok(DesignSaveResult {
            ok: true,
            stored: true,
            correction_required: false,
            revision,
            errors,
            structurizr_dsl: Some(dsl),
        })
    } else {
        errors.sort_by(|left, right| left.code.cmp(&right.code));
        Ok(DesignSaveResult {
            ok: false,
            stored: false,
            correction_required: true,
            revision: current_revision,
            errors,
            structurizr_dsl: None,
        })
    }
}

fn apply_change(
    db: &Connection,
    project_id: i64,
    workspace_id: i64,
    change: &DesignChange,
) -> Result<(), String> {
    match change.op.as_str() {
        "upsert_element" => {
            let external_id = required(change.external_id.as_deref(), "externalId")?;
            let element_type = required(change.element_type.as_deref(), "elementType")?;
            let name = required(change.name.as_deref(), "name")?;
            db.execute(
                "INSERT INTO c4_elements(
                    workspace_id, external_id, parent_external_id, element_type, name, description, technology, tags
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(workspace_id, external_id) DO UPDATE SET
                    parent_external_id = excluded.parent_external_id,
                    element_type = excluded.element_type,
                    name = excluded.name,
                    description = excluded.description,
                    technology = excluded.technology,
                    tags = excluded.tags",
                params![
                    workspace_id,
                    external_id,
                    optional_trim(change.parent_external_id.as_deref()),
                    element_type.trim(),
                    name.trim(),
                    change.description.as_deref().unwrap_or("").trim(),
                    change.technology.as_deref().unwrap_or("").trim(),
                    change.tags.as_deref().unwrap_or("").trim(),
                ],
            )
            .map_err(|err| err.to_string())?;
        }
        "upsert_relationship" => {
            let external_id = required(change.external_id.as_deref(), "externalId")?;
            let source_id = required(change.source_external_id.as_deref(), "sourceExternalId")?;
            let destination_id = required(
                change.destination_external_id.as_deref(),
                "destinationExternalId",
            )?;
            let description = required(change.description.as_deref(), "description")?;
            db.execute(
                "INSERT INTO c4_relationships(
                    workspace_id, external_id, source_external_id, destination_external_id, description, technology, tags
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(workspace_id, external_id) DO UPDATE SET
                    source_external_id = excluded.source_external_id,
                    destination_external_id = excluded.destination_external_id,
                    description = excluded.description,
                    technology = excluded.technology,
                    tags = excluded.tags",
                params![
                    workspace_id,
                    external_id,
                    source_id.trim(),
                    destination_id.trim(),
                    description.trim(),
                    change.technology.as_deref().unwrap_or("").trim(),
                    change.tags.as_deref().unwrap_or("").trim(),
                ],
            )
            .map_err(|err| err.to_string())?;
        }
        "upsert_uml" => {
            let key = required(change.key.as_deref(), "key")?;
            let title = required(change.title.as_deref(), "title")?;
            let language = required(change.language.as_deref(), "language")?;
            let source = required(change.source.as_deref(), "source")?;
            db.execute(
                "INSERT INTO diagrams(
                    workspace_id, kind, key, title, source, diagram_type, attached_to_external_id, sort_order
                 )
                 VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7,
                    COALESCE((SELECT MAX(sort_order) + 1 FROM diagrams WHERE workspace_id = ?1), 1)
                 )
                 ON CONFLICT(workspace_id, key) DO UPDATE SET
                    kind = excluded.kind,
                    title = excluded.title,
                    source = excluded.source,
                    diagram_type = excluded.diagram_type,
                    attached_to_external_id = excluded.attached_to_external_id,
                    updated_at = CURRENT_TIMESTAMP",
                params![
                    workspace_id,
                    language.trim(),
                    key.trim(),
                    title.trim(),
                    source,
                    change.diagram_type.as_deref().unwrap_or("").trim(),
                    optional_trim(change.attached_to_external_id.as_deref()),
                ],
            )
            .map_err(|err| err.to_string())?;
        }
        "upsert_binding" => {
            let design_external_id =
                required(change.design_external_id.as_deref(), "designExternalId")?;
            let target_type = required(change.target_type.as_deref(), "targetType")?;
            let target = required(change.target.as_deref(), "target")?;
            db.execute(
                "INSERT INTO design_bindings(workspace_id, design_external_id, target_type, target)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(workspace_id, design_external_id, target_type, target) DO UPDATE SET
                    updated_at = CURRENT_TIMESTAMP",
                params![
                    workspace_id,
                    design_external_id.trim(),
                    target_type.trim(),
                    target.trim()
                ],
            )
            .map_err(|err| err.to_string())?;
        }
        "upsert_mockup" => {
            let input = mockups::CreateMockupInput {
                external_id: required(change.external_id.as_deref(), "externalId")?.to_string(),
                title: required(change.title.as_deref(), "title")?.to_string(),
                attached_to_external_id: required(
                    change.attached_to_external_id.as_deref(),
                    "attachedToExternalId",
                )?
                .to_string(),
                viewport_width: change
                    .viewport_width
                    .ok_or("Design save field 'viewportWidth' is required")?,
                viewport_height: change
                    .viewport_height
                    .ok_or("Design save field 'viewportHeight' is required")?,
                screen: change.screen.clone().unwrap_or_default(),
                state: change.mockup_state.clone().unwrap_or_default(),
                fidelity: change.fidelity.clone().unwrap_or_default(),
                schema_version: change.schema_version,
                accepted_svg: required(change.accepted_svg.as_deref(), "acceptedSvg")?.to_string(),
                expected_revision: 0,
            };
            mockups::upsert_initial_in_transaction(db, project_id, &input)?;
        }
        "upsert_mockup_proposal" => {
            let input = mockups::ProposeMockupInput {
                external_id: required(change.external_id.as_deref(), "externalId")?.to_string(),
                base_revision: change
                    .base_revision
                    .ok_or("Design save field 'baseRevision' is required")?,
                proposed_svg: required(change.proposed_svg.as_deref(), "proposedSvg")?.to_string(),
                proposed_manifest: change
                    .proposed_manifest
                    .clone()
                    .ok_or("Design save field 'proposedManifest' is required")?,
                expected_revision: 0,
            };
            mockups::upsert_proposal_in_transaction(db, project_id, &input)?;
        }
        "delete_element" => {
            let external_id = required(change.external_id.as_deref(), "externalId")?;
            delete_element_subtree(db, workspace_id, external_id.trim())?;
        }
        "delete_relationship" => {
            let external_id = required(change.external_id.as_deref(), "externalId")?;
            delete_relationship(db, workspace_id, external_id.trim(), true)?;
        }
        "delete_uml" => {
            let key = required(change.key.as_deref(), "key")?;
            delete_uml_artifact(db, workspace_id, key.trim(), true)?;
        }
        "delete_binding" => {
            let design_external_id =
                required(change.design_external_id.as_deref(), "designExternalId")?;
            let target_type = required(change.target_type.as_deref(), "targetType")?;
            let target = required(change.target.as_deref(), "target")?;
            delete_binding(
                db,
                workspace_id,
                design_external_id.trim(),
                target_type.trim(),
                target.trim(),
            )?;
        }
        "delete_mockup" => {
            let external_id = required(change.external_id.as_deref(), "externalId")?;
            mockups::delete_in_transaction(db, project_id, external_id.trim(), true)?;
        }
        _ => return Err(format!("Unknown design save op: {}", change.op)),
    }

    Ok(())
}

fn delete_element_subtree(
    db: &Connection,
    workspace_id: i64,
    external_id: &str,
) -> Result<(), String> {
    let elements = load_elements(db, workspace_id)?;
    if !elements
        .iter()
        .any(|element| element.external_id == external_id)
    {
        return Err(format!("Cannot delete unknown C4 element '{external_id}'."));
    }

    let element_ids = collect_descendants(&elements, external_id, None);
    let relationships = load_relationships(db, workspace_id)?;
    let relationship_ids = relationships
        .iter()
        .filter(|relationship| {
            element_ids.contains(&relationship.source_external_id)
                || element_ids.contains(&relationship.destination_external_id)
        })
        .map(|relationship| relationship.external_id.clone())
        .collect::<Vec<_>>();

    for relationship_id in relationship_ids {
        delete_relationship(db, workspace_id, &relationship_id, false)?;
    }

    let element_ids = element_ids.into_iter().collect::<Vec<_>>();
    for element_id in &element_ids {
        delete_attached_uml_artifacts(db, workspace_id, element_id)?;
        delete_attached_mockups(db, workspace_id, element_id)?;
        delete_bindings_for_design_id(db, workspace_id, element_id)?;
    }

    for element_id in element_ids {
        db.execute(
            "DELETE FROM c4_elements
             WHERE workspace_id = ?1 AND external_id = ?2",
            params![workspace_id, element_id],
        )
        .map_err(|err| err.to_string())?;
    }

    Ok(())
}

fn delete_relationship(
    db: &Connection,
    workspace_id: i64,
    external_id: &str,
    require_existing: bool,
) -> Result<(), String> {
    let deleted = db
        .execute(
            "DELETE FROM c4_relationships
             WHERE workspace_id = ?1 AND external_id = ?2",
            params![workspace_id, external_id],
        )
        .map_err(|err| err.to_string())?;
    if require_existing && deleted == 0 {
        return Err(format!(
            "Cannot delete unknown C4 relationship '{external_id}'."
        ));
    }

    delete_attached_uml_artifacts(db, workspace_id, external_id)?;
    delete_attached_mockups(db, workspace_id, external_id)?;
    delete_bindings_for_design_id(db, workspace_id, external_id)?;

    Ok(())
}

fn delete_attached_mockups(
    db: &Connection,
    workspace_id: i64,
    design_external_id: &str,
) -> Result<(), String> {
    db.execute("DELETE FROM ui_mockups WHERE project_id=(SELECT project_id FROM design_workspaces WHERE id=?1) AND attached_to_external_id=?2",
        params![workspace_id, design_external_id]).map_err(|err| err.to_string())?;
    Ok(())
}

fn delete_uml_artifact(
    db: &Connection,
    workspace_id: i64,
    key: &str,
    require_existing: bool,
) -> Result<(), String> {
    let kind = db
        .query_row(
            "SELECT kind
             FROM diagrams
             WHERE workspace_id = ?1 AND key = ?2",
            params![workspace_id, key],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|err| err.to_string())?;

    let Some(kind) = kind else {
        if require_existing {
            return Err(format!("Cannot delete unknown UML artifact '{key}'."));
        }
        return Ok(());
    };

    if kind == "structurizr" {
        return Err(format!(
            "Cannot delete generated Structurizr artifact '{key}' through delete_uml."
        ));
    }

    db.execute(
        "DELETE FROM diagrams
         WHERE workspace_id = ?1 AND key = ?2",
        params![workspace_id, key],
    )
    .map_err(|err| err.to_string())?;
    delete_bindings_for_design_id(db, workspace_id, key)?;

    Ok(())
}

fn delete_binding(
    db: &Connection,
    workspace_id: i64,
    design_external_id: &str,
    target_type: &str,
    target: &str,
) -> Result<(), String> {
    let deleted = db
        .execute(
            "DELETE FROM design_bindings
             WHERE workspace_id = ?1
                AND design_external_id = ?2
                AND target_type = ?3
                AND target = ?4",
            params![workspace_id, design_external_id, target_type, target],
        )
        .map_err(|err| err.to_string())?;
    if deleted == 0 {
        return Err(format!(
            "Cannot delete unknown binding '{design_external_id}' -> {target_type} '{target}'."
        ));
    }

    Ok(())
}

fn delete_attached_uml_artifacts(
    db: &Connection,
    workspace_id: i64,
    design_external_id: &str,
) -> Result<(), String> {
    let keys = {
        let mut statement = db
            .prepare(
                "SELECT key
                 FROM diagrams
                 WHERE workspace_id = ?1
                    AND kind != 'structurizr'
                    AND attached_to_external_id = ?2",
            )
            .map_err(|err| err.to_string())?;
        let rows = statement
            .query_map(params![workspace_id, design_external_id], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|err| err.to_string())?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|err| err.to_string())?
    };

    for key in keys {
        delete_uml_artifact(db, workspace_id, &key, false)?;
    }

    Ok(())
}

fn delete_bindings_for_design_id(
    db: &Connection,
    workspace_id: i64,
    design_external_id: &str,
) -> Result<(), String> {
    db.execute(
        "DELETE FROM design_bindings
         WHERE workspace_id = ?1 AND design_external_id = ?2",
        params![workspace_id, design_external_id],
    )
    .map_err(|err| err.to_string())?;

    Ok(())
}

fn validate_workspace(db: &Connection, workspace_id: i64) -> Result<Vec<DesignCorrection>, String> {
    let elements = load_elements(db, workspace_id)?;
    let relationships = load_relationships(db, workspace_id)?;
    let diagrams = load_diagrams(db, workspace_id)?;
    let bindings = load_bindings(db, workspace_id)?;
    let project_id: i64 = db
        .query_row(
            "SELECT project_id FROM design_workspaces WHERE id=?1",
            params![workspace_id],
            |row| row.get(0),
        )
        .map_err(|err| err.to_string())?;
    let mockup_ids = mockups::load_summaries(db, project_id)?
        .into_iter()
        .map(|mockup| mockup.external_id)
        .collect::<HashSet<_>>();
    let mut errors = Vec::new();
    let element_by_id = elements
        .iter()
        .map(|element| (element.external_id.as_str(), element))
        .collect::<HashMap<_, _>>();
    let relationship_ids = relationships
        .iter()
        .map(|relationship| relationship.external_id.as_str())
        .collect::<HashSet<_>>();

    let internal_systems = elements
        .iter()
        .filter(|element| {
            element.element_type.eq_ignore_ascii_case("Software System")
                && !has_tag(&element.tags, "External")
        })
        .count();
    if internal_systems != 1 {
        errors.push(correction(
            "c4.system_root_count",
            format!("Expected exactly one internal Software System, found {internal_systems}."),
            "Resubmit the model with one internal top-level software system and mark other systems as External if needed.",
        ));
    }

    for element in &elements {
        if element.external_id.trim().is_empty() {
            errors.push(correction(
                "c4.empty_element_id",
                "A C4 element has an empty external id.",
                "Assign every element a stable non-empty externalId.",
            ));
        }

        if element.name.trim().is_empty() {
            errors.push(correction(
                "c4.empty_element_name",
                format!("Element '{}' has no name.", element.external_id),
                "Provide a human-readable name for every C4 element.",
            ));
        }

        match normalized_element_type(&element.element_type).as_deref() {
            Some("Person") | Some("Software System") => {
                if element.parent_external_id.is_some() {
                    errors.push(correction(
                        "c4.invalid_parent",
                        format!("Top-level element '{}' must not have a parent.", element.external_id),
                        "Move this element to the top level or change it to a valid contained C4 type.",
                    ));
                }
            }
            Some("Container") => {
                validate_parent_type(&mut errors, &element_by_id, element, "Software System")
            }
            Some("Component") => {
                validate_parent_type(&mut errors, &element_by_id, element, "Container")
            }
            _ => errors.push(correction(
                "c4.invalid_element_type",
                format!(
                    "Element '{}' has unsupported type '{}'.",
                    element.external_id, element.element_type
                ),
                "Use one of: Person, Software System, Container, Component.",
            )),
        }
    }

    for relationship in &relationships {
        if relationship.external_id.trim().is_empty() {
            errors.push(correction(
                "c4.empty_relationship_id",
                "A C4 relationship has an empty external id.",
                "Assign every relationship a stable non-empty externalId.",
            ));
        }
        if relationship.description.trim().is_empty() {
            errors.push(correction(
                "c4.empty_relationship_description",
                format!(
                    "Relationship '{}' has no description.",
                    relationship.external_id
                ),
                "Describe the relationship and resubmit the full changeset.",
            ));
        }
        if !element_by_id.contains_key(relationship.source_external_id.as_str()) {
            errors.push(correction(
                "c4.unresolved_relationship_source",
                format!(
                    "Relationship '{}' references unknown source '{}'.",
                    relationship.external_id, relationship.source_external_id
                ),
                "Create the source element or correct sourceExternalId.",
            ));
        }
        if !element_by_id.contains_key(relationship.destination_external_id.as_str()) {
            errors.push(correction(
                "c4.unresolved_relationship_destination",
                format!(
                    "Relationship '{}' references unknown destination '{}'.",
                    relationship.external_id, relationship.destination_external_id
                ),
                "Create the destination element or correct destinationExternalId.",
            ));
        }
        if relationship.source_external_id == relationship.destination_external_id {
            errors.push(correction(
                "c4.self_relationship",
                format!(
                    "Relationship '{}' connects an element to itself.",
                    relationship.external_id
                ),
                "Connect two different elements or remove the relationship.",
            ));
        }
    }

    for diagram in &diagrams {
        if diagram.language == "structurizr" {
            continue;
        }
        if !diagram.language.eq_ignore_ascii_case("mermaid") {
            errors.push(correction(
                "uml.unsupported_language",
                format!(
                    "Diagram '{}' uses unsupported language '{}'.",
                    diagram.key, diagram.language
                ),
                "Use Mermaid UML for stored UML artifacts.",
            ));
            continue;
        }
        if let Some(error) = validate_mermaid(&diagram.source) {
            errors.push(correction(
                "uml.syntax_error",
                format!("Diagram '{}': {error}", diagram.key),
                "Correct the Mermaid UML source and resubmit the full diagram.",
            ));
        }
        match diagram.attached_to_external_id.as_deref() {
            Some(id) if element_by_id.contains_key(id) || relationship_ids.contains(id) => {}
            Some(id) => errors.push(correction(
                "uml.invalid_attachment",
                format!(
                    "Diagram '{}' is attached to unknown design id '{}'.",
                    diagram.key, id
                ),
                "Attach the UML artifact to an existing C4 element or relationship id.",
            )),
            None => errors.push(correction(
                "uml.missing_attachment",
                format!(
                    "Diagram '{}' is not attached to a C4 element or relationship.",
                    diagram.key
                ),
                "Attach the UML artifact to the C4 element or relationship it specifies.",
            )),
        }
    }

    for binding in &bindings {
        if binding.target_type != "file" && binding.target_type != "symbol" {
            errors.push(correction(
                "binding.invalid_target_type",
                format!(
                    "Binding '{}' uses invalid target type '{}'.",
                    binding.design_external_id, binding.target_type
                ),
                "Use targetType 'file' or 'symbol'.",
            ));
        }
        if !element_by_id.contains_key(binding.design_external_id.as_str())
            && !relationship_ids.contains(binding.design_external_id.as_str())
            && !diagrams
                .iter()
                .any(|diagram| diagram.key == binding.design_external_id)
            && !mockup_ids.contains(&binding.design_external_id)
        {
            errors.push(correction(
                "binding.unknown_design_id",
                format!(
                    "Binding points to unknown design id '{}'.",
                    binding.design_external_id
                ),
                "Bind files or symbols only to existing element, relationship, or UML ids.",
            ));
        }
    }

    Ok(errors)
}

fn validate_parent_type(
    errors: &mut Vec<DesignCorrection>,
    element_by_id: &HashMap<&str, &DesignElementRecord>,
    element: &DesignElementRecord,
    required_type: &str,
) {
    let Some(parent_id) = element.parent_external_id.as_deref() else {
        errors.push(correction(
            "c4.missing_parent",
            format!("Element '{}' has no parent.", element.external_id),
            format!("Set parentExternalId to a valid {required_type} id."),
        ));
        return;
    };

    let Some(parent) = element_by_id.get(parent_id) else {
        errors.push(correction(
            "c4.missing_parent",
            format!(
                "Element '{}' references unknown parent '{}'.",
                element.external_id, parent_id
            ),
            "Create the parent element or correct parentExternalId.",
        ));
        return;
    };

    if normalized_element_type(&parent.element_type).as_deref() != Some(required_type) {
        errors.push(correction(
            "c4.invalid_parent_type",
            format!(
                "Element '{}' is a '{}' but parent '{}' is '{}'.",
                element.external_id, element.element_type, parent.external_id, parent.element_type
            ),
            format!("Move this element under a {required_type}."),
        ));
    }
}

fn validate_mermaid(source: &str) -> Option<String> {
    let trimmed = source.trim();
    if trimmed.is_empty() {
        return Some("source is empty".to_string());
    }

    let first_line = trimmed.lines().next().unwrap_or("").trim();
    let is_sequence_diagram = first_line.starts_with("sequenceDiagram");
    let valid_start = first_line.starts_with("sequenceDiagram")
        || first_line.starts_with("flowchart ")
        || first_line.starts_with("graph ")
        || first_line.starts_with("classDiagram")
        || first_line.starts_with("stateDiagram")
        || first_line.starts_with("erDiagram")
        || first_line.starts_with("journey")
        || first_line.starts_with("gantt");
    if !valid_start {
        return Some(format!(
            "first line '{first_line}' is not a supported Mermaid UML diagram header"
        ));
    }

    for (open, close) in [('(', ')'), ('[', ']'), ('{', '}')] {
        let opens = trimmed
            .chars()
            .filter(|character| *character == open)
            .count();
        let closes = trimmed
            .chars()
            .filter(|character| *character == close)
            .count();
        if opens != closes {
            return Some(format!("unbalanced '{open}' and '{close}' delimiters"));
        }
    }

    if is_sequence_diagram {
        if let Some(error) = validate_sequence_diagram_statement_separators(trimmed) {
            return Some(error);
        }
    }

    None
}

fn validate_sequence_diagram_statement_separators(source: &str) -> Option<String> {
    for (line_index, line) in source.lines().enumerate() {
        let mut start_index = 0;

        while let Some(relative_index) = line[start_index..].find(';') {
            let semicolon_index = start_index + relative_index;
            if is_mermaid_semicolon_entity(line, semicolon_index) {
                start_index = semicolon_index + 1;
                continue;
            }

            let following_statement = line[semicolon_index + 1..]
                .split(';')
                .next()
                .unwrap_or("")
                .trim();
            if !following_statement.is_empty()
                && !looks_like_sequence_diagram_statement(following_statement)
            {
                return Some(format!(
                    "line {} contains a raw semicolon in sequence text; Mermaid treats ';' as a statement separator, so replace it with a period or escape it as #59;",
                    line_index + 1
                ));
            }

            start_index = semicolon_index + 1;
        }
    }

    None
}

fn is_mermaid_semicolon_entity(line: &str, semicolon_index: usize) -> bool {
    let prefix = &line[..semicolon_index];
    prefix.ends_with("#59") || prefix.ends_with("&#59")
}

fn looks_like_sequence_diagram_statement(statement: &str) -> bool {
    let lower = statement.to_ascii_lowercase();
    let keyword = lower.split_whitespace().next().unwrap_or("");
    matches!(
        keyword,
        "actor"
            | "and"
            | "alt"
            | "autonumber"
            | "activate"
            | "box"
            | "break"
            | "critical"
            | "create"
            | "deactivate"
            | "destroy"
            | "else"
            | "end"
            | "loop"
            | "note"
            | "opt"
            | "option"
            | "par"
            | "participant"
            | "rect"
    ) || statement.contains("->")
        || statement.contains("-->")
        || statement.contains("-->>")
        || statement.contains("-x")
        || statement.contains("--x")
        || statement.contains("-)")
        || statement.contains("--)")
}

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

fn artifact_type_record(
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

fn normalized_diagram_type(diagram_type: &str) -> String {
    diagram_type
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || *character == '-')
        .collect()
}

fn load_workspace(db: &Connection) -> Result<WorkspaceRecord, String> {
    db.query_row(
        "SELECT id, name, description, structurizr_dsl
         FROM design_workspaces
         ORDER BY id
         LIMIT 1",
        [],
        |row| {
            Ok(WorkspaceRecord {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                structurizr_dsl: row.get(3)?,
            })
        },
    )
    .map_err(|err| err.to_string())
}

fn load_elements(db: &Connection, workspace_id: i64) -> Result<Vec<DesignElementRecord>, String> {
    let mut statement = db
        .prepare(
            "SELECT external_id, parent_external_id, element_type, name, description, technology, tags
             FROM c4_elements
             WHERE workspace_id = ?1
             ORDER BY parent_external_id IS NOT NULL, parent_external_id, id",
        )
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map(params![workspace_id], |row| {
            Ok(DesignElementRecord {
                external_id: row.get(0)?,
                parent_external_id: row.get(1)?,
                element_type: row.get(2)?,
                name: row.get(3)?,
                description: row.get(4)?,
                technology: row.get(5)?,
                tags: row.get(6)?,
            })
        })
        .map_err(|err| err.to_string())?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| err.to_string())
}

fn load_relationships(
    db: &Connection,
    workspace_id: i64,
) -> Result<Vec<DesignRelationshipRecord>, String> {
    let mut statement = db
        .prepare(
            "SELECT external_id, source_external_id, destination_external_id, description, technology, tags
             FROM c4_relationships
             WHERE workspace_id = ?1
             ORDER BY id",
        )
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map(params![workspace_id], |row| {
            Ok(DesignRelationshipRecord {
                external_id: row.get(0)?,
                source_external_id: row.get(1)?,
                destination_external_id: row.get(2)?,
                description: row.get(3)?,
                technology: row.get(4)?,
                tags: row.get(5)?,
            })
        })
        .map_err(|err| err.to_string())?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| err.to_string())
}

fn load_diagrams(db: &Connection, workspace_id: i64) -> Result<Vec<DesignDiagramRecord>, String> {
    let mut statement = db
        .prepare(
            "SELECT
                d.kind,
                d.key,
                d.title,
                d.source,
                d.diagram_type,
                d.attached_to_external_id,
                CASE
                    WHEN e.external_id IS NOT NULL THEN 'element'
                    WHEN r.external_id IS NOT NULL THEN 'relationship'
                    ELSE NULL
                END AS attached_to_target_type,
                d.sort_order
             FROM diagrams d
             LEFT JOIN c4_elements e
                ON e.workspace_id = d.workspace_id
                AND e.external_id = d.attached_to_external_id
             LEFT JOIN c4_relationships r
                ON r.workspace_id = d.workspace_id
                AND r.external_id = d.attached_to_external_id
             WHERE d.workspace_id = ?1
             ORDER BY d.sort_order, d.id",
        )
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map(params![workspace_id], |row| {
            let diagram_type: String = row.get(4)?;
            Ok(DesignDiagramRecord {
                language: row.get(0)?,
                key: row.get(1)?,
                title: row.get(2)?,
                source: row.get(3)?,
                artifact_role: diagram_artifact_role(&diagram_type).to_string(),
                artifact_label: diagram_artifact_label(&diagram_type).to_string(),
                artifact_rank: diagram_artifact_rank(&diagram_type),
                diagram_type,
                attached_to_external_id: row.get(5)?,
                attached_to_target_type: row.get(6)?,
                sort_order: row.get(7)?,
            })
        })
        .map_err(|err| err.to_string())?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| err.to_string())
}

fn load_bindings(db: &Connection, workspace_id: i64) -> Result<Vec<DesignBindingRecord>, String> {
    let mut statement = db
        .prepare(
            "SELECT design_external_id, target_type, target
             FROM design_bindings
             WHERE workspace_id = ?1
             ORDER BY target_type, target, design_external_id",
        )
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map(params![workspace_id], |row| {
            Ok(DesignBindingRecord {
                design_external_id: row.get(0)?,
                target_type: row.get(1)?,
                target: row.get(2)?,
            })
        })
        .map_err(|err| err.to_string())?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| err.to_string())
}

fn filter_elements_by_depth(
    elements: Vec<DesignElementRecord>,
    max_depth: Option<usize>,
) -> Vec<DesignElementRecord> {
    let Some(max_depth) = max_depth else {
        return elements;
    };
    let element_by_id = elements
        .iter()
        .map(|element| (element.external_id.as_str(), element))
        .collect::<HashMap<_, _>>();

    elements
        .iter()
        .filter(|element| element_depth(element, &element_by_id) <= max_depth)
        .cloned()
        .collect()
}

fn element_depth(
    element: &DesignElementRecord,
    element_by_id: &HashMap<&str, &DesignElementRecord>,
) -> usize {
    let mut depth = 0;
    let mut current = element.parent_external_id.as_deref();

    while let Some(parent_id) = current {
        depth += 1;
        current = element_by_id
            .get(parent_id)
            .and_then(|parent| parent.parent_external_id.as_deref());
    }

    depth
}

fn collect_descendants(
    elements: &[DesignElementRecord],
    root_id: &str,
    children_depth: Option<usize>,
) -> HashSet<String> {
    let max_depth = children_depth.unwrap_or(usize::MAX);
    let mut ids = HashSet::new();
    ids.insert(root_id.to_string());

    for depth in 0..max_depth {
        let current_ids = ids.clone();
        let mut added = false;
        for element in elements {
            if element
                .parent_external_id
                .as_deref()
                .map(|parent| current_ids.contains(parent))
                .unwrap_or(false)
                && ids.insert(element.external_id.clone())
            {
                added = true;
            }
        }
        if !added || depth == max_depth {
            break;
        }
    }

    ids
}

fn matches_terms(haystack: &str, terms: &[String]) -> bool {
    if terms.is_empty() {
        return true;
    }

    terms.iter().all(|term| haystack.contains(term))
}

fn build_structurizr_dsl(db: &Connection, workspace_id: i64) -> Result<String, String> {
    let workspace = load_workspace(db)?;
    let elements = load_elements(db, workspace_id)?;
    let relationships = load_relationships(db, workspace_id)?;
    let children_by_parent = group_children(&elements);
    let root_system = elements
        .iter()
        .find(|element| {
            element.element_type.eq_ignore_ascii_case("Software System")
                && !has_tag(&element.tags, "External")
        })
        .or_else(|| {
            elements
                .iter()
                .find(|element| element.parent_external_id.is_none())
        });

    let mut dsl = String::new();
    dsl.push_str(&format!(
        "workspace \"{}\" \"{}\" {{\n",
        escape_dsl(&workspace.name),
        escape_dsl(&workspace.description)
    ));
    dsl.push_str("    model {\n");

    for element in elements
        .iter()
        .filter(|element| element.parent_external_id.is_none())
    {
        push_element_dsl(&mut dsl, element, &children_by_parent, 2);
    }

    dsl.push('\n');
    for relationship in &relationships {
        dsl.push_str(&format!(
            "        {} -> {} \"{}\" \"{}\"{}\n",
            dsl_identifier(&relationship.source_external_id),
            dsl_identifier(&relationship.destination_external_id),
            escape_dsl(&relationship.description),
            escape_dsl(&relationship.technology),
            dsl_tags(&relationship.tags)
        ));
    }

    dsl.push_str("    }\n\n");
    dsl.push_str("    views {\n");
    if let Some(system) = root_system {
        dsl.push_str(&format!(
            "        container {} \"AdashiContainers\" {{\n",
            dsl_identifier(&system.external_id)
        ));
        dsl.push_str("            include *\n");
        dsl.push_str("            autolayout lr\n");
        dsl.push_str("        }\n");
    }
    dsl.push_str("    }\n");
    dsl.push('}');
    Ok(dsl)
}

fn build_structurizr_json_source(db: &Connection, workspace_id: i64) -> Result<String, String> {
    let workspace = load_workspace(db)?;
    let elements = load_elements(db, workspace_id)?;
    let relationships = load_relationships(db, workspace_id)?;
    let mut element_json = elements
        .iter()
        .map(|element| (element.external_id.clone(), element_to_json(element)))
        .collect::<HashMap<_, _>>();

    for relationship in &relationships {
        if let Some(source) = element_json.get_mut(&relationship.source_external_id) {
            let relationship_json = json!({
                "id": relationship.external_id,
                "tags": relationship.tags,
                "sourceId": relationship.source_external_id,
                "destinationId": relationship.destination_external_id,
                "description": relationship.description,
                "technology": relationship.technology
            });
            source
                .as_object_mut()
                .and_then(|object| object.get_mut("relationships"))
                .and_then(Value::as_array_mut)
                .map(|items| items.push(relationship_json));
        }
    }

    let mut people = Vec::new();
    let mut systems = Vec::new();
    for element in elements
        .iter()
        .filter(|element| element.parent_external_id.is_none())
    {
        if element.element_type.eq_ignore_ascii_case("Person") {
            if let Some(value) = element_json.remove(&element.external_id) {
                people.push(value);
            }
        } else if element.element_type.eq_ignore_ascii_case("Software System") {
            let mut system = element_json
                .remove(&element.external_id)
                .unwrap_or_else(|| element_to_json(element));
            let containers = build_child_json(&elements, &mut element_json, &element.external_id);
            system["containers"] = json!(containers);
            systems.push(system);
        }
    }

    let root_system_id = elements
        .iter()
        .find(|element| {
            element.element_type.eq_ignore_ascii_case("Software System")
                && !has_tag(&element.tags, "External")
        })
        .map(|element| element.external_id.clone())
        .unwrap_or_default();
    let view_elements = elements
        .iter()
        .map(|element| {
            let relationship_ids = relationships
                .iter()
                .filter(|relationship| relationship.source_external_id == element.external_id)
                .map(|relationship| relationship.external_id.clone())
                .collect::<Vec<_>>();
            json!({
                "id": element.external_id,
                "relationships": relationship_ids
            })
        })
        .collect::<Vec<_>>();

    let workspace_json = json!({
        "id": workspace.id,
        "name": workspace.name,
        "description": workspace.description,
        "model": {
            "people": people,
            "softwareSystems": systems
        },
        "views": {
            "containerViews": [{
                "softwareSystemId": root_system_id,
                "key": "AdashiContainers",
                "description": "Container view for the Adashi design workspace.",
                "elements": view_elements,
                "animations": [],
                "automaticLayout": {
                    "implementation": "Dagre",
                    "rankDirection": "LeftRight",
                    "rankSeparation": 300,
                    "nodeSeparation": 300,
                    "edgeSeparation": 50,
                    "vertices": false
                }
            }],
            "configuration": {
                "defaultView": "AdashiContainers",
                "styles": {
                    "elements": [
                        { "tag": "Person", "shape": "Person", "background": "#2f6f6d", "color": "#ffffff" },
                        { "tag": "Software System", "background": "#335c67", "color": "#ffffff" },
                        { "tag": "Container", "background": "#fffaf0", "color": "#1f2933", "stroke": "#2f6f6d" },
                        { "tag": "Component", "background": "#f7f7f2", "color": "#1f2933", "stroke": "#7fb0a8" },
                        { "tag": "Database", "shape": "Cylinder", "background": "#e4b363", "color": "#1f2933" }
                    ],
                    "relationships": [
                        { "tag": "Relationship", "color": "#47615f", "thickness": 3 }
                    ]
                }
            }
        }
    });

    serde_json::to_string_pretty(&workspace_json).map_err(|err| err.to_string())
}

fn build_child_json(
    elements: &[DesignElementRecord],
    element_json: &mut HashMap<String, Value>,
    parent_id: &str,
) -> Vec<Value> {
    elements
        .iter()
        .filter(|element| element.parent_external_id.as_deref() == Some(parent_id))
        .map(|element| {
            let mut value = element_json
                .remove(&element.external_id)
                .unwrap_or_else(|| element_to_json(element));
            let children = build_child_json(elements, element_json, &element.external_id);
            if !children.is_empty() {
                value["components"] = json!(children);
            }
            value
        })
        .collect()
}

fn element_to_json(element: &DesignElementRecord) -> Value {
    json!({
        "id": element.external_id,
        "tags": element.tags,
        "name": element.name,
        "description": element.description,
        "technology": element.technology,
        "relationships": [],
        "location": if has_tag(&element.tags, "External") { "External" } else { "Internal" },
        "type": element.element_type,
        "canonicalName": format!("/{}", element.name),
        "parentId": element.parent_external_id
    })
}

fn group_children<'a>(
    elements: &'a [DesignElementRecord],
) -> HashMap<&'a str, Vec<&'a DesignElementRecord>> {
    let mut children_by_parent: HashMap<&str, Vec<&DesignElementRecord>> = HashMap::new();
    for element in elements {
        if let Some(parent_id) = element.parent_external_id.as_deref() {
            children_by_parent
                .entry(parent_id)
                .or_default()
                .push(element);
        }
    }
    children_by_parent
}

fn push_element_dsl(
    dsl: &mut String,
    element: &DesignElementRecord,
    children_by_parent: &HashMap<&str, Vec<&DesignElementRecord>>,
    indent: usize,
) {
    let spaces = "    ".repeat(indent);
    let declaration = match normalized_element_type(&element.element_type).as_deref() {
        Some("Person") => "person",
        Some("Software System") => "softwareSystem",
        Some("Container") => "container",
        Some("Component") => "component",
        _ => "softwareSystem",
    };
    let children = children_by_parent
        .get(element.external_id.as_str())
        .cloned()
        .unwrap_or_default();

    if children.is_empty() {
        dsl.push_str(&format!(
            "{}{} = {} \"{}\" \"{}\" \"{}\"{}\n",
            spaces,
            dsl_identifier(&element.external_id),
            declaration,
            escape_dsl(&element.name),
            escape_dsl(&element.description),
            escape_dsl(&element.technology),
            dsl_tags(&element.tags)
        ));
        return;
    }

    dsl.push_str(&format!(
        "{}{} = {} \"{}\" \"{}\" \"{}\"{} {{\n",
        spaces,
        dsl_identifier(&element.external_id),
        declaration,
        escape_dsl(&element.name),
        escape_dsl(&element.description),
        escape_dsl(&element.technology),
        dsl_tags(&element.tags)
    ));
    for child in children {
        push_element_dsl(dsl, child, children_by_parent, indent + 1);
    }
    dsl.push_str(&format!("{}}}\n", spaces));
}

fn normalized_element_type(element_type: &str) -> Option<String> {
    match element_type.trim().to_ascii_lowercase().as_str() {
        "person" => Some("Person".to_string()),
        "software system" | "softwaresystem" => Some("Software System".to_string()),
        "container" => Some("Container".to_string()),
        "component" => Some("Component".to_string()),
        _ => None,
    }
}

fn dsl_identifier(external_id: &str) -> String {
    let sanitized = external_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    format!("e_{sanitized}")
}

fn dsl_tags(tags: &str) -> String {
    if tags.trim().is_empty() {
        String::new()
    } else {
        format!(" \"{}\"", escape_dsl(tags.trim()))
    }
}

fn escape_dsl(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn has_tag(tags: &str, tag: &str) -> bool {
    tags.split(',')
        .map(str::trim)
        .any(|candidate| candidate.eq_ignore_ascii_case(tag))
}

fn required<'a>(value: Option<&'a str>, field: &str) -> Result<&'a str, String> {
    let value = value.unwrap_or("").trim();
    if value.is_empty() {
        Err(format!("Design save field '{field}' is required"))
    } else {
        Ok(value)
    }
}

fn optional_trim(value: Option<&str>) -> Option<String> {
    let trimmed = value.unwrap_or("").trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn failed_save(
    revision: i64,
    code: &str,
    message: impl Into<String>,
    request: impl Into<String>,
) -> DesignSaveResult {
    DesignSaveResult {
        ok: false,
        stored: false,
        correction_required: true,
        revision,
        errors: vec![correction(code, message, request)],
        structurizr_dsl: None,
    }
}

fn correction(
    code: &str,
    message: impl Into<String>,
    request: impl Into<String>,
) -> DesignCorrection {
    DesignCorrection {
        code: code.to_string(),
        message: message.into(),
        request: request.into(),
    }
}

struct WorkspaceRecord {
    id: i64,
    name: String,
    description: String,
    structurizr_dsl: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{params, Connection};

    #[test]
    fn validate_mermaid_rejects_sequence_note_raw_semicolon_text() {
        let source = "sequenceDiagram
    participant Pipeline
    participant Session
    Note over Pipeline,Session: The subsystem never serializes a parallel model; Save delegates to the live USD stage session.";

        let error = validate_mermaid(source).unwrap();

        assert!(error.contains("line 4"));
        assert!(error.contains("raw semicolon"));
    }

    #[test]
    fn validate_mermaid_accepts_sequence_note_escaped_semicolon_text() {
        let source = "sequenceDiagram
    participant Pipeline
    participant Session
    Note over Pipeline,Session: The subsystem never serializes a parallel model#59; Save delegates to the live USD stage session.";

        assert_eq!(validate_mermaid(source), None);
    }

    #[test]
    fn validate_mermaid_accepts_sequence_semicolon_statement_separator() {
        let source = "sequenceDiagram
    A->>B: one; B-->>A: two";

        assert_eq!(validate_mermaid(source), None);
    }

    #[test]
    fn save_changes_deletes_relationship_and_uml_artifact() {
        let (mut db, project_id, workspace_id) = setup_design_workspace();
        insert_minimal_design(&db, workspace_id);

        db.execute(
            "INSERT INTO diagrams(workspace_id, kind, key, title, source, diagram_type, attached_to_external_id, sort_order)
             VALUES (?1, 'mermaid', 'PlaceholderFlow', 'Placeholder flow', 'flowchart TD\n    A --> B', 'flow', 'placeholder-rel', 2)",
            params![workspace_id],
        )
        .unwrap();
        db.execute(
            "INSERT INTO design_bindings(workspace_id, design_external_id, target_type, target)
             VALUES (?1, 'PlaceholderFlow', 'symbol', 'PlaceholderFlow')",
            params![workspace_id],
        )
        .unwrap();
        db.execute(
            "INSERT INTO diagrams(workspace_id, kind, key, title, source, diagram_type, attached_to_external_id, sort_order)
             VALUES (?1, 'mermaid', 'LegacyState', 'Legacy state', 'stateDiagram-v2\n    [*] --> Old', 'state', 'container-a', 3)",
            params![workspace_id],
        )
        .unwrap();
        db.execute(
            "INSERT INTO design_bindings(workspace_id, design_external_id, target_type, target)
             VALUES (?1, 'LegacyState', 'symbol', 'LegacyState')",
            params![workspace_id],
        )
        .unwrap();

        let result = save_changes(
            &mut db,
            project_id,
            0,
            "Remove superseded placeholder design rows.",
            &[
                change("delete_relationship").with_external_id("placeholder-rel"),
                change("delete_uml").with_key("LegacyState"),
            ],
        )
        .unwrap();

        assert!(result.ok);
        assert!(result.stored);
        assert_eq!(result.revision, 1);

        let overview = load_overview(&db, project_id, None).unwrap();
        assert!(!overview
            .relationships
            .iter()
            .any(|relationship| relationship.external_id == "placeholder-rel"));
        assert!(!overview
            .diagrams
            .iter()
            .any(|diagram| diagram.key == "PlaceholderFlow" || diagram.key == "LegacyState"));
        assert!(!overview
            .bindings
            .iter()
            .any(|binding| binding.design_external_id == "PlaceholderFlow"
                || binding.design_external_id == "LegacyState"));
    }

    #[test]
    fn save_changes_delete_element_cascades_design_dependencies() {
        let (mut db, project_id, workspace_id) = setup_design_workspace();
        insert_minimal_design(&db, workspace_id);
        db.execute(
            "INSERT INTO diagrams(workspace_id, kind, key, title, source, diagram_type, attached_to_external_id, sort_order)
             VALUES (?1, 'mermaid', 'ComponentSequence', 'Component sequence', 'sequenceDiagram\n    User->>Component: call', 'sequence', 'component-a', 2)",
            params![workspace_id],
        )
        .unwrap();
        db.execute(
            "INSERT INTO diagrams(workspace_id, kind, key, title, source, diagram_type, attached_to_external_id, sort_order)
             VALUES (?1, 'mermaid', 'RelationshipFlow', 'Relationship flow', 'flowchart TD\n    A --> B', 'flow', 'placeholder-rel', 3)",
            params![workspace_id],
        )
        .unwrap();
        for design_id in [
            "component-a",
            "placeholder-rel",
            "ComponentSequence",
            "RelationshipFlow",
        ] {
            db.execute(
                "INSERT INTO design_bindings(workspace_id, design_external_id, target_type, target)
                 VALUES (?1, ?2, 'symbol', ?2)",
                params![workspace_id, design_id],
            )
            .unwrap();
        }

        let result = save_changes(
            &mut db,
            project_id,
            0,
            "Remove a retired component branch.",
            &[change("delete_element").with_external_id("component-a")],
        )
        .unwrap();

        assert!(result.ok);

        let overview = load_overview(&db, project_id, None).unwrap();
        assert!(!overview
            .elements
            .iter()
            .any(|element| element.external_id == "component-a"));
        assert!(!overview
            .relationships
            .iter()
            .any(|relationship| relationship.external_id == "placeholder-rel"));
        assert!(
            !overview
                .diagrams
                .iter()
                .any(|diagram| diagram.key == "ComponentSequence"
                    || diagram.key == "RelationshipFlow")
        );
        assert!(!overview.bindings.iter().any(|binding| {
            matches!(
                binding.design_external_id.as_str(),
                "component-a" | "placeholder-rel" | "ComponentSequence" | "RelationshipFlow"
            )
        }));
    }

    #[test]
    fn save_changes_rejects_unknown_delete_without_bumping_revision() {
        let (mut db, project_id, workspace_id) = setup_design_workspace();
        insert_minimal_design(&db, workspace_id);

        let result = save_changes(
            &mut db,
            project_id,
            0,
            "Remove a typoed relationship.",
            &[change("delete_relationship").with_external_id("missing-rel")],
        )
        .unwrap();

        assert!(!result.ok);
        assert!(!result.stored);
        assert_eq!(result.revision, 0);
        assert_eq!(result.errors[0].code, "save.invalid_changeset");
        assert!(result.errors[0].message.contains("missing-rel"));
    }

    #[test]
    fn save_changes_rejects_unrenderable_sequence_uml_without_bumping_revision() {
        let (mut db, project_id, workspace_id) = setup_design_workspace();
        insert_minimal_design(&db, workspace_id);

        let result = save_changes(
            &mut db,
            project_id,
            0,
            "Store sequence diagram.",
            &[change("upsert_uml").with_sequence_uml(
                "SaveSequence",
                "Project save sequence",
                "container-a",
                "sequenceDiagram
    participant Pipeline
    participant Session
    Note over Pipeline,Session: The subsystem never serializes a parallel model; Save delegates to the live USD stage session.",
            )],
        )
        .unwrap();

        assert!(!result.ok);
        assert!(!result.stored);
        assert_eq!(result.revision, 0);
        assert_eq!(result.errors[0].code, "uml.syntax_error");
        assert!(result.errors[0].message.contains("raw semicolon"));

        let stored_count: i64 = db
            .query_row(
                "SELECT COUNT(*)
                 FROM diagrams
                 WHERE workspace_id = ?1 AND key = 'SaveSequence'",
                params![workspace_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored_count, 0);
    }

    fn setup_design_workspace() -> (Connection, i64, i64) {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(include_str!("schema.sql")).unwrap();
        db.execute(
            "INSERT INTO projects(name, slug, repository_path)
             VALUES ('Test Project', 'test-project', NULL)",
            [],
        )
        .unwrap();
        let project_id = db.last_insert_rowid();
        db.execute(
            "INSERT INTO project_state(project_id, revision)
             VALUES (?1, 0)",
            params![project_id],
        )
        .unwrap();
        db.execute(
            "INSERT INTO design_workspaces(project_id, name, description, structurizr_dsl, structurizr_json)
             VALUES (?1, 'Test Workspace', 'Test design workspace.', '', '{}')",
            params![project_id],
        )
        .unwrap();
        let workspace_id = db.last_insert_rowid();
        (db, project_id, workspace_id)
    }

    fn insert_minimal_design(db: &Connection, workspace_id: i64) {
        db.execute(
            "INSERT INTO c4_elements(workspace_id, external_id, parent_external_id, element_type, name, description, technology, tags)
             VALUES (?1, 'system', NULL, 'Software System', 'System', 'System under design.', '', '')",
            params![workspace_id],
        )
        .unwrap();
        db.execute(
            "INSERT INTO c4_elements(workspace_id, external_id, parent_external_id, element_type, name, description, technology, tags)
             VALUES (?1, 'container-a', 'system', 'Container', 'Container A', 'Primary container.', 'Rust', '')",
            params![workspace_id],
        )
        .unwrap();
        db.execute(
            "INSERT INTO c4_elements(workspace_id, external_id, parent_external_id, element_type, name, description, technology, tags)
             VALUES (?1, 'component-a', 'container-a', 'Component', 'Component A', 'Retired component.', 'Rust', '')",
            params![workspace_id],
        )
        .unwrap();
        db.execute(
            "INSERT INTO c4_relationships(workspace_id, external_id, source_external_id, destination_external_id, description, technology, tags)
             VALUES (?1, 'placeholder-rel', 'container-a', 'component-a', 'Placeholder relationship.', '', '')",
            params![workspace_id],
        )
        .unwrap();
        db.execute(
            "INSERT INTO diagrams(workspace_id, kind, key, title, source, diagram_type, attached_to_external_id, sort_order)
             VALUES (?1, 'structurizr', 'StructurizrWorkspace', 'Structurizr workspace', '{}', 'structurizr', NULL, 1)",
            params![workspace_id],
        )
        .unwrap();
    }

    fn change(op: &str) -> TestChangeBuilder {
        TestChangeBuilder(DesignChange {
            op: op.to_string(),
            external_id: None,
            parent_external_id: None,
            element_type: None,
            name: None,
            description: None,
            technology: None,
            tags: None,
            source_external_id: None,
            destination_external_id: None,
            key: None,
            title: None,
            language: None,
            diagram_type: None,
            attached_to_external_id: None,
            source: None,
            design_external_id: None,
            target_type: None,
            target: None,
            viewport_width: None,
            viewport_height: None,
            screen: None,
            mockup_state: None,
            fidelity: None,
            schema_version: None,
            accepted_svg: None,
            base_revision: None,
            proposed_svg: None,
            proposed_manifest: None,
        })
    }

    struct TestChangeBuilder(DesignChange);

    impl TestChangeBuilder {
        fn with_external_id(mut self, external_id: &str) -> DesignChange {
            self.0.external_id = Some(external_id.to_string());
            self.0
        }

        fn with_key(mut self, key: &str) -> DesignChange {
            self.0.key = Some(key.to_string());
            self.0
        }

        fn with_sequence_uml(
            mut self,
            key: &str,
            title: &str,
            attached_to_external_id: &str,
            source: &str,
        ) -> DesignChange {
            self.0.key = Some(key.to_string());
            self.0.title = Some(title.to_string());
            self.0.language = Some("mermaid".to_string());
            self.0.diagram_type = Some("sequence".to_string());
            self.0.attached_to_external_id = Some(attached_to_external_id.to_string());
            self.0.source = Some(source.to_string());
            self.0
        }
    }
}
