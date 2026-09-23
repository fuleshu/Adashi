use super::*;
use super::{design, fixed_hooks, health as design_health, memory, mockups, qa, rules, tasks};
use adashi_storage_api::{self as api, *};
use api::{
    documents::*,
    qa::QaJob,
    rules::{NewRule, Rule},
    tasks::Task,
};

/// Translate legacy domain diagnostics at the SQLite boundary. Driver diagnostics
/// are never part of the public contract; domain corrections retain their detail.
pub(super) fn domain_error(message: String) -> StorageError {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&message) {
        if value["code"] == "resource.conflict" {
            if let Ok(conflicts) = serde_json::from_value(value["conflicts"].clone()) {
                return StorageError::Conflict(conflicts);
            }
        }
        if value["code"] == "out_of_date" || value["code"] == "read_required" {
            let items = value
                .get("conflicts")
                .cloned()
                .unwrap_or_else(|| serde_json::json!([value]));
            if let Ok(conflicts) = serde_json::from_value(items) {
                return StorageError::Documents(conflicts);
            }
        }
    }
    let lower = message.to_lowercase();
    if [
        "no such table",
        "no such column",
        "syntax error",
        "constraint failed",
        "database is",
        "invalid column",
        "out of range",
        "sqlite",
    ]
    .iter()
    .any(|v| lower.contains(v))
    {
        StorageError::Backend("The project data operation failed".into())
    } else {
        StorageError::Validation(message)
    }
}

fn bump(db: &Connection, project: i64, kind: &str, id: impl ToString) -> StorageResult<()> {
    concurrency::bump_version(db, project, kind, &id.to_string())
        .map(|_| ())
        .map_err(StorageError::backend)
}

pub(super) fn apply(
    db: &mut rusqlite::Transaction<'_>,
    project: i64,
    operation: &str,
    change: &Change,
) -> StorageResult<ChangeOutcome> {
    Ok(match change {
        Change::Design(write) => {
            let result = match write {
                DesignWrite::EditElement {
                    external_id,
                    name,
                    description,
                    technology,
                    tags,
                    ..
                } => {
                    let element = design::load_content(db)
                        .map_err(domain_error)?
                        .elements
                        .into_iter()
                        .find(|e| e.external_id == *external_id)
                        .ok_or_else(|| {
                            StorageError::ResourceNotFound(ResourceKey {
                                kind: "design.element".into(),
                                id: external_id.clone(),
                            })
                        })?;
                    let changes = [design::DesignChange::UpsertElement {
                        external_id: external_id.clone(),
                        parent_external_id: element.parent_external_id,
                        element_type: element.element_type,
                        name: name.clone(),
                        description: Some(description.clone()),
                        technology: Some(technology.clone()),
                        tags: Some(tags.clone()),
                    }];
                    let guard = design::required_guard(db, project, operation, &changes)
                        .map_err(domain_error)?;
                    design::save_changes_apply(
                        db,
                        project,
                        &guard,
                        "Update a design element from the dashboard.",
                        &changes,
                    )
                }
                DesignWrite::EditRelationship {
                    external_id,
                    description,
                    technology,
                    tags,
                    ..
                } => {
                    let rel = design::load_content(db)
                        .map_err(domain_error)?
                        .relationships
                        .into_iter()
                        .find(|r| r.external_id == *external_id)
                        .ok_or_else(|| {
                            StorageError::ResourceNotFound(ResourceKey {
                                kind: "design.relationship".into(),
                                id: external_id.clone(),
                            })
                        })?;
                    let changes = [design::DesignChange::UpsertRelationship {
                        external_id: external_id.clone(),
                        source_external_id: rel.source_external_id,
                        destination_external_id: rel.destination_external_id,
                        description: description.clone(),
                        technology: Some(technology.clone()),
                        tags: Some(tags.clone()),
                    }];
                    let guard = design::required_guard(db, project, operation, &changes)
                        .map_err(domain_error)?;
                    design::save_changes_apply(
                        db,
                        project,
                        &guard,
                        "Update a design relationship from the dashboard.",
                        &changes,
                    )
                }
                DesignWrite::Save {
                    change_intent,
                    changes,
                    read_tokens,
                } => design::save_changes_apply(
                    db,
                    project,
                    &DocumentGuard {
                        operation_id: operation.into(),
                        read_tokens: read_tokens.clone(),
                    },
                    change_intent,
                    changes,
                ),
                DesignWrite::Describe {
                    updates,
                    read_tokens,
                } => design::set_element_descriptions_apply(
                    db,
                    project,
                    &DocumentGuard {
                        operation_id: operation.into(),
                        read_tokens: read_tokens.clone(),
                    },
                    updates,
                ),
            }
            .map_err(domain_error)?;
            if !result.ok && !result.errors.iter().all(|e| e.code == "save.no_changes") {
                return Err(StorageError::DesignRejected(result));
            }
            ChangeOutcome::Design(result)
        }
        Change::Mockup(write) => {
            let result = match write {
                MockupWrite::Create(input) => {
                    mockups::create_mockup_apply(db, project, input.clone())
                }
                MockupWrite::SaveDraft(input) => {
                    mockups::save_draft_apply(db, project, input.clone())
                }
                MockupWrite::RequestRevision(input) => {
                    mockups::request_revision_apply(db, project, input.clone())
                }
                MockupWrite::ResumeEditing(input) => {
                    mockups::resume_editing_apply(db, project, input.clone())
                }
                MockupWrite::Propose(input) => mockups::propose_apply(db, project, input.clone()),
                MockupWrite::AcceptProposal(input) => {
                    mockups::accept_proposal_apply(db, project, input.clone())
                }
                MockupWrite::RejectProposal(input) => {
                    mockups::reject_proposal_apply(db, project, input.clone())
                }
                MockupWrite::DiscardDraft(input) => {
                    mockups::discard_draft_apply(db, project, input.clone())
                }
                MockupWrite::Delete(input) => {
                    mockups::delete_mockup_apply(db, project, input.clone())
                        .map_err(domain_error)?;
                    return Ok(ChangeOutcome::Mockup(None));
                }
            }
            .map_err(domain_error)?;
            ChangeOutcome::Mockup(Some(result))
        }
        Change::Task(write) => {
            let id = match write {
                TaskWrite::Create { input } => {
                    let task =
                        tasks::create_task(db, project, input.clone()).map_err(domain_error)?;
                    bump(db, project, "task", task.id)?;
                    task.id
                }
                TaskWrite::Delete { id, .. } => {
                    let dependents: i64 = db
                        .query_row(
                            "SELECT COUNT(*) FROM qa_job_task_links WHERE task_id=?1",
                            [id],
                            |r| r.get(0),
                        )
                        .map_err(StorageError::backend)?;
                    if dependents > 0 {
                        return Err(StorageError::Validation(format!("Task {id} is linked from {dependents} QA job(s); remove or version those dependencies before deletion")));
                    }
                    tasks::delete_task(db, project, *id).map_err(domain_error)?;
                    bump(db, project, "task", id)?;
                    return Ok(ChangeOutcome::Task(None));
                }
                _ => {
                    let id = match write {
                        TaskWrite::Update { input, .. } => input.task_id,
                        TaskWrite::Finish { input, .. } => input.task_id,
                        TaskWrite::Close { id, .. } => *id,
                        _ => unreachable!(),
                    };
                    let current = tasks::load_task(db, project, id).map_err(domain_error)?;
                    let mut stage = db.savepoint().map_err(StorageError::backend)?;
                    let updated = match write {
                        TaskWrite::Update { input, .. } => {
                            tasks::update_task(&stage, project, input.clone())
                        }
                        TaskWrite::Finish { input, .. } => {
                            tasks::finish_task(&stage, project, input.clone())
                        }
                        TaskWrite::Close { .. } => tasks::close_task(&stage, project, id),
                        _ => unreachable!(),
                    }
                    .map_err(domain_error)?;
                    if same_task_content(&current, &updated) {
                        stage.rollback().map_err(StorageError::backend)?;
                    } else {
                        bump(&stage, project, "task", id)?;
                    }
                    stage.commit().map_err(StorageError::backend)?;
                    id
                }
            };
            ChangeOutcome::Task(Some(
                tasks::load_task(db, project, id).map_err(domain_error)?,
            ))
        }
        Change::Qa(write) => apply_qa(db, project, write)?,
        Change::Memory(write) => {
            match write {
                MemoryWrite::Append { note } => {
                    memory::append_note_apply(db, project, note.clone()).map_err(domain_error)?;
                }
                MemoryWrite::Compact {
                    expected_version,
                    summary,
                    superseded_note_ids,
                } => {
                    memory::compact_memory_review_apply(
                        db,
                        project,
                        *expected_version,
                        operation,
                        summary.clone(),
                        superseded_note_ids,
                    )
                    .map_err(domain_error)?;
                }
                MemoryWrite::Protocol {
                    expected_version,
                    rule,
                } => {
                    memory::update_memory_rule_apply(
                        db,
                        project,
                        *expected_version,
                        operation,
                        rule.clone(),
                    )
                    .map_err(domain_error)?;
                }
            }
            ChangeOutcome::Memory(memory::load_memory(db, project).map_err(domain_error)?)
        }
        Change::Rule(write) => {
            let id = match write {
                RuleWrite::Create { input } => {
                    let id =
                        rules::create_rule(db, project, input.clone()).map_err(domain_error)?;
                    bump(db, project, "rule", id)?;
                    id
                }
                RuleWrite::Update { id, input, .. } => {
                    require_rule(db, project, *id)?;
                    let current = rules::load_rule(db, *id).map_err(domain_error)?;
                    if !same_rule(&current, input) {
                        rules::update_rule(
                            db,
                            rules::UpdateRule {
                                id: *id,
                                name: input.name.clone(),
                                enabled: input.enabled,
                                intend: input.intend.clone(),
                                hook: input.hook.clone(),
                                prompt: input.prompt.clone(),
                            },
                        )
                        .map_err(domain_error)?;
                        bump(db, project, "rule", id)?;
                    }
                    *id
                }
                RuleWrite::Delete { id, .. } => {
                    require_rule(db, project, *id)?;
                    rules::delete_rule(db, *id).map_err(domain_error)?;
                    bump(db, project, "rule", id)?;
                    return Ok(ChangeOutcome::Rule(None));
                }
            };
            ChangeOutcome::Rule(Some(rules::load_rule(db, id).map_err(domain_error)?))
        }
        Change::FixedPrompt(input) => {
            let current = fixed_hooks::load_fixed_hook_prompts(db, project)
                .map_err(domain_error)?
                .into_iter()
                .find(|v| v.key == input.key)
                .ok_or_else(|| {
                    StorageError::ResourceNotFound(ResourceKey {
                        kind: "fixed-hook".into(),
                        id: input.key.clone(),
                    })
                })?;
            if current.prompt != input.prompt.trim() {
                fixed_hooks::update_fixed_hook_prompt(
                    db,
                    project,
                    input.key.clone(),
                    input.prompt.clone(),
                )
                .map_err(domain_error)?;
                bump(db, project, "fixed-hook", &input.key)?;
            }
            ChangeOutcome::FixedPrompt(
                fixed_hooks::load_fixed_hook_prompts(db, project)
                    .map_err(domain_error)?
                    .into_iter()
                    .find(|v| v.key == input.key)
                    .ok_or_else(|| StorageError::Backend("Stored prompt is missing".into()))?,
            )
        }
        Change::Computer { input, .. } => {
            let changed=db.execute("INSERT INTO project_computers(project_id,computer_id,repository_path) VALUES(?1,?2,?3) ON CONFLICT(project_id,computer_id) DO UPDATE SET repository_path=excluded.repository_path WHERE repository_path IS NOT excluded.repository_path",params![project,input.computer_id,input.repository_path]).map_err(StorageError::backend)?;
            if changed > 0 {
                bump(db, project, "computer", &input.computer_id)?;
            }
            ChangeOutcome::Computer(input.clone())
        }
        Change::Legacy { content, .. } => {
            let current = snapshot::legacy_content(db, project)?;
            if serde_json::to_value(&current).map_err(StorageError::backend)?
                != serde_json::to_value(content).map_err(StorageError::backend)?
            {
                replace_legacy(db, project, content)?;
                bump(db, project, "legacy", "content")?;
            }
            ChangeOutcome::Legacy
        }
        Change::HealthWaiver {
            external_id,
            state,
            reason,
            task_id,
        } => {
            let exists:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM c4_elements e JOIN design_workspaces w ON w.id=e.workspace_id WHERE w.project_id=?1 AND e.external_id=?2)",params![project,external_id],|r|r.get(0)).map_err(StorageError::backend)?;
            if !exists {
                return Err(StorageError::ResourceNotFound(ResourceKey {
                    kind: "design.element".into(),
                    id: external_id.clone(),
                }));
            }
            if let Some(id) = task_id {
                tasks::load_task(db, project, *id).map_err(domain_error)?;
            }
            let id =
                design_health::record_waiver(db, project, external_id, *state, reason, *task_id)
                    .map_err(domain_error)?;
            bump(db, project, "health.waiver", id)?;
            let value = design_health::load_waivers(db, project, external_id)
                .map_err(domain_error)?
                .into_iter()
                .find(|v| v.id == id)
                .ok_or_else(|| StorageError::Backend("Stored waiver is missing".into()))?;
            ChangeOutcome::HealthWaiver(value)
        }
    })
}

fn apply_qa(
    db: &mut rusqlite::Transaction<'_>,
    project: i64,
    write: &QaWrite,
) -> StorageResult<ChangeOutcome> {
    match write {
        QaWrite::CreateJob { input } => {
            let job = qa::create_job(db, project, input.clone()).map_err(domain_error)?;
            bump(db, project, "qa.job", job.id)?;
            Ok(ChangeOutcome::QaJob(Some(
                qa::load_job(db, project, job.id).map_err(domain_error)?,
            )))
        }
        QaWrite::UpdateJob { input, .. } => {
            let current = qa::load_job(db, project, input.qa_job_id).map_err(domain_error)?;
            let mut stage = db.savepoint().map_err(StorageError::backend)?;
            let job = qa::update_job(&stage, project, input.clone()).map_err(domain_error)?;
            if same_qa_definition(&current, &job) {
                stage.rollback().map_err(StorageError::backend)?;
            } else {
                bump(&stage, project, "qa.job", job.id)?;
            }
            stage.commit().map_err(StorageError::backend)?;
            Ok(ChangeOutcome::QaJob(Some(
                qa::load_job(db, project, input.qa_job_id).map_err(domain_error)?,
            )))
        }
        QaWrite::DeleteJob { id, .. } => {
            qa::delete_job(db, project, *id).map_err(domain_error)?;
            bump(db, project, "qa.job", id)?;
            Ok(ChangeOutcome::QaJob(None))
        }
        QaWrite::StartRun {
            query,
            trigger_source,
            jobs,
        } => {
            let selected = qa::load_jobs(db, project, Some(query))
                .map_err(domain_error)?
                .into_iter()
                .filter(|v| v.enabled)
                .map(|v| (v.id, v))
                .collect::<std::collections::BTreeMap<_, _>>();
            if jobs.is_empty() || jobs.len() != selected.len() {
                return Err(StorageError::Validation(
                    "QA run must reserve every enabled job in the selection".into(),
                ));
            }
            for plan in jobs {
                let job = selected.get(&plan.job_id).ok_or_else(|| {
                    StorageError::Validation(
                        "QA execution plan does not match the selection".into(),
                    )
                })?;
                if crate::qa_runner::command_snapshot(job).map_err(domain_error)?
                    != plan.command_snapshot
                {
                    return Err(StorageError::Validation(
                        "QA command snapshot does not match its job definition".into(),
                    ));
                }
            }
            db.execute("INSERT INTO qa_runs(project_id,trigger_source,query_snapshot,status) VALUES(?1,?2,?3,'running')",params![project,trigger_source,serde_json::to_string(query).map_err(StorageError::backend)?]).map_err(StorageError::backend)?;
            let run = db.last_insert_rowid();
            bump(db, project, "qa.run", run)?;
            for plan in jobs {
                db.execute("INSERT INTO qa_job_runs(qa_run_id,qa_job_id,command_snapshot,status,output) VALUES(?1,?2,?3,'running','')",params![run,plan.job_id,plan.command_snapshot]).map_err(StorageError::backend)?;
                bump(db, project, "qa.job-run", db.last_insert_rowid())?;
            }
            Ok(ChangeOutcome::QaRun(
                qa::load_run(db, project, run).map_err(domain_error)?,
            ))
        }
        QaWrite::ClaimJob {
            job_run_id,
            expected_version,
        } => {
            let (run,status):(i64,String)=db.query_row("SELECT j.qa_run_id,j.status FROM qa_job_runs j JOIN qa_runs r ON r.id=j.qa_run_id WHERE r.project_id=?1 AND j.id=?2",params![project,job_run_id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(StorageError::backend)?;
            if *expected_version != 1 || status != "running" {
                return Err(StorageError::Validation(
                    "QA job run has already been claimed".into(),
                ));
            }
            bump(db, project, "qa.job-run", job_run_id)?;
            let job = qa::load_run(db, project, run)
                .map_err(domain_error)?
                .job_runs
                .into_iter()
                .find(|v| v.id == *job_run_id)
                .ok_or_else(|| StorageError::Backend("Reserved QA job is missing".into()))?;
            Ok(ChangeOutcome::QaEvidence(job))
        }
        QaWrite::CompleteJob {
            job_run_id,
            evidence,
            ..
        } => {
            let (run,_job,status):(i64,i64,String)=db.query_row("SELECT j.qa_run_id,j.qa_job_id,j.status FROM qa_job_runs j JOIN qa_runs r ON r.id=j.qa_run_id WHERE r.project_id=?1 AND j.id=?2",params![project,job_run_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(StorageError::backend)?;
            if status != "running" {
                return Err(StorageError::Validation(
                    "QA job run is already complete".into(),
                ));
            }
            let status = match evidence.outcome {
                QaJobOutcome::Passed => "passed",
                QaJobOutcome::Failed => "failed",
                QaJobOutcome::TimedOut => "timed_out",
            };
            if status == "passed" && evidence.exit_code != Some(0) {
                return Err(StorageError::Validation(
                    "Passed QA evidence requires exit code 0".into(),
                ));
            }
            db.execute("UPDATE qa_job_runs SET status=?1,exit_code=?2,finished_at=CURRENT_TIMESTAMP,duration_ms=?3,output=?4 WHERE id=?5",params![status,evidence.exit_code,evidence.duration_ms,evidence.output,job_run_id]).map_err(StorageError::backend)?;
            bump(db, project, "qa.job-run", job_run_id)?;
            let result = qa::load_run(db, project, run)
                .map_err(domain_error)?
                .job_runs
                .into_iter()
                .find(|v| v.id == *job_run_id)
                .ok_or_else(|| StorageError::Backend("QA evidence is missing".into()))?;
            Ok(ChangeOutcome::QaEvidence(result))
        }
        QaWrite::CompleteRun { run_id, .. } => {
            let run = qa::load_run(db, project, *run_id).map_err(domain_error)?;
            if run.status != "running" {
                return Err(StorageError::Validation(
                    "QA run is already complete".into(),
                ));
            }
            if run.job_runs.iter().any(|v| v.status == "running") {
                return Err(StorageError::Validation(
                    "QA run still has unfinished jobs".into(),
                ));
            }
            let passed = run.job_runs.iter().filter(|v| v.status == "passed").count();
            let timed_out = run
                .job_runs
                .iter()
                .filter(|v| v.status == "timed_out")
                .count();
            let failed = run.job_runs.len() - passed - timed_out;
            let status = if failed == 0 && timed_out == 0 {
                "passed"
            } else {
                "failed"
            };
            let summary = format!("{passed} passed, {failed} failed, {timed_out} timed out");
            db.execute(
                "UPDATE qa_runs SET status=?1,finished_at=CURRENT_TIMESTAMP,summary=?2 WHERE id=?3",
                params![status, summary, run_id],
            )
            .map_err(StorageError::backend)?;
            bump(db, project, "qa.run", run_id)?;
            let result = qa::load_run(db, project, *run_id).map_err(domain_error)?;
            for job in &run.job_runs {
                qa::prune_job_run_history(db, job.qa_job_id).map_err(domain_error)?;
            }
            Ok(ChangeOutcome::QaRun(result))
        }
    }
}

fn replace_legacy(db: &Connection, project: i64, content: &LegacyContent) -> StorageResult<()> {
    for table in ["coding_guidelines", "post_task_commands", "qa_checks"] {
        db.execute(
            &format!("DELETE FROM {table} WHERE project_id=?1"),
            [project],
        )
        .map_err(StorageError::backend)?;
    }
    db.execute("DELETE FROM task_qa_entries WHERE task_id IN (SELECT id FROM agent_tasks WHERE project_id=?1)",[project]).map_err(StorageError::backend)?;
    for v in &content.guidelines {
        db.execute("INSERT INTO coding_guidelines(id,project_id,title,body,severity,created_at) VALUES(?1,?2,?3,?4,?5,?6)",params![v.id,project,v.title,v.body,v.severity,v.created_at]).map_err(StorageError::backend)?;
    }
    for v in &content.post_task_commands {
        db.execute("INSERT INTO post_task_commands(id,project_id,label,command,trigger,created_at) VALUES(?1,?2,?3,?4,?5,?6)",params![v.id,project,v.label,v.command,v.trigger,v.created_at]).map_err(StorageError::backend)?;
    }
    for v in &content.qa_checks {
        db.execute("INSERT INTO qa_checks(id,project_id,label,command,required,created_at) VALUES(?1,?2,?3,?4,?5,?6)",params![v.id,project,v.label,v.command,v.required,v.created_at]).map_err(StorageError::backend)?;
    }
    for v in &content.task_qa_entries {
        db.execute("INSERT INTO task_qa_entries(id,task_id,label,status,body,created_at) VALUES(?1,?2,?3,?4,?5,?6)",params![v.id,v.task_id,v.label,v.status,v.body,v.created_at]).map_err(StorageError::backend)?;
    }
    Ok(())
}

fn same_task_content(left: &Task, right: &Task) -> bool {
    left.title == right.title
        && left.description == right.description
        && left.state == right.state
        && left.completion_memo == right.completion_memo
        && left.created_files == right.created_files
        && left.changed_files == right.changed_files
        && left.confirmation_commit_id == right.confirmation_commit_id
        && left.design_specification_links.len() == right.design_specification_links.len()
        && left
            .design_specification_links
            .iter()
            .zip(&right.design_specification_links)
            .all(|(a, b)| {
                a.target_type == b.target_type && a.design_external_id == b.design_external_id
            })
}

fn same_qa_definition(left: &QaJob, right: &QaJob) -> bool {
    left.name == right.name
        && left.description == right.description
        && left.command == right.command
        && left.working_directory == right.working_directory
        && left.shell == right.shell
        && left.timeout_seconds == right.timeout_seconds
        && left.enabled == right.enabled
        && left.design_specification_links.len() == right.design_specification_links.len()
        && left
            .design_specification_links
            .iter()
            .zip(&right.design_specification_links)
            .all(|(a, b)| {
                a.target_type == b.target_type && a.design_external_id == b.design_external_id
            })
        && left
            .task_links
            .iter()
            .map(|link| link.task_id)
            .collect::<Vec<_>>()
            == right
                .task_links
                .iter()
                .map(|link| link.task_id)
                .collect::<Vec<_>>()
        && left.tags == right.tags
}

fn same_rule(current: &Rule, replacement: &NewRule) -> bool {
    current.name == replacement.name
        && current.enabled == replacement.enabled
        && current.intend == replacement.intend
        && current.hook == replacement.hook
        && current.prompt == replacement.prompt
}
