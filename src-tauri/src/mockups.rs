use crate::state;
use base64::Engine as _;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

#[derive(Clone, Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
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

#[derive(Clone, Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MockupEditOperation {
    pub sequence: i64,
    pub kind: String,
    pub target_element_id: Option<String>,
    pub payload_json: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MockupAnnotation {
    pub external_id: String,
    pub svg_path: String,
    pub optional_text: String,
    pub sort_order: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MockupProposal {
    pub base_revision: i64,
    pub proposed_svg: String,
    pub proposed_manifest: MockupManifest,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UiMockup {
    pub id: i64,
    pub external_id: String,
    pub title: String,
    pub manifest: MockupManifest,
    pub accepted_svg: String,
    pub accepted_revision: i64,
    pub working_svg: Option<String>,
    pub base_revision: Option<i64>,
    pub status: String,
    pub edit_operations: Vec<MockupEditOperation>,
    pub annotations: Vec<MockupAnnotation>,
    pub proposal: Option<MockupProposal>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
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
    pub status: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
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
    pub expected_revision: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SaveDraftInput {
    pub external_id: String,
    pub working_svg: String,
    pub base_revision: i64,
    pub expected_revision: i64,
    pub edit_operations: Vec<MockupEditOperation>,
    pub annotations: Vec<MockupAnnotation>,
}

#[derive(Clone, Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MockupMutationInput {
    pub external_id: String,
    pub expected_revision: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProposeMockupInput {
    pub external_id: String,
    pub base_revision: i64,
    pub proposed_svg: String,
    pub proposed_manifest: MockupManifest,
    pub expected_revision: i64,
}

pub fn load_summaries(db: &Connection, project_id: i64) -> Result<Vec<MockupSummary>, String> {
    let mut statement = db.prepare(
        "SELECT external_id, title, attached_to_external_id, viewport_width, viewport_height,
                screen, state, fidelity, accepted_revision, status
         FROM ui_mockups WHERE project_id = ?1 ORDER BY attached_to_external_id, title, external_id",
    ).map_err(|err| err.to_string())?;
    let rows = statement
        .query_map(params![project_id], |row| {
            Ok(MockupSummary {
                external_id: row.get(0)?,
                title: row.get(1)?,
                attached_to_external_id: row.get(2)?,
                viewport_width: row.get(3)?,
                viewport_height: row.get(4)?,
                screen: row.get(5)?,
                state: row.get(6)?,
                fidelity: row.get(7)?,
                accepted_revision: row.get(8)?,
                status: row.get(9)?,
            })
        })
        .map_err(|err| err.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| err.to_string())
}

pub fn load_pending(db: &Connection, project_id: i64) -> Result<Vec<MockupSummary>, String> {
    Ok(load_summaries(db, project_id)?
        .into_iter()
        .filter(|mockup| matches!(mockup.status.as_str(), "pendingAgent" | "proposed"))
        .collect())
}

pub fn load_mockup(
    db: &Connection,
    project_id: i64,
    external_id: &str,
) -> Result<UiMockup, String> {
    let row = db.query_row(
        "SELECT id, external_id, title, attached_to_external_id, viewport_width, viewport_height,
                screen, state, fidelity, schema_version, accepted_svg, accepted_revision,
                working_svg, base_revision, status, created_at, updated_at
         FROM ui_mockups WHERE project_id = ?1 AND external_id = ?2",
        params![project_id, external_id], |row| Ok((
            row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?,
            row.get::<_, String>(3)?, row.get::<_, i64>(4)?, row.get::<_, i64>(5)?,
            row.get::<_, String>(6)?, row.get::<_, String>(7)?, row.get::<_, String>(8)?,
            row.get::<_, i64>(9)?, row.get::<_, String>(10)?, row.get::<_, i64>(11)?,
            row.get::<_, Option<String>>(12)?, row.get::<_, Option<i64>>(13)?,
            row.get::<_, String>(14)?, row.get::<_, String>(15)?, row.get::<_, String>(16)?,
        )),
    ).optional().map_err(|err| err.to_string())?
        .ok_or_else(|| format!("Unknown UI mockup id: {external_id}"))?;
    let manifest = MockupManifest {
        schema_version: row.9,
        key: row.1.clone(),
        attached_to_external_id: row.3,
        viewport_width: row.4,
        viewport_height: row.5,
        screen: row.6,
        state: row.7,
        fidelity: row.8,
    };
    Ok(UiMockup {
        id: row.0,
        external_id: row.1,
        title: row.2,
        manifest,
        accepted_svg: row.10,
        accepted_revision: row.11,
        working_svg: row.12,
        base_revision: row.13,
        status: row.14,
        edit_operations: load_operations(db, row.0)?,
        annotations: load_annotations(db, row.0)?,
        proposal: load_proposal(db, row.0)?,
        created_at: row.15,
        updated_at: row.16,
    })
}

pub fn create_mockup(
    db: &mut Connection,
    project_id: i64,
    input: CreateMockupInput,
) -> Result<UiMockup, String> {
    ensure_revision(db, project_id, input.expected_revision)?;
    let tx = db.transaction().map_err(|err| err.to_string())?;
    upsert_initial_in_transaction(&tx, project_id, &input)?;
    state::bump_project_revision(&tx, project_id)?;
    tx.commit().map_err(|err| err.to_string())?;
    load_mockup(db, project_id, input.external_id.trim())
}

pub fn upsert_initial_in_transaction(
    db: &Connection,
    project_id: i64,
    input: &CreateMockupInput,
) -> Result<(), String> {
    let external_id = required(&input.external_id, "externalId")?;
    let title = required(&input.title, "title")?;
    validate_attachment(db, input.attached_to_external_id.trim())?;
    validate_viewport(input.viewport_width, input.viewport_height)?;
    let svg = validate_svg(
        &input.accepted_svg,
        input.viewport_width,
        input.viewport_height,
    )?;
    db.execute(
        "INSERT INTO ui_mockups(project_id, external_id, title, attached_to_external_id,
            viewport_width, viewport_height, screen, state, fidelity, schema_version,
            accepted_svg, accepted_revision, status)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 1, 'accepted')
         ON CONFLICT(project_id, external_id) DO UPDATE SET
            title = excluded.title, attached_to_external_id = excluded.attached_to_external_id,
            viewport_width = excluded.viewport_width, viewport_height = excluded.viewport_height,
            screen = excluded.screen, state = excluded.state, fidelity = excluded.fidelity,
            schema_version = excluded.schema_version, accepted_svg = excluded.accepted_svg,
            accepted_revision = ui_mockups.accepted_revision + 1,
            working_svg = NULL, base_revision = NULL, status = 'accepted', updated_at = CURRENT_TIMESTAMP",
        params![project_id, external_id, title, input.attached_to_external_id.trim(), input.viewport_width,
            input.viewport_height, input.screen.trim(), input.state.trim(), input.fidelity.trim(),
            input.schema_version.unwrap_or(1).max(1), svg],
    ).map_err(|err| err.to_string())?;
    let mockup_id: i64 = db
        .query_row(
            "SELECT id FROM ui_mockups WHERE project_id=?1 AND external_id=?2",
            params![project_id, external_id],
            |row| row.get(0),
        )
        .map_err(|err| err.to_string())?;
    clear_draft_evidence(db, mockup_id)?;
    Ok(())
}

pub fn save_draft(
    db: &mut Connection,
    project_id: i64,
    input: SaveDraftInput,
) -> Result<UiMockup, String> {
    ensure_revision(db, project_id, input.expected_revision)?;
    let current = load_mockup(db, project_id, input.external_id.trim())?;
    if input.base_revision != current.accepted_revision {
        return Err(format!(
            "Stale mockup base revision: expected {}, received {}",
            current.accepted_revision, input.base_revision
        ));
    }
    let svg = validate_svg(
        &input.working_svg,
        current.manifest.viewport_width,
        current.manifest.viewport_height,
    )?;
    validate_operations(&input.edit_operations)?;
    validate_annotations(&input.annotations)?;
    let tx = db.transaction().map_err(|err| err.to_string())?;
    tx.execute("UPDATE ui_mockups SET working_svg=?1, base_revision=?2, status='workingDraft', updated_at=CURRENT_TIMESTAMP WHERE id=?3",
        params![svg, input.base_revision, current.id]).map_err(|err| err.to_string())?;
    tx.execute(
        "DELETE FROM ui_mockup_preview_cache WHERE mockup_id=?1 AND variant IN ('working', 'proposed')",
        params![current.id],
    ).map_err(|err| err.to_string())?;
    replace_operations(&tx, current.id, &input.edit_operations)?;
    replace_annotations(&tx, current.id, &input.annotations)?;
    tx.execute(
        "DELETE FROM ui_mockup_proposals WHERE mockup_id=?1",
        params![current.id],
    )
    .map_err(|err| err.to_string())?;
    state::bump_project_revision(&tx, project_id)?;
    tx.commit().map_err(|err| err.to_string())?;
    load_mockup(db, project_id, input.external_id.trim())
}

pub fn request_revision(
    db: &mut Connection,
    project_id: i64,
    input: MockupMutationInput,
) -> Result<UiMockup, String> {
    mutate_status(
        db,
        project_id,
        &input,
        "workingDraft",
        "pendingAgent",
        false,
    )
}

pub fn resume_editing(
    db: &mut Connection,
    project_id: i64,
    input: MockupMutationInput,
) -> Result<UiMockup, String> {
    let current = load_mockup(db, project_id, input.external_id.trim())?;
    if !matches!(current.status.as_str(), "pendingAgent" | "proposed") {
        return Err("Only pending or proposed mockups can resume editing".into());
    }
    ensure_revision(db, project_id, input.expected_revision)?;
    let tx = db.transaction().map_err(|err| err.to_string())?;
    tx.execute(
        "DELETE FROM ui_mockup_proposals WHERE mockup_id=?1",
        params![current.id],
    )
    .map_err(|err| err.to_string())?;
    tx.execute(
        "UPDATE ui_mockups SET status='workingDraft', updated_at=CURRENT_TIMESTAMP WHERE id=?1",
        params![current.id],
    )
    .map_err(|err| err.to_string())?;
    state::bump_project_revision(&tx, project_id)?;
    tx.commit().map_err(|err| err.to_string())?;
    load_mockup(db, project_id, input.external_id.trim())
}

#[cfg(test)]
pub fn propose(
    db: &mut Connection,
    project_id: i64,
    input: ProposeMockupInput,
) -> Result<UiMockup, String> {
    ensure_revision(db, project_id, input.expected_revision)?;
    let tx = db.transaction().map_err(|err| err.to_string())?;
    upsert_proposal_in_transaction(&tx, project_id, &input)?;
    state::bump_project_revision(&tx, project_id)?;
    tx.commit().map_err(|err| err.to_string())?;
    load_mockup(db, project_id, input.external_id.trim())
}

pub fn upsert_proposal_in_transaction(
    db: &Connection,
    project_id: i64,
    input: &ProposeMockupInput,
) -> Result<(), String> {
    let current = load_mockup(db, project_id, input.external_id.trim())?;
    if current.status != "pendingAgent" {
        return Err("AI proposals require pendingAgent status".into());
    }
    if input.base_revision != current.accepted_revision
        || current.base_revision != Some(input.base_revision)
    {
        return Err(format!(
            "Stale mockup base revision: expected {}",
            current.accepted_revision
        ));
    }
    if input.proposed_manifest.key != current.external_id
        || input.proposed_manifest.attached_to_external_id
            != current.manifest.attached_to_external_id
    {
        return Err("Proposal manifest key and attachment must match the existing mockup".into());
    }
    let svg = validate_svg(
        &input.proposed_svg,
        input.proposed_manifest.viewport_width,
        input.proposed_manifest.viewport_height,
    )?;
    let manifest_json =
        serde_json::to_string(&input.proposed_manifest).map_err(|err| err.to_string())?;
    db.execute("INSERT INTO ui_mockup_proposals(mockup_id, base_revision, proposed_svg, proposed_manifest_json)
        VALUES (?1, ?2, ?3, ?4) ON CONFLICT(mockup_id) DO UPDATE SET base_revision=excluded.base_revision,
        proposed_svg=excluded.proposed_svg, proposed_manifest_json=excluded.proposed_manifest_json, created_at=CURRENT_TIMESTAMP",
        params![current.id, input.base_revision, svg, manifest_json]).map_err(|err| err.to_string())?;
    db.execute(
        "UPDATE ui_mockups SET status='proposed', updated_at=CURRENT_TIMESTAMP WHERE id=?1",
        params![current.id],
    )
    .map_err(|err| err.to_string())?;
    db.execute(
        "DELETE FROM ui_mockup_preview_cache WHERE mockup_id=?1 AND variant='proposed'",
        params![current.id],
    )
    .map_err(|err| err.to_string())?;
    Ok(())
}

pub fn accept_proposal(
    db: &mut Connection,
    project_id: i64,
    input: MockupMutationInput,
) -> Result<UiMockup, String> {
    ensure_revision(db, project_id, input.expected_revision)?;
    let current = load_mockup(db, project_id, input.external_id.trim())?;
    let proposal = current
        .proposal
        .ok_or_else(|| "Mockup has no proposal to accept".to_string())?;
    if proposal.base_revision != current.accepted_revision {
        return Err("Proposal base revision is stale".into());
    }
    let tx = db.transaction().map_err(|err| err.to_string())?;
    tx.execute("UPDATE ui_mockups SET accepted_svg=?1, accepted_revision=accepted_revision+1,
        viewport_width=?2, viewport_height=?3, screen=?4, state=?5, fidelity=?6, schema_version=?7,
        working_svg=NULL, base_revision=NULL, status='accepted', updated_at=CURRENT_TIMESTAMP WHERE id=?8",
        params![proposal.proposed_svg, proposal.proposed_manifest.viewport_width, proposal.proposed_manifest.viewport_height,
            proposal.proposed_manifest.screen, proposal.proposed_manifest.state, proposal.proposed_manifest.fidelity,
            proposal.proposed_manifest.schema_version, current.id]).map_err(|err| err.to_string())?;
    clear_draft_evidence(&tx, current.id)?;
    state::bump_project_revision(&tx, project_id)?;
    tx.commit().map_err(|err| err.to_string())?;
    load_mockup(db, project_id, input.external_id.trim())
}

pub fn reject_proposal(
    db: &mut Connection,
    project_id: i64,
    input: MockupMutationInput,
) -> Result<UiMockup, String> {
    resume_editing(db, project_id, input)
}

pub fn discard_draft(
    db: &mut Connection,
    project_id: i64,
    input: MockupMutationInput,
) -> Result<UiMockup, String> {
    ensure_revision(db, project_id, input.expected_revision)?;
    let current = load_mockup(db, project_id, input.external_id.trim())?;
    let tx = db.transaction().map_err(|err| err.to_string())?;
    tx.execute("UPDATE ui_mockups SET working_svg=NULL, base_revision=NULL, status='accepted', updated_at=CURRENT_TIMESTAMP WHERE id=?1", params![current.id]).map_err(|err| err.to_string())?;
    clear_draft_evidence(&tx, current.id)?;
    state::bump_project_revision(&tx, project_id)?;
    tx.commit().map_err(|err| err.to_string())?;
    load_mockup(db, project_id, input.external_id.trim())
}

pub fn delete_mockup(
    db: &mut Connection,
    project_id: i64,
    input: MockupMutationInput,
) -> Result<(), String> {
    ensure_revision(db, project_id, input.expected_revision)?;
    let tx = db.transaction().map_err(|err| err.to_string())?;
    delete_in_transaction(&tx, project_id, input.external_id.trim(), true)?;
    state::bump_project_revision(&tx, project_id)?;
    tx.commit().map_err(|err| err.to_string())?;
    Ok(())
}

pub fn delete_in_transaction(
    db: &Connection,
    project_id: i64,
    external_id: &str,
    require_existing: bool,
) -> Result<(), String> {
    let deleted = db
        .execute(
            "DELETE FROM ui_mockups WHERE project_id=?1 AND external_id=?2",
            params![project_id, external_id],
        )
        .map_err(|err| err.to_string())?;
    if require_existing && deleted == 0 {
        return Err(format!("Cannot delete unknown UI mockup '{external_id}'."));
    }
    db.execute(
        "DELETE FROM design_bindings WHERE design_external_id=?1",
        params![external_id],
    )
    .map_err(|err| err.to_string())?;
    Ok(())
}

pub fn preview_png(db: &Connection, mockup: &UiMockup, variant: &str) -> Result<Vec<u8>, String> {
    let (revision, svg, width, height) = match variant {
        "accepted" => (
            mockup.accepted_revision,
            mockup.accepted_svg.as_str(),
            mockup.manifest.viewport_width,
            mockup.manifest.viewport_height,
        ),
        "working" => (
            mockup.base_revision.unwrap_or(mockup.accepted_revision),
            mockup
                .working_svg
                .as_deref()
                .ok_or("Mockup has no working SVG")?,
            mockup.manifest.viewport_width,
            mockup.manifest.viewport_height,
        ),
        "proposed" => {
            let proposal = mockup
                .proposal
                .as_ref()
                .ok_or("Mockup has no proposed SVG")?;
            (
                proposal.base_revision,
                proposal.proposed_svg.as_str(),
                proposal.proposed_manifest.viewport_width,
                proposal.proposed_manifest.viewport_height,
            )
        }
        _ => return Err("Preview variant must be accepted, working, or proposed".into()),
    };
    if let Some(png) = db.query_row("SELECT png FROM ui_mockup_preview_cache WHERE mockup_id=?1 AND source_revision=?2 AND variant=?3",
        params![mockup.id, revision, variant], |row| row.get::<_, Vec<u8>>(0)).optional().map_err(|err| err.to_string())? { return Ok(png); }
    let png = render_png(svg, width, height)?;
    db.execute("INSERT OR REPLACE INTO ui_mockup_preview_cache(mockup_id, source_revision, variant, png) VALUES (?1, ?2, ?3, ?4)",
        params![mockup.id, revision, variant, png]).map_err(|err| err.to_string())?;
    Ok(png)
}

pub fn preview_base64(db: &Connection, mockup: &UiMockup, variant: &str) -> Result<String, String> {
    Ok(base64::engine::general_purpose::STANDARD.encode(preview_png(db, mockup, variant)?))
}

pub fn render_png(svg: &str, width: i64, height: i64) -> Result<Vec<u8>, String> {
    validate_viewport(width, height)?;
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    let tree = resvg::usvg::Tree::from_str(svg, &options)
        .map_err(|err| format!("SVG parse failed: {err}"))?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width as u32, height as u32)
        .ok_or("Could not allocate PNG surface")?;
    let size = tree.size();
    let transform = resvg::tiny_skia::Transform::from_scale(
        width as f32 / size.width(),
        height as f32 / size.height(),
    );
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    pixmap.encode_png().map_err(|err| err.to_string())
}

pub fn validate_svg(svg: &str, width: i64, height: i64) -> Result<String, String> {
    validate_viewport(width, height)?;
    let document =
        roxmltree::Document::parse(svg).map_err(|err| format!("Invalid SVG XML: {err}"))?;
    let root = document.root_element();
    if root.tag_name().name() != "svg" {
        return Err("Mockup source must have an <svg> root".into());
    }
    let allowed = [
        "svg",
        "g",
        "rect",
        "circle",
        "ellipse",
        "line",
        "polyline",
        "polygon",
        "path",
        "text",
        "tspan",
        "image",
        "defs",
        "clipPath",
        "mask",
        "linearGradient",
        "radialGradient",
        "stop",
        "title",
        "desc",
    ];
    let identified = [
        "g", "rect", "circle", "ellipse", "line", "polyline", "polygon", "path", "text", "image",
    ];
    let mut ids = HashSet::new();
    let mut count = 0;
    for node in document.descendants().filter(|node| node.is_element()) {
        let name = node.tag_name().name();
        if !allowed.contains(&name) {
            return Err(format!("Unsafe or unsupported SVG element: <{name}>"));
        }
        for attribute in node.attributes() {
            let attr = attribute.name().to_ascii_lowercase();
            let value = attribute.value().trim().to_ascii_lowercase();
            if attr.starts_with("on")
                || attr == "style" && (value.contains("url(") || value.contains("expression("))
            {
                return Err(format!("Unsafe SVG attribute: {}", attribute.name()));
            }
            if attr == "href" || attr.ends_with(":href") {
                if !(value.starts_with('#')
                    || value.starts_with("data:image/png")
                    || value.starts_with("data:image/jpeg")
                    || value.starts_with("data:image/webp")
                    || value.starts_with("data:image/gif"))
                {
                    return Err("SVG external resources are not allowed".into());
                }
            }
            if value.contains("javascript:") {
                return Err("SVG javascript URLs are not allowed".into());
            }
        }
        if identified.contains(&name)
            && node
                .ancestors()
                .all(|ancestor| ancestor.tag_name().name() != "defs")
        {
            let id = node
                .attribute("data-adashi-id")
                .ok_or_else(|| format!("<{name}> requires a stable data-adashi-id"))?;
            if id.trim().is_empty() || !ids.insert(id.to_string()) {
                return Err(format!("Duplicate or empty data-adashi-id: {id}"));
            }
            count += 1;
        }
    }
    if count == 0 {
        return Err("Mockup SVG must contain at least one identified visual element".into());
    }
    Ok(svg.trim().to_string())
}

fn mutate_status(
    db: &mut Connection,
    project_id: i64,
    input: &MockupMutationInput,
    from: &str,
    to: &str,
    clear: bool,
) -> Result<UiMockup, String> {
    ensure_revision(db, project_id, input.expected_revision)?;
    let current = load_mockup(db, project_id, input.external_id.trim())?;
    if current.status != from {
        return Err(format!("Mockup must be {from} before changing to {to}"));
    }
    let tx = db.transaction().map_err(|err| err.to_string())?;
    tx.execute(
        "UPDATE ui_mockups SET status=?1, updated_at=CURRENT_TIMESTAMP WHERE id=?2",
        params![to, current.id],
    )
    .map_err(|err| err.to_string())?;
    if clear {
        clear_draft_evidence(&tx, current.id)?;
    }
    state::bump_project_revision(&tx, project_id)?;
    tx.commit().map_err(|err| err.to_string())?;
    load_mockup(db, project_id, input.external_id.trim())
}

fn ensure_revision(db: &Connection, project_id: i64, expected: i64) -> Result<(), String> {
    let current = state::load_project_revision(db, project_id)?.revision;
    if current == expected {
        Ok(())
    } else {
        Err(format!(
            "Stale project revision: expected {expected}, current {current}"
        ))
    }
}

fn validate_attachment(db: &Connection, id: &str) -> Result<(), String> {
    let exists = db.query_row("SELECT 1 FROM c4_elements WHERE external_id=?1 UNION SELECT 1 FROM c4_relationships WHERE external_id=?1 LIMIT 1",
        params![id], |_| Ok(())).optional().map_err(|err| err.to_string())?.is_some();
    if exists {
        Ok(())
    } else {
        Err(format!("Unknown mockup attachment id: {id}"))
    }
}

fn validate_viewport(width: i64, height: i64) -> Result<(), String> {
    if (1..=8192).contains(&width) && (1..=8192).contains(&height) {
        Ok(())
    } else {
        Err("Mockup viewport width and height must be between 1 and 8192".into())
    }
}

fn validate_operations(operations: &[MockupEditOperation]) -> Result<(), String> {
    for (index, operation) in operations.iter().enumerate() {
        if operation.sequence != index as i64 {
            return Err("Mockup edit operation sequence must be contiguous from zero".into());
        }
        if operation.kind.trim().is_empty() {
            return Err("Mockup edit operation kind is required".into());
        }
        serde_json::from_str::<Value>(&operation.payload_json)
            .map_err(|err| format!("Invalid edit operation payload JSON: {err}"))?;
    }
    Ok(())
}

fn validate_annotations(annotations: &[MockupAnnotation]) -> Result<(), String> {
    let mut ids = HashSet::new();
    for annotation in annotations {
        if annotation.external_id.trim().is_empty() || !ids.insert(annotation.external_id.trim()) {
            return Err("Annotation ids must be non-empty and unique".into());
        }
        let path = annotation.svg_path.trim();
        if path.is_empty() {
            return Err("Annotation SVG path is required".into());
        }
        if path.len() > 100_000
            || path.chars().any(|character| {
                !character.is_ascii_digit()
                    && !"MmLlHhVvCcSsQqTtAaZzEe+-. ,\t\r\n".contains(character)
            })
        {
            return Err("Annotation SVG path contains unsupported content".into());
        }
    }
    Ok(())
}

fn replace_operations(
    db: &Connection,
    mockup_id: i64,
    operations: &[MockupEditOperation],
) -> Result<(), String> {
    db.execute(
        "DELETE FROM ui_mockup_edit_operations WHERE mockup_id=?1",
        params![mockup_id],
    )
    .map_err(|err| err.to_string())?;
    for operation in operations {
        db.execute("INSERT INTO ui_mockup_edit_operations(mockup_id, sequence, kind, target_element_id, payload_json) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![mockup_id, operation.sequence, operation.kind.trim(), operation.target_element_id.as_deref(), operation.payload_json]).map_err(|err| err.to_string())?;
    }
    Ok(())
}

fn replace_annotations(
    db: &Connection,
    mockup_id: i64,
    annotations: &[MockupAnnotation],
) -> Result<(), String> {
    db.execute(
        "DELETE FROM ui_mockup_annotations WHERE mockup_id=?1",
        params![mockup_id],
    )
    .map_err(|err| err.to_string())?;
    for annotation in annotations {
        db.execute("INSERT INTO ui_mockup_annotations(mockup_id, external_id, svg_path, optional_text, sort_order) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![mockup_id, annotation.external_id.trim(), annotation.svg_path.trim(), annotation.optional_text.trim(), annotation.sort_order]).map_err(|err| err.to_string())?;
    }
    Ok(())
}

fn load_operations(db: &Connection, mockup_id: i64) -> Result<Vec<MockupEditOperation>, String> {
    let mut statement = db.prepare("SELECT sequence, kind, target_element_id, payload_json FROM ui_mockup_edit_operations WHERE mockup_id=?1 ORDER BY sequence").map_err(|err| err.to_string())?;
    let rows = statement
        .query_map(params![mockup_id], |row| {
            Ok(MockupEditOperation {
                sequence: row.get(0)?,
                kind: row.get(1)?,
                target_element_id: row.get(2)?,
                payload_json: row.get(3)?,
            })
        })
        .map_err(|err| err.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| err.to_string())
}

fn load_annotations(db: &Connection, mockup_id: i64) -> Result<Vec<MockupAnnotation>, String> {
    let mut statement = db.prepare("SELECT external_id, svg_path, optional_text, sort_order FROM ui_mockup_annotations WHERE mockup_id=?1 ORDER BY sort_order, id").map_err(|err| err.to_string())?;
    let rows = statement
        .query_map(params![mockup_id], |row| {
            Ok(MockupAnnotation {
                external_id: row.get(0)?,
                svg_path: row.get(1)?,
                optional_text: row.get(2)?,
                sort_order: row.get(3)?,
            })
        })
        .map_err(|err| err.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| err.to_string())
}

fn load_proposal(db: &Connection, mockup_id: i64) -> Result<Option<MockupProposal>, String> {
    let row = db.query_row("SELECT base_revision, proposed_svg, proposed_manifest_json, created_at FROM ui_mockup_proposals WHERE mockup_id=?1", params![mockup_id],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?))).optional().map_err(|err| err.to_string())?;
    row.map(|row| {
        Ok(MockupProposal {
            base_revision: row.0,
            proposed_svg: row.1,
            proposed_manifest: serde_json::from_str(&row.2).map_err(|err| err.to_string())?,
            created_at: row.3,
        })
    })
    .transpose()
}

fn clear_draft_evidence(db: &Connection, mockup_id: i64) -> Result<(), String> {
    db.execute(
        "DELETE FROM ui_mockup_edit_operations WHERE mockup_id=?1",
        params![mockup_id],
    )
    .map_err(|err| err.to_string())?;
    db.execute(
        "DELETE FROM ui_mockup_annotations WHERE mockup_id=?1",
        params![mockup_id],
    )
    .map_err(|err| err.to_string())?;
    db.execute(
        "DELETE FROM ui_mockup_proposals WHERE mockup_id=?1",
        params![mockup_id],
    )
    .map_err(|err| err.to_string())?;
    Ok(())
}

fn required<'a>(value: &'a str, field: &str) -> Result<&'a str, String> {
    let value = value.trim();
    if value.is_empty() {
        Err(format!("Mockup field '{field}' is required"))
    } else {
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAFE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 60"><rect data-adashi-id="panel" width="100" height="60" fill="#fff"/></svg>"##;

    #[test]
    fn validates_safe_layered_svg() {
        assert!(validate_svg(SAFE, 100, 60).is_ok());
    }

    #[test]
    fn rejects_scripts_external_urls_and_missing_ids() {
        assert!(validate_svg("<svg><script>alert(1)</script></svg>", 100, 60).is_err());
        assert!(validate_svg(
            "<svg><image data-adashi-id='i' href='https://example.com/a.png'/></svg>",
            100,
            60
        )
        .is_err());
        assert!(validate_svg("<svg><rect width='10' height='10'/></svg>", 100, 60).is_err());
    }

    #[test]
    fn rasterizes_png() {
        let png = render_png(SAFE, 100, 60).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    }

    fn database() -> Connection {
        let mut db = Connection::open_in_memory().unwrap();
        crate::schema::migrate(&mut db).unwrap();
        db.execute(
            "INSERT INTO projects(name, slug) VALUES ('Test', 'test')",
            [],
        )
        .unwrap();
        db.execute("INSERT INTO design_workspaces(project_id, name, structurizr_dsl, structurizr_json) VALUES (1, 'Test', '', '{}')", []).unwrap();
        db.execute("INSERT INTO c4_elements(workspace_id, external_id, element_type, name, description, technology, tags) VALUES (1, 'screen', 'Software System', 'Screen', '', '', '')", []).unwrap();
        crate::state::ensure_project_state(&db).unwrap();
        db
    }

    fn create_input(expected_revision: i64) -> CreateMockupInput {
        CreateMockupInput {
            external_id: "mockup-login".into(),
            title: "Login".into(),
            attached_to_external_id: "screen".into(),
            viewport_width: 100,
            viewport_height: 60,
            screen: "Login".into(),
            state: "Default".into(),
            fidelity: "static".into(),
            schema_version: Some(1),
            accepted_svg: SAFE.into(),
            expected_revision,
        }
    }

    #[test]
    fn lifecycle_preserves_rejected_draft_and_accepts_only_explicitly() {
        let mut db = database();
        let created = create_mockup(&mut db, 1, create_input(0)).unwrap();
        assert_eq!(created.accepted_revision, 1);
        let draft_svg = SAFE.replace("#fff", "#eee");
        let draft = save_draft(
            &mut db,
            1,
            SaveDraftInput {
                external_id: created.external_id.clone(),
                working_svg: draft_svg.clone(),
                base_revision: 1,
                expected_revision: 1,
                edit_operations: vec![MockupEditOperation {
                    sequence: 0,
                    kind: "setFill".into(),
                    target_element_id: Some("panel".into()),
                    payload_json: "{\"fill\":\"#eee\"}".into(),
                }],
                annotations: vec![MockupAnnotation {
                    external_id: "note-1".into(),
                    svg_path: "M 1 1 L 8 8".into(),
                    optional_text: "Review".into(),
                    sort_order: 0,
                }],
            },
        )
        .unwrap();
        assert_eq!(draft.status, "workingDraft");
        request_revision(
            &mut db,
            1,
            MockupMutationInput {
                external_id: created.external_id.clone(),
                expected_revision: 2,
            },
        )
        .unwrap();
        let manifest = created.manifest.clone();
        propose(
            &mut db,
            1,
            ProposeMockupInput {
                external_id: created.external_id.clone(),
                base_revision: 1,
                proposed_svg: SAFE.replace("#fff", "#ddd"),
                proposed_manifest: manifest.clone(),
                expected_revision: 3,
            },
        )
        .unwrap();
        let rejected = reject_proposal(
            &mut db,
            1,
            MockupMutationInput {
                external_id: created.external_id.clone(),
                expected_revision: 4,
            },
        )
        .unwrap();
        assert_eq!(rejected.working_svg.as_deref(), Some(draft_svg.as_str()));
        assert_eq!(rejected.edit_operations.len(), 1);
        assert_eq!(rejected.annotations.len(), 1);
        request_revision(
            &mut db,
            1,
            MockupMutationInput {
                external_id: created.external_id.clone(),
                expected_revision: 5,
            },
        )
        .unwrap();
        propose(
            &mut db,
            1,
            ProposeMockupInput {
                external_id: created.external_id.clone(),
                base_revision: 1,
                proposed_svg: SAFE.replace("#fff", "#ccc"),
                proposed_manifest: manifest,
                expected_revision: 6,
            },
        )
        .unwrap();
        let accepted = accept_proposal(
            &mut db,
            1,
            MockupMutationInput {
                external_id: created.external_id,
                expected_revision: 7,
            },
        )
        .unwrap();
        assert_eq!(accepted.status, "accepted");
        assert_eq!(accepted.accepted_revision, 2);
        assert!(accepted.working_svg.is_none() && accepted.proposal.is_none());
    }

    #[test]
    fn stale_or_unsafe_mutations_do_not_change_revision() {
        let mut db = database();
        create_mockup(&mut db, 1, create_input(0)).unwrap();
        let stale = save_draft(
            &mut db,
            1,
            SaveDraftInput {
                external_id: "mockup-login".into(),
                working_svg: SAFE.into(),
                base_revision: 1,
                expected_revision: 0,
                edit_operations: vec![],
                annotations: vec![],
            },
        );
        assert!(stale.is_err());
        assert_eq!(
            crate::state::load_project_revision(&db, 1)
                .unwrap()
                .revision,
            1
        );
        let mut unsafe_input = create_input(1);
        unsafe_input.external_id = "unsafe".into();
        unsafe_input.accepted_svg = "<svg><script>alert(1)</script></svg>".into();
        assert!(create_mockup(&mut db, 1, unsafe_input).is_err());
        assert_eq!(
            crate::state::load_project_revision(&db, 1)
                .unwrap()
                .revision,
            1
        );
    }

    #[test]
    fn task_and_qa_links_resolve_mockup_titles() {
        let mut db = database();
        create_mockup(&mut db, 1, create_input(0)).unwrap();
        let task = crate::tasks::create_task(
            &db,
            1,
            crate::tasks::NewTask {
                title: "Check login mockup".into(),
                description: None,
                design_specification_links: Some(vec![
                    crate::tasks::TaskDesignSpecificationLinkInput {
                        target_type: None,
                        design_external_id: "mockup-login".into(),
                    },
                ]),
            },
        )
        .unwrap();
        assert_eq!(task.design_specification_links[0].target_type, "mockup");
        assert_eq!(task.design_specification_links[0].title, "Login");
        let job = crate::qa::create_job(
            &db,
            1,
            crate::qa::NewQaJob {
                name: "Visual contract".into(),
                description: None,
                command: "echo ok".into(),
                working_directory: None,
                shell: None,
                timeout_seconds: None,
                enabled: Some(true),
                created_by: None,
                design_specification_links: Some(vec![crate::qa::QaDesignLinkInput {
                    target_type: None,
                    design_external_id: "mockup-login".into(),
                }]),
                task_ids: Some(vec![task.id]),
                tags: Some(vec!["mockup".into()]),
            },
        )
        .unwrap();
        assert_eq!(job.design_specification_links[0].target_type, "mockup");
        assert_eq!(job.design_specification_links[0].title, "Login");
    }

    #[test]
    fn transactional_design_save_creates_mockups_and_stores_proposals_as_candidates() {
        let mut db = database();
        let initial = crate::design::DesignChange {
            op: "upsert_mockup".into(),
            external_id: Some("mockup-login".into()),
            parent_external_id: None,
            element_type: None,
            name: None,
            description: None,
            technology: None,
            tags: None,
            source_external_id: None,
            destination_external_id: None,
            key: None,
            title: Some("Login".into()),
            language: None,
            diagram_type: None,
            attached_to_external_id: Some("screen".into()),
            source: None,
            design_external_id: None,
            target_type: None,
            target: None,
            viewport_width: Some(100),
            viewport_height: Some(60),
            screen: Some("Login".into()),
            mockup_state: Some("Default".into()),
            fidelity: Some("static".into()),
            schema_version: Some(1),
            accepted_svg: Some(SAFE.into()),
            base_revision: None,
            proposed_svg: None,
            proposed_manifest: None,
        };
        let saved =
            crate::design::save_changes(&mut db, 1, 0, "Create UI mockup", &[initial]).unwrap();
        assert!(saved.stored);
        save_draft(
            &mut db,
            1,
            SaveDraftInput {
                external_id: "mockup-login".into(),
                working_svg: SAFE.replace("#fff", "#eee"),
                base_revision: 1,
                expected_revision: 1,
                edit_operations: vec![],
                annotations: vec![],
            },
        )
        .unwrap();
        request_revision(
            &mut db,
            1,
            MockupMutationInput {
                external_id: "mockup-login".into(),
                expected_revision: 2,
            },
        )
        .unwrap();
        let proposal = crate::design::DesignChange {
            op: "upsert_mockup_proposal".into(),
            external_id: Some("mockup-login".into()),
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
            base_revision: Some(1),
            proposed_svg: Some(SAFE.replace("#fff", "#ddd")),
            proposed_manifest: Some(MockupManifest {
                schema_version: 1,
                key: "mockup-login".into(),
                attached_to_external_id: "screen".into(),
                viewport_width: 100,
                viewport_height: 60,
                screen: "Login".into(),
                state: "Default".into(),
                fidelity: "static".into(),
            }),
        };
        let saved =
            crate::design::save_changes(&mut db, 1, 3, "Propose UI mockup revision", &[proposal])
                .unwrap();
        assert!(saved.stored);
        let stored = load_mockup(&db, 1, "mockup-login").unwrap();
        assert_eq!(stored.status, "proposed");
        assert_eq!(stored.accepted_svg, SAFE);
        assert!(stored.proposal.is_some());
    }
}
