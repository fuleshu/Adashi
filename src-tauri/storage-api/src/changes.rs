use crate::{
    coordination::*, design::*, documents::*, fixed_hooks::*, memory::*, mockups::*, qa::*,
    rules::*, tasks::*, *,
};
use serde::{Deserialize, Serialize};

/// The operation id covers the entire ordered batch. Cross-domain references
/// are checked against the final staged state, not against a stale preflight.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Mutation {
    pub operation_id: String,
    pub changes: Vec<Change>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(
    tag = "domain",
    content = "change",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum Change {
    Design(DesignWrite),
    Mockup(MockupWrite),
    Task(TaskWrite),
    Qa(QaWrite),
    Memory(MemoryWrite),
    Rule(RuleWrite),
    FixedPrompt(FixedPromptWrite),
    /// Explicit registration write only; opening/reading never updates paths.
    Computer {
        expected_version: i64,
        input: ComputerCheckout,
    },
    /// Migration/import preserves old content even without an editing UI.
    Legacy {
        expected_version: i64,
        content: LegacyContent,
    },
    HealthWaiver {
        external_id: String,
        state: health::ElementHealth,
        reason: String,
        task_id: Option<i64>,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum DesignWrite {
    /// Existing desktop edit form: preserve hierarchy inside the transaction.
    EditElement {
        external_id: String,
        expected_version: i64,
        name: String,
        description: String,
        technology: String,
        tags: String,
    },
    /// Existing desktop edit form: preserve endpoints inside the transaction.
    EditRelationship {
        external_id: String,
        expected_version: i64,
        description: String,
        technology: String,
        tags: String,
    },
    Save {
        change_intent: String,
        changes: Vec<DesignChange>,
        read_tokens: Vec<DocumentReadToken>,
    },
    Describe {
        updates: Vec<ElementDescriptionUpdate>,
        read_tokens: Vec<DocumentReadToken>,
    },
}

/// Inputs keep their established transport fields. Nested operation ids must
/// equal Mutation.operation_id. Accepted/working guards remain independent.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    content = "input",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum MockupWrite {
    Create(CreateMockupInput),
    SaveDraft(SaveDraftInput),
    RequestRevision(MockupMutationInput),
    ResumeEditing(MockupMutationInput),
    Propose(ProposeMockupInput),
    AcceptProposal(MockupMutationInput),
    RejectProposal(MockupMutationInput),
    DiscardDraft(MockupMutationInput),
    Delete(MockupMutationInput),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum TaskWrite {
    Create {
        input: NewTask,
    },
    Update {
        expected_version: i64,
        input: UpdateTask,
    },
    Finish {
        expected_version: i64,
        input: FinishTask,
    },
    Close {
        id: i64,
        expected_version: i64,
    },
    Delete {
        id: i64,
        expected_version: i64,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QaExecutionPlan {
    pub job_id: i64,
    pub expected_version: i64,
    /// The historical command snapshot has the existing serialized JSON shape.
    pub command_snapshot: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QaJobOutcome {
    Passed,
    Failed,
    TimedOut,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QaEvidence {
    pub outcome: QaJobOutcome,
    pub exit_code: Option<i64>,
    pub duration_ms: i64,
    pub output: String,
}

/// Process execution is outside storage transactions. StartRun atomically
/// freezes the selected job versions and command snapshots; workers write
/// evidence in short guarded commits. Retention occurs with CompleteRun so
/// evidence belonging to active runs cannot be pruned before aggregation.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum QaWrite {
    CreateJob {
        input: NewQaJob,
    },
    UpdateJob {
        expected_version: i64,
        input: UpdateQaJob,
    },
    DeleteJob {
        id: i64,
        expected_version: i64,
    },
    StartRun {
        query: QaJobQuery,
        trigger_source: String,
        jobs: Vec<QaExecutionPlan>,
    },
    CompleteJob {
        job_run_id: i64,
        expected_version: i64,
        evidence: QaEvidence,
    },
    /// Reserve execution exactly once before launching a process. Only an
    /// unclaimed running job (version 1) may be claimed; retries never rerun it.
    ClaimJob {
        job_run_id: i64,
        expected_version: i64,
    },
    CompleteRun {
        run_id: i64,
        expected_version: i64,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum MemoryWrite {
    Append {
        note: AppendMemoryNote,
    },
    /// Caller authorization remains in the application. Resolves only the
    /// explicitly reviewed notes; preserves their provenance within retention.
    Compact {
        expected_version: i64,
        summary: String,
        superseded_note_ids: Vec<String>,
    },
    Protocol {
        expected_version: i64,
        rule: String,
    },
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RuleWrite {
    Create {
        input: NewRule,
    },
    Update {
        id: i64,
        expected_version: i64,
        input: NewRule,
    },
    Delete {
        id: i64,
        expected_version: i64,
    },
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FixedPromptWrite {
    pub key: String,
    pub expected_version: i64,
    pub prompt: String,
}

/// Exactly one outcome for each Change, in request order, persisted in the
/// receipt. Generated integer IDs/numbers are returned here and never reused.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "domain", content = "value", rename_all = "camelCase")]
pub enum ChangeOutcome {
    Design(DesignSaveResult),
    Mockup(Option<UiMockup>),
    Task(Option<Task>),
    QaJob(Option<QaJob>),
    QaRun(QaRun),
    QaEvidence(QaJobRun),
    Memory(ProjectMemory),
    Rule(Option<Rule>),
    FixedPrompt(FixedHookPrompt),
    Computer(ComputerCheckout),
    Legacy,
    HealthWaiver(health::HealthWaiver),
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitResult {
    pub cursor: ChangeCursor,
    pub revision: i64,
    pub changed: bool,
    pub outcomes: Vec<ChangeOutcome>,
    pub versions: Vec<ResourceVersion>,
    pub read_tokens: Vec<DocumentReadToken>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(
    tag = "format",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum OperationReceipt {
    V1 {
        fingerprint: String,
        result: CommitResult,
    },
    /// Preserved old receipts cannot prove request equality; never replay them
    /// as a new-format mutation. Transport migration may inspect them separately.
    Legacy { payload: serde_json::Value },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentConflict {
    pub code: DocumentConflictCode,
    pub document_id: String,
    pub current_document: serde_json::Value,
    pub read_token: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DocumentConflictCode {
    OutOfDate,
    ReadRequired,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingReference {
    pub source: ResourceKey,
    pub target: ResourceKey,
}

/// Only shared preparation can construct this. A backend MUST still check
/// stateful guards, domain invariants and references inside its transaction.
#[derive(Clone, Debug)]
pub struct PreparedMutation {
    pub(crate) mutation: Mutation,
    pub(crate) fingerprint: String,
}
impl PreparedMutation {
    pub fn mutation(&self) -> &Mutation {
        &self.mutation
    }
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
}
