//! Validate external edits with the same domain validators used by mutations.
use super::*;
use rusqlite::{params, Connection};

pub(super) fn validate(db: &Connection, rows: &engine::Rows) -> StorageResult<i64> {
    let project: i64 = db
        .query_row("SELECT id FROM projects", [], |r| r.get(0))
        .map_err(StorageError::backend)?;
    let workspace: i64 = db
        .query_row(
            "SELECT id FROM design_workspaces WHERE project_id=?1",
            [project],
            |r| r.get(0),
        )
        .map_err(StorageError::backend)?;
    sqlite::references::validate(db, project)?;
    let errors =
        sqlite::design::validate_workspace(db, workspace).map_err(StorageError::Validation)?;
    if !errors.is_empty() {
        return Err(invalid(
            "design",
            &serde_json::to_string(&errors).map_err(StorageError::backend)?,
        ));
    }
    let mut link_positions = BTreeMap::new();
    for ((collection, key), data) in rows {
        let path = collection.as_str();
        for (field, value) in data {
            if collection == "design_workspaces" && field == "structurizr_json" {
                continue;
            }
            if field.ends_with("_json")
                || [
                    "created_files",
                    "changed_files",
                    "query_snapshot",
                    "command_snapshot",
                ]
                .contains(&field.as_str())
            {
                let text = value
                    .as_str()
                    .ok_or_else(|| invalid(path, "JSON field is not text"))?;
                let _: Value = codec::parse(text.as_bytes(), &format!("{path}/{field}"))?;
            }
        }
        if collection == "rules" {
            api::rules::validate_rule(
                data["name"].as_str().unwrap(),
                data["intend"].as_str().unwrap(),
                data["hook"].as_str().unwrap(),
            )
            .map_err(|e| invalid(path, &e))?;
        }
        if collection == "markdown_design_documents" {
            let document=sqlite::markdown::get(db,project,data["external_id"].as_str().unwrap()).map_err(StorageError::Validation)?.ok_or_else(||invalid(path,"missing Markdown document"))?;
            document.validate()?;
        }
        if [
            "task_design_specification_links",
            "qa_job_design_links",
            "qa_job_task_links",
            "ui_mockup_annotations",
            "markdown_design_links",
        ]
        .contains(&path)
            && data["sort_order"].as_i64().unwrap_or(-1) < 0
        {
            return Err(invalid(path, "sort_order must be nonnegative"));
        }
        // Task/QA link APIs assign one position per entry. A clean Git merge
        // can move different entries into the same position; an ID tie-breaker
        // would silently invent an order neither author selected.
        let owner = match path {
            "markdown_design_links" => Some("document_id"),
            "task_design_specification_links" => Some("task_id"),
            "qa_job_design_links" | "qa_job_task_links" => Some("qa_job_id"),
            _ => None,
        };
        if let Some(owner) = owner {
            let owner_id = data[owner].as_i64().unwrap();
            let position = data["sort_order"].as_i64().unwrap();
            if let Some(previous) = link_positions.insert((path, owner_id, position), key) {
                return Err(invalid(path, &format!(
                    "conflicting ordering: records {previous} and {key} both use sort_order {position} for {owner} {owner_id}; explicitly reorder the merged list"
                )));
            }
        }
        if collection == "ui_mockup_edit_operations" && data["sequence"].as_i64().unwrap_or(-1) < 0
        {
            return Err(invalid(path, "sequence must be nonnegative"));
        }
        if collection == "qa_job_runs" && data["status"] != "running" {
            if data["finished_at"].is_null() || data["duration_ms"].as_i64().is_none_or(|n| n < 0)
                || (data["status"] == "passed" && data["exit_code"] != 0) {
                return Err(invalid(path, "completed QA evidence requires its completion time, nonnegative duration and a successful exit code when passed"));
            }
        }
    }
    // Loading all retained domain data checks typed JSON fields and enums as well
    // as SQL constraints; no domain is silently skipped after a merge.
    sqlite::tasks::load_tasks(
        db,
        project,
        &[
            api::tasks::TaskState::Todo,
            api::tasks::TaskState::Active,
            api::tasks::TaskState::Finished,
            api::tasks::TaskState::Closed,
        ],
    )
    .map_err(StorageError::Validation)?;
    sqlite::qa::load_jobs(db, project, None).map_err(StorageError::Validation)?;
    sqlite::qa::load_runs(db, project, None).map_err(StorageError::Validation)?;
    sqlite::memory::load_memory(db, project).map_err(StorageError::Validation)?;
    sqlite::fixed_hooks::load_fixed_hook_prompts(db, project).map_err(StorageError::Validation)?;
    for summary in sqlite::mockups::load_summaries(db, project).map_err(StorageError::Validation)? {
        let m = sqlite::mockups::load_mockup(db, project, &summary.external_id)
            .map_err(StorageError::Validation)?;
        let (w, h) = (m.manifest.viewport_width, m.manifest.viewport_height);
        crate::mockups::validate_svg(&m.accepted_svg, w, h).map_err(StorageError::Validation)?;
        if let Some(svg) = &m.working_svg {
            crate::mockups::validate_svg(svg, w, h).map_err(StorageError::Validation)?;
        }
        if m.working_svg.is_some() && m.base_revision != Some(m.accepted_revision) {
            return Err(invalid(
                &m.external_id,
                "draft base revision differs from accepted revision",
            ));
        }
        if m.status != "accepted" && m.working_svg.is_none() {
            return Err(invalid(&m.external_id, "editable mockup status requires a working draft"));
        }
        if m.status == "accepted" && (m.base_revision.is_some() || !m.edit_operations.is_empty() || !m.annotations.is_empty()) {
            return Err(invalid(&m.external_id, "accepted mockup retains draft-only fields"));
        }
        if (m.status == "accepted" && (m.working_svg.is_some() || m.proposal.is_some()))
            || (m.status == "proposed") != m.proposal.is_some()
        {
            return Err(invalid(
                &m.external_id,
                "mockup status/draft/proposal conflict",
            ));
        }
        if let Some(p) = &m.proposal {
            if p.base_revision != m.accepted_revision
                || p.proposed_manifest.key != m.external_id
                || p.proposed_manifest.attached_to_external_id != m.manifest.attached_to_external_id
            {
                return Err(invalid(&m.external_id, "proposal identity/base conflict"));
            }
            crate::mockups::validate_svg(
                &p.proposed_svg,
                p.proposed_manifest.viewport_width,
                p.proposed_manifest.viewport_height,
            )
            .map_err(StorageError::Validation)?;
        }
    }
    let dsl =
        sqlite::design::build_structurizr_dsl(db, workspace).map_err(StorageError::Validation)?;
    let json = sqlite::design::build_structurizr_json_source(db, workspace)
        .map_err(StorageError::Validation)?;
    db.execute(
        "UPDATE design_workspaces SET structurizr_dsl=?1,structurizr_json=?2 WHERE id=?3",
        params![dsl, json, workspace],
    )
    .map_err(StorageError::backend)?;
    db.execute(
        "UPDATE diagrams SET source=?1 WHERE workspace_id=?2 AND kind='structurizr'",
        params![json, workspace],
    )
    .map_err(StorageError::backend)?;
    Ok(project)
}
