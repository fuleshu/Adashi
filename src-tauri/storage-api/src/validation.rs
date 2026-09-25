use crate::{
    coordination::*, design::DesignChange, documents::*, memory::*, rules::validate_rule,
    tasks::TaskState, *,
};
use std::collections::{BTreeMap, BTreeSet};

fn invalid(message: impl Into<String>) -> StorageError {
    StorageError::Validation(message.into())
}
fn required(value: &str, label: &str) -> StorageResult<()> {
    if value.trim().is_empty() {
        Err(invalid(format!("{label} is required")))
    } else {
        Ok(())
    }
}
fn positive(value: i64, label: &str) -> StorageResult<()> {
    if value <= 0 {
        Err(invalid(format!("{label} must be positive")))
    } else {
        Ok(())
    }
}
pub fn validate_intent(update: &IntentUpdate) -> StorageResult<()> {
    required(&update.agent_run_id, "agentRunId")?;
    if update.ttl_seconds > 86_400 {
        return Err(invalid("ttlSeconds must be between 0 and 86400"));
    }
    let mut seen = BTreeSet::new();
    for key in &update.resources {
        required(&key.kind, "resource kind")?;
        required(&key.id, "resource id")?;
        if !seen.insert(key) {
            return Err(invalid("Duplicate intent resource"));
        }
    }
    Ok(())
}
fn tokens(tokens: &[DocumentReadToken]) -> StorageResult<()> {
    let mut seen = BTreeSet::new();
    for token in tokens {
        required(&token.document_id, "documentId")?;
        required(&token.read_token, "readToken")?;
        if !seen.insert(&token.document_id) {
            return Err(invalid("Duplicate documentId"));
        }
    }
    Ok(())
}
fn mockup_guard(write: &MockupWrite) -> (&str, &str, i64, i64) {
    match write {
        MockupWrite::Create(v) => (
            &v.external_id,
            &v.operation_id,
            v.expected_accepted_version,
            v.expected_working_version,
        ),
        MockupWrite::SaveDraft(v) => (
            &v.external_id,
            &v.operation_id,
            v.expected_accepted_version,
            v.expected_working_version,
        ),
        MockupWrite::Propose(v) => (
            &v.external_id,
            &v.operation_id,
            v.expected_accepted_version,
            v.expected_working_version,
        ),
        MockupWrite::RequestRevision(v)
        | MockupWrite::ResumeEditing(v)
        | MockupWrite::AcceptProposal(v)
        | MockupWrite::RejectProposal(v)
        | MockupWrite::DiscardDraft(v)
        | MockupWrite::Delete(v) => (
            &v.external_id,
            &v.operation_id,
            v.expected_accepted_version,
            v.expected_working_version,
        ),
    }
}
fn unique(targets: &mut BTreeSet<String>, key: String) -> StorageResult<()> {
    if targets.insert(key) {
        Ok(())
    } else {
        Err(invalid("A resource may be written only once per batch"))
    }
}

/// Static validation and canonical fingerprinting shared by all adapters.
/// State-dependent invariants (task transitions, C4 hierarchy, SVG policy,
/// references, QA states and retention) must be checked in the atomic commit.
pub fn prepare_mutation(mut mutation: Mutation) -> StorageResult<PreparedMutation> {
    mutation.operation_id = mutation.operation_id.trim().to_owned();
    required(&mutation.operation_id, "operationId")?;
    if mutation.changes.is_empty() {
        return Err(invalid("At least one change is required"));
    }
    let mut targets = BTreeSet::new();
    for change in &mutation.changes {
        match change {
            Change::Design(DesignWrite::EditElement {
                external_id, name, ..
            }) => {
                required(external_id, "externalId")?;
                required(name, "name")?;
            }
            Change::Design(DesignWrite::EditRelationship { external_id, .. }) => {
                required(external_id, "externalId")?
            }
            Change::Design(DesignWrite::Save {
                changes,
                read_tokens,
                change_intent,
            }) => {
                required(change_intent, "changeIntent")?;
                if changes.is_empty() {
                    return Err(invalid("At least one design change is required"));
                }
                tokens(read_tokens)?;
                for item in changes {
                    let key = match item {
                        DesignChange::UpsertMarkdown { external_id, title, body, design_links } => {
                            crate::markdown::MarkdownDesignDocument { external_id: external_id.clone(), title: title.clone(), body: body.clone(), design_links: design_links.clone() }.validate()?;
                            format!("design.markdown:{external_id}")
                        }
                        DesignChange::DeleteMarkdown { external_id } => {
                            crate::markdown::validate_identity(external_id)?;
                            format!("design.markdown:{external_id}")
                        }
                        DesignChange::UpsertElement { external_id, .. }
                        | DesignChange::DeleteElement { external_id } => {
                            format!("design.element:{external_id}")
                        }
                        DesignChange::UpsertRelationship { external_id, .. }
                        | DesignChange::DeleteRelationship { external_id } => {
                            format!("design.relationship:{external_id}")
                        }
                        DesignChange::UpsertUml { key, .. } | DesignChange::DeleteUml { key } => {
                            format!("design.uml:{key}")
                        }
                        DesignChange::UpsertBinding {
                            design_external_id,
                            target_type,
                            target,
                        }
                        | DesignChange::DeleteBinding {
                            design_external_id,
                            target_type,
                            target,
                        } => format!("design.binding:{design_external_id}|{target_type}|{target}"),
                        DesignChange::UpsertMockup { external_id, .. }
                        | DesignChange::UpsertMockupProposal { external_id, .. }
                        | DesignChange::DeleteMockup { external_id } => {
                            format!("mockup:{external_id}")
                        }
                    };
                    unique(&mut targets, key)?;
                }
            }
            Change::Design(DesignWrite::Describe {
                updates,
                read_tokens,
            }) => {
                tokens(read_tokens)?;
                if updates.is_empty() {
                    return Err(invalid("At least one description is required"));
                }
                for update in updates {
                    required(&update.external_id, "externalId")?;
                    unique(
                        &mut targets,
                        format!("design.element:{}", update.external_id),
                    )?;
                }
            }
            Change::Task(TaskWrite::Create { input }) => required(&input.title, "Task title")?,
            Change::Task(TaskWrite::Update { input, .. }) => {
                positive(input.task_id, "taskId")?;
                if let Some(title) = &input.title {
                    required(title, "Task title")?;
                }
                if let Some(state) = &input.state {
                    TaskState::parse(state).map_err(invalid)?;
                }
            }
            Change::Task(TaskWrite::Finish { input, .. }) => positive(input.task_id, "taskId")?,
            Change::Task(TaskWrite::Close { id, .. } | TaskWrite::Delete { id, .. }) => {
                positive(*id, "taskId")?
            }
            Change::Qa(QaWrite::CreateJob { input }) => {
                required(&input.name, "QA name")?;
                required(&input.command, "QA command")?;
                if let Some(timeout) = input.timeout_seconds {
                    positive(timeout, "QA timeout")?;
                }
            }
            Change::Qa(QaWrite::UpdateJob { input, .. }) => {
                positive(input.qa_job_id, "qaJobId")?;
                if let Some(name) = &input.name {
                    required(name, "QA name")?;
                }
                if let Some(command) = &input.command {
                    required(command, "QA command")?;
                }
                if let Some(timeout) = input.timeout_seconds {
                    positive(timeout, "QA timeout")?;
                }
            }
            Change::Qa(QaWrite::DeleteJob { id, .. }) => positive(*id, "qaJobId")?,
            Change::Qa(QaWrite::StartRun {
                jobs,
                trigger_source,
                ..
            }) => {
                required(trigger_source, "trigger source")?;
                if jobs.is_empty() {
                    return Err(invalid("No enabled QA jobs matched the run request"));
                }
                let mut seen = BTreeSet::new();
                for job in jobs {
                    positive(job.job_id, "qaJobId")?;
                    positive(job.expected_version, "QA expectedVersion")?;
                    required(&job.command_snapshot, "command snapshot")?;
                    if !seen.insert(job.job_id) {
                        return Err(invalid("Duplicate QA job"));
                    }
                }
            }
            Change::Qa(QaWrite::CompleteJob {
                job_run_id,
                evidence,
                ..
            }) => {
                positive(*job_run_id, "jobRunId")?;
                if evidence.duration_ms < 0 {
                    return Err(invalid("Negative QA duration"));
                }
            }
            Change::Qa(QaWrite::CompleteRun { run_id, .. }) => positive(*run_id, "runId")?,
            Change::Qa(QaWrite::ClaimJob { job_run_id, .. }) => positive(*job_run_id, "jobRunId")?,
            Change::Memory(MemoryWrite::Append { note }) => {
                required(&note.note_id, "noteId")?;
                required(&note.run_id, "runId")?;
                required(&note.body, "Note body")?;
                if note.operation_id != mutation.operation_id {
                    return Err(invalid("Nested operationId must match the batch"));
                }
                if note.body.chars().count() > MAX_NOTE_CHARS {
                    return Err(invalid("Memory note exceeds limit"));
                }
                if let Some(id) = note.task_id {
                    positive(id, "taskId")?;
                }
                unique(&mut targets, format!("memory.note:{}", note.note_id))?;
                // Legacy notes retain their unique operationId as provenance.
                unique(&mut targets, "memory.append".into())?;
            }
            Change::Memory(MemoryWrite::Compact {
                summary,
                superseded_note_ids,
                ..
            }) => {
                if summary.chars().count() > MAX_SUMMARY_CHARS {
                    return Err(invalid("Memory summary exceeds limit"));
                }
                let mut seen = BTreeSet::new();
                for id in superseded_note_ids {
                    required(id, "superseded noteId")?;
                    if !seen.insert(id) {
                        return Err(invalid("Duplicate superseded noteId"));
                    }
                }
            }
            Change::Rule(RuleWrite::Create { input } | RuleWrite::Update { input, .. }) => {
                validate_rule(&input.name, &input.intend, &input.hook).map_err(invalid)?;
                if let Change::Rule(RuleWrite::Update { id, .. }) = change {
                    positive(*id, "ruleId")?;
                }
            }
            Change::Rule(RuleWrite::Delete { id, .. }) => positive(*id, "ruleId")?,
            Change::FixedPrompt(input) => required(&input.key, "Fixed prompt key")?,
            Change::Computer {
                expected_version,
                input,
            } => {
                required(&input.computer_id, "computerId")?;
                required(&input.repository_path, "repository path")?;
                if *expected_version < 0 {
                    return Err(invalid("Computer version must be nonnegative"));
                }
            }
            Change::Mockup(write) => {
                let (id, operation, accepted, working) = mockup_guard(write);
                required(id, "externalId")?;
                if operation != mutation.operation_id {
                    return Err(invalid("Nested operationId must match the batch"));
                }
                if accepted < 0 || working < 0 {
                    return Err(invalid("Mockup versions must be nonnegative"));
                }
                unique(&mut targets, format!("mockup:{id}"))?;
                if let MockupWrite::Create(v) = write {
                    positive(v.viewport_width, "viewport width")?;
                    positive(v.viewport_height, "viewport height")?;
                }
            }
            Change::HealthWaiver {
                external_id,
                reason,
                task_id,
                ..
            } => {
                required(external_id, "externalId")?;
                required(reason, "Waiver reason")?;
                if let Some(id) = task_id {
                    positive(*id, "taskId")?;
                }
            }
            Change::Memory(MemoryWrite::Protocol { .. }) | Change::Legacy { .. } => {}
        }
        for expected in expectations(change) {
            if !matches!(change, Change::Computer { .. }) {
                positive(expected.expected_version, "expectedVersion")?;
            }
            unique(
                &mut targets,
                format!("{}:{}", expected.resource_kind, expected.resource_id),
            )?;
        }
    }
    let fingerprint = hash(&("adashi.storage.mutation.v1", &mutation)).map_err(invalid)?;
    Ok(PreparedMutation {
        mutation,
        fingerprint,
    })
}

fn expectation(kind: &str, id: impl ToString, version: i64) -> ResourceExpectation {
    ResourceExpectation {
        resource_kind: kind.into(),
        resource_id: id.to_string(),
        expected_version: version,
    }
}
fn expectations(change: &Change) -> Vec<ResourceExpectation> {
    let item = match change {
        Change::Design(DesignWrite::EditElement {
            external_id,
            expected_version,
            ..
        }) => expectation("design.element", external_id, *expected_version),
        Change::Design(DesignWrite::EditRelationship {
            external_id,
            expected_version,
            ..
        }) => expectation("design.relationship", external_id, *expected_version),
        Change::Task(TaskWrite::Update {
            expected_version,
            input,
        }) => expectation("task", input.task_id, *expected_version),
        Change::Task(TaskWrite::Finish {
            expected_version,
            input,
        }) => expectation("task", input.task_id, *expected_version),
        Change::Task(
            TaskWrite::Close {
                id,
                expected_version,
            }
            | TaskWrite::Delete {
                id,
                expected_version,
            },
        ) => expectation("task", id, *expected_version),
        Change::Qa(QaWrite::UpdateJob {
            expected_version,
            input,
        }) => expectation("qa.job", input.qa_job_id, *expected_version),
        Change::Qa(QaWrite::DeleteJob {
            id,
            expected_version,
        }) => expectation("qa.job", id, *expected_version),
        Change::Qa(
            QaWrite::CompleteJob {
                job_run_id,
                expected_version,
                ..
            }
            | QaWrite::ClaimJob {
                job_run_id,
                expected_version,
            },
        ) => expectation("qa.job-run", job_run_id, *expected_version),
        Change::Qa(QaWrite::CompleteRun {
            run_id,
            expected_version,
        }) => expectation("qa.run", run_id, *expected_version),
        Change::Memory(MemoryWrite::Compact {
            expected_version, ..
        }) => expectation("memory.canonical", "canonical", *expected_version),
        Change::Memory(MemoryWrite::Protocol {
            expected_version, ..
        }) => expectation("memory.protocol", "protocol", *expected_version),
        Change::Rule(
            RuleWrite::Update {
                id,
                expected_version,
                ..
            }
            | RuleWrite::Delete {
                id,
                expected_version,
            },
        ) => expectation("rule", id, *expected_version),
        Change::FixedPrompt(input) => expectation("fixed-hook", &input.key, input.expected_version),
        Change::Legacy {
            expected_version, ..
        } => expectation("legacy", "content", *expected_version),
        Change::Computer {
            expected_version,
            input,
        } => expectation("computer", &input.computer_id, *expected_version),
        _ => return vec![],
    };
    vec![item]
}
impl PreparedMutation {
    /// Includes read dependencies for frozen QA jobs and both mockup resources.
    /// Design checks use content tokens, not numeric version histories.
    pub fn expected_versions(&self) -> Vec<ResourceExpectation> {
        let mut result = Vec::new();
        for change in &self.mutation.changes {
            result.extend(expectations(change));
            match change {
                Change::Qa(QaWrite::StartRun { jobs, .. }) => result.extend(
                    jobs.iter()
                        .map(|v| expectation("qa.job", v.job_id, v.expected_version)),
                ),
                Change::Mockup(write) => {
                    let (id, _, accepted, working) = mockup_guard(write);
                    result.push(expectation("mockup.accepted", id, accepted));
                    result.push(expectation("mockup.working", id, working));
                }
                _ => {}
            }
        }
        result
    }
    /// Call within the write transaction BEFORE evaluating stale guards.
    pub fn replay(
        &self,
        receipt: Option<&OperationReceipt>,
    ) -> StorageResult<Option<CommitResult>> {
        match receipt {
            None => Ok(None),
            Some(OperationReceipt::V1 {
                fingerprint,
                result,
            }) if fingerprint == &self.fingerprint => Ok(Some(result.clone())),
            Some(_) => Err(StorageError::OperationReused),
        }
    }
    pub fn receipt(&self, result: CommitResult) -> OperationReceipt {
        OperationReceipt::V1 {
            fingerprint: self.fingerprint.clone(),
            result,
        }
    }
}
/// Current versions must come from the write transaction. Absent resources use
/// 0; deleted identities retain tombstone versions.
pub fn check_versions(
    expected: &[ResourceExpectation],
    current: &[ResourceVersion],
) -> StorageResult<()> {
    let versions = current
        .iter()
        .map(|r| {
            (
                (r.resource_kind.as_str(), r.resource_id.as_str()),
                r.version,
            )
        })
        .collect::<BTreeMap<_, _>>();
    let conflicts = expected
        .iter()
        .filter_map(|r| {
            let version = versions
                .get(&(r.resource_kind.as_str(), r.resource_id.as_str()))
                .copied()
                .unwrap_or(0);
            (version != r.expected_version).then(|| ResourceConflict {
                resource_kind: r.resource_kind.clone(),
                resource_id: r.resource_id.clone(),
                expected_version: r.expected_version,
                current_version: version,
            })
        })
        .collect::<Vec<_>>();
    if conflicts.is_empty() {
        Ok(())
    } else {
        Err(StorageError::Conflict(conflicts))
    }
}
/// The affected set includes every document a cascade removes. The backend
/// computes this closure and loads complete canonical snapshots atomically.
pub fn check_document_tokens(
    submitted: &[DocumentReadToken],
    affected: &[DesignDocument],
) -> StorageResult<()> {
    tokens(submitted)?;
    let supplied = submitted
        .iter()
        .map(|r| (r.document_id.as_str(), r.read_token.as_str()))
        .collect::<BTreeMap<_, _>>();
    let mut conflicts = Vec::new();
    for current in affected {
        let code = match supplied.get(current.document_id.as_str()) {
            Some(token) if *token != current.read_token => DocumentConflictCode::OutOfDate,
            None if !current.document.is_null() => DocumentConflictCode::ReadRequired,
            _ => continue,
        };
        conflicts.push(DocumentConflict {
            code,
            document_id: current.document_id.clone(),
            current_document: current.document.clone(),
            read_token: current.read_token.clone(),
        });
    }
    if conflicts.is_empty() {
        Ok(())
    } else {
        Err(StorageError::Documents(conflicts))
    }
}
/// Resolve every edge against the final staged graph, including retained
/// incoming links on delete. References created in the same batch are valid.
pub fn check_references(
    edges: &[MissingReference],
    final_resources: &BTreeSet<ResourceKey>,
) -> StorageResult<()> {
    let missing = edges
        .iter()
        .filter(|edge| !final_resources.contains(&edge.target))
        .cloned()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(StorageError::References(missing))
    }
}
