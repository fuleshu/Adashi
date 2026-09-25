use crate::concurrency::ResourceIntent;
use crate::design::{
    self, DesignChange, DesignOverviewResult, DesignSaveResult, DesignScopeResult,
    DesignSearchResult, ElementDescriptionUpdate,
};
use crate::design_health;
use crate::grep::{self, GrepParams};
use crate::memory::{AppendMemoryNote, MemoryNote, ProjectMemory};
use crate::mockups::{self, MockupSummary, UiMockup};
use crate::project::{open_project_store, resolve_project_from_settings};
use crate::qa::{
    self, NewQaJob, QaDesignLinkInput, QaJob, QaJobQuery, QaJobSummary, QaRun, QaRunSummary,
    UpdateQaJob,
};
use crate::rules::{NewRule, Rule};
use crate::settings::{self, AppSettings, ProjectSettings};
#[cfg(test)]
use crate::state as project_state;
use crate::storage::api::{
    BindingQuery, Change, ChangeOutcome, DesignSearchQuery, DesignWrite, IntentUpdate, MemoryWrite,
    Mutation, QaWrite, ReadSnapshot, ResourceKey, RuleWrite, ScopeQuery, TaskQuery, TaskWrite,
};
use crate::storage::{ProjectStorage, ProjectStore, StorageError};
use crate::tasks::{
    self, FinishTask, NewTask, Task, TaskDesignSpecificationLink, TaskDesignSpecificationLinkInput,
    UpdateTask,
};
#[cfg(test)]
use crate::{concurrency, storage::RuleStorage};
#[cfg(test)]
use crate::{memory, rules};
use rmcp::handler::server::tool::IntoCallToolResult;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::model::{CallToolResult, ContentBlock, ErrorData};
use rmcp::transport::stdio;
use rmcp::{serve_server, tool, tool_router};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
mod context;
mod errors;
mod help;
mod markdown;
use base64::Engine as _;
use context::{MemoryContext, RuleInjectionResult};
use std::path::PathBuf;

/// Publishes non-negative counts using portable JSON Schema validation keywords.
///
/// Schemars labels Rust `usize` values with its custom `uint` format. JSON Schema
/// clients may ignore that annotation, so MCP-facing count fields use `minimum`
/// instead while retaining their native Rust representation.
fn nonnegative_count_schema(_: &mut rmcp::schemars::SchemaGenerator) -> rmcp::schemars::Schema {
    rmcp::schemars::json_schema!({
        "type": "integer",
        "minimum": 0,
    })
}
#[derive(Clone)]
pub struct AdashiMcpServer {
    settings_path: PathBuf,
}

impl AdashiMcpServer {
    pub fn new(settings_path: PathBuf) -> Self {
        Self { settings_path }
    }

    fn load_settings(&self) -> Result<AppSettings, ErrorData> {
        settings::load_or_init(&self.settings_path).map_err(internal_error)
    }

    fn open_project_storage(
        &self,
        project_name: Option<&str>,
    ) -> Result<(ProjectSettings, ProjectStore), ErrorData> {
        let settings = self.load_settings()?;
        let project = resolve_project_from_settings(&settings, project_name)
            .map_err(|err| ErrorData::invalid_params(err, None))?;
        let store = open_project_store(&project).map_err(storage_error)?;
        Ok((project, store))
    }

    /// The router method's body, reachable from other modules' tests without widening the
    /// tool surface itself.
    #[cfg(test)]
    pub(crate) fn grep_result_for_tests(
        &self,
        params: &GrepParams,
    ) -> Result<grep::GrepResult, String> {
        let (_project, mut store) = self
            .open_project_storage(Some(&params.project_name))
            .map_err(|e| e.to_string())?;
        let snapshot = store.snapshot().map_err(|e| e.to_string())?;
        snapshot.search(params).map_err(|e| e.to_string())
    }
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProjectParams {
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetMemoryParams {
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    /// Literal case-insensitive substring in retained note bodies.
    query: Option<String>,
    /// Exact note id, so a `memory:<noteId>` locator is drillable.
    note_id: Option<String>,
    run_id: Option<String>,
    task_id: Option<i64>,
    #[serde(default)]
    include_superseded: bool,
}

#[derive(Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MemoryReadResult {
    project_id: String,
    project_name: String,
    revision: i64,
    memory: ProjectMemory,
    retained_notes: u32,
    matched_notes: u32,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RuleInjectionParams {
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    intend: String,
    hook: String,
    /// Summary by default; protocolOnly skips the summary for operational work.
    #[serde(default)]
    memory_context: MemoryContext,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateRuleParams {
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    operation_id: String,
    name: String,
    enabled: bool,
    intend: String,
    hook: String,
    prompt: String,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateRuleParams {
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    operation_id: String,
    expected_version: i64,
    rule_id: i64,
    name: String,
    enabled: bool,
    intend: String,
    hook: String,
    prompt: String,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DeleteRuleParams {
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    operation_id: String,
    expected_version: i64,
    rule_id: i64,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateTaskParams {
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    operation_id: String,
    title: String,
    description: Option<String>,
    design_specification_links: Option<Vec<TaskDesignSpecificationLinkInput>>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateTaskParams {
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    operation_id: String,
    expected_version: i64,
    task_id: i64,
    title: Option<String>,
    description: Option<String>,
    state: Option<String>,
    design_specification_links: Option<Vec<TaskDesignSpecificationLinkInput>>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListTasksParams {
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    /// Omitted returns todo, active and finished so closed history stays out of the way; []
    /// selects nothing. Values are exact.
    states: Option<Vec<tasks::TaskState>>,
    /// Default 25, range 1..=100.
    limit: Option<u32>,
    /// Opaque continuation; keep the same states.
    cursor: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CloseTaskParams {
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    /// Idempotency key.
    operation_id: String,
    /// Expected task version.
    expected_version: i64,
    task_id: i64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TaskCursor {
    contract_version: u32,
    project_id: String,
    revision: i64,
    states: Option<Vec<tasks::TaskState>>,
    after_id: i64,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TaskIdParams {
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    task_id: i64,
    /// get: also inline each linked design specification's scope and mockup content. Off by
    /// default, because a task with many links otherwise returns a very large payload; prefer
    /// retrieving the one or two scopes the work actually needs.
    #[serde(default)]
    include_design_scopes: bool,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DeleteTaskParams {
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    task_id: i64,
    operation_id: String,
    expected_version: i64,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FinishTaskParams {
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    operation_id: String,
    expected_version: i64,
    task_id: i64,
    completion_memo: String,
    created_files: Option<Vec<String>>,
    changed_files: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListQaJobsParams {
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    query: Option<QaJobQuery>,
    /// Default 25, range 1..=100.
    limit: Option<i64>,
    /// Opaque continuation; keep the same query.
    cursor: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QaJobIdParams {
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    qa_job_id: i64,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QaRunIdParams {
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    qa_run_id: i64,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DeleteQaJobParams {
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    qa_job_id: i64,
    operation_id: String,
    expected_version: i64,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateQaJobParams {
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    operation_id: String,
    name: String,
    description: Option<String>,
    command: String,
    working_directory: Option<String>,
    shell: Option<String>,
    timeout_seconds: Option<i64>,
    enabled: Option<bool>,
    design_specification_links: Option<Vec<QaDesignLinkInput>>,
    task_ids: Option<Vec<i64>>,
    tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateQaJobParams {
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    operation_id: String,
    expected_version: i64,
    qa_job_id: i64,
    name: Option<String>,
    description: Option<String>,
    command: Option<String>,
    working_directory: Option<String>,
    shell: Option<String>,
    timeout_seconds: Option<i64>,
    enabled: Option<bool>,
    design_specification_links: Option<Vec<QaDesignLinkInput>>,
    task_ids: Option<Vec<i64>>,
    tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RunQaJobsParams {
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    operation_id: String,
    query: QaJobQuery,
    trigger_source: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListQaRunsParams {
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    limit: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QaJobCursor {
    contract_version: u32,
    project_id: String,
    revision: i64,
    state_rank: i32,
    number: i64,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateMemoryParams {
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    operation_id: String,
    expected_version: i64,
    memory: String,
    /// Authorized coordinator only: exact reviewed note ids covered by the summary.
    #[serde(default)]
    superseded_note_ids: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateMemoryRuleParams {
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    operation_id: String,
    expected_version: i64,
    rule: String,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AppendMemoryNoteParams {
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    note_id: String,
    operation_id: String,
    run_id: String,
    task_id: Option<i64>,
    body: String,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PublishIntentParams {
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    agent_run_id: String,
    resource_kind: String,
    resource_id: String,
    ttl_seconds: i64,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DesignOverviewParams {
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    #[serde(default)]
    #[schemars(schema_with = "nonnegative_count_schema")]
    max_depth: Option<usize>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DesignScopeParams {
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    element_id: String,
    include_ancestors: Option<bool>,
    #[serde(default)]
    #[schemars(schema_with = "nonnegative_count_schema")]
    children_depth: Option<usize>,
    include_source: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DesignSearchParams {
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    query: String,
    kinds: Option<Vec<String>>,
    #[serde(default)]
    #[schemars(schema_with = "nonnegative_count_schema")]
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DesignByIdsParams {
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    ids: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DesignBindingsParams {
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    files: Option<Vec<String>>,
    symbols: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DesignSaveParams {
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    /// Unique mutation id; reuse only for an identical retry.
    operation_id: String,
    /// Copy documentId/readToken pairs from complete design reads for existing targets.
    /// New documents need no token.
    #[serde(default)]
    read_tokens: Vec<design::documents::DocumentReadToken>,
    change_intent: String,
    changes: Vec<DesignChange>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SetElementDescriptionsParams {
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    operation_id: String,
    read_tokens: Vec<design::documents::DocumentReadToken>,
    updates: Vec<ElementDescriptionUpdate>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MockupContextParams {
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    external_id: String,
}

#[derive(Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MockupPendingResult {
    project_id: String,
    project_name: String,
    revision: i64,
    mockups: Vec<MockupSummary>,
}

#[derive(Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RuleListResult {
    project_id: String,
    project_name: String,
    rules: Vec<Rule>,
}

#[derive(Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TaskListResult {
    contract_version: u32,
    project_id: String,
    project_name: String,
    revision: i64,
    tasks: Vec<tasks::TaskSummary>,
    filtered_total: i64,
    /// Closed tasks the default filter withheld, so the omission is stated rather than silent.
    /// Always 0 once `states` is given explicitly.
    closed_hidden: i64,
    has_more: bool,
    next_cursor: Option<String>,
}

#[derive(Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TaskReadResult {
    project_id: String,
    project_name: String,
    revision: i64,
    task: Task,
    /// One entry per linked design specification. `scope` and `mockup` are only filled when the
    /// caller asked for them with `includeDesignScopes`, so a task read stays small however many
    /// links the task carries.
    design_specifications: Vec<TaskDesignSpecificationBranch>,
    /// How to obtain the design detail for the links above.
    design_scope_hint: String,
}

#[derive(Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TaskDesignSpecificationBranch {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    documents: Vec<adashi_storage_api::documents::DesignDocument>,
    link: TaskDesignSpecificationLink,
    scope: Option<DesignScopeResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mockup: Option<UiMockup>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mockup_preview: Option<TaskMockupPreview>,
    note: Option<String>,
}

#[derive(Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TaskMockupPreview {
    variant: String,
    mime_type: String,
    #[schemars(schema_with = "nonnegative_count_schema")]
    content_index: usize,
}

#[derive(Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateRuleResult {
    project_id: String,
    rule_id: i64,
}

#[derive(Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateRuleResult {
    project_id: String,
    updated_rule_id: i64,
    revision: i64,
}

#[derive(Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DeleteRuleResult {
    project_id: String,
    deleted_rule_id: i64,
    revision: i64,
}

#[derive(Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TaskMutationResult {
    project_id: String,
    project_name: String,
    revision: i64,
    task: Task,
    design_specifications: Vec<TaskDesignSpecificationBranch>,
}

#[derive(Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DeleteTaskResult {
    project_id: String,
    project_name: String,
    revision: i64,
    deleted_task_id: i64,
}

#[derive(Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QaJobListResult {
    contract_version: u32,
    project_id: String,
    project_name: String,
    revision: i64,
    jobs: Vec<QaJobSummary>,
    filtered_total: i64,
    has_more: bool,
    next_cursor: Option<String>,
}

#[derive(Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QaJobResult {
    project_id: String,
    project_name: String,
    revision: i64,
    job: QaJob,
}

#[derive(Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DeleteQaJobResult {
    project_id: String,
    project_name: String,
    revision: i64,
    deleted_qa_job_id: i64,
}

#[derive(Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QaRunResult {
    project_id: String,
    project_name: String,
    revision: i64,
    run: QaRun,
}

#[derive(Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QaRunListResult {
    project_id: String,
    project_name: String,
    revision: i64,
    runs: Vec<QaRunSummary>,
}

#[derive(Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MemoryResult {
    project_id: String,
    project_name: String,
    revision: i64,
    memory: ProjectMemory,
}

#[derive(Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MemoryNoteResult {
    project_id: String,
    project_name: String,
    revision: i64,
    note: MemoryNote,
}

#[derive(Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResourceIntentResult {
    project_id: String,
    project_name: String,
    intent: ResourceIntent,
}

#[derive(Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResourceIntentListResult {
    project_id: String,
    project_name: String,
    intents: Vec<ResourceIntent>,
}

/// Bounded QA job-listing budget, mirroring the task-list contract.
const QA_JOB_LIST_DEFAULT_LIMIT: i64 = 25;
const QA_JOB_LIST_MAX_LIMIT: i64 = 100;

/// Capability menu for the consolidated design/mockup tool. The model reads these
/// values straight from the JSON schema, so no memorization is required.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
enum DesignOperation {
    ListMarkdown,
    Save,
    GetScope,
    GetByIds,
    GetDocuments,
    Search,
    GetOverview,
    GetBindings,
    SetElementDescriptions,
    // Design-to-code correspondence: which bindings resolve, which elements claim nothing, and
    // where bound code references what the model does not declare. A line comment rather than a
    // doc comment on purpose: a documented variant makes schemars emit `oneOf` with titles
    // instead of the flat string enum clients already read.
    Health,
    MockupListPendingRevisions,
    MockupGetRevisionContext,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
enum TasksOperation {
    Create,
    List,
    Update,
    Finish,
    Close,
    Delete,
    Get,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
enum QaOperation {
    CreateJob,
    UpdateJob,
    DeleteJob,
    ListJobs,
    RunJobs,
    ListRuns,
    GetJob,
    GetRun,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
enum MemoryOperation {
    Get,
    Append,
    Update,
    UpdateRule,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
enum RulesOperation {
    List,
    Create,
    Update,
    Delete,
    GetRuleInjections,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
enum IntentsOperation {
    Publish,
    List,
}

/// Consolidated design/mockup arguments. `operation` selects the sub-API; fields not
/// used by the selected operation are ignored. Per-operation required fields are
/// validated at call time.
#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DesignParams {
    /// list_markdown: bounded metadata, filters and nextAfterId continuation.
    markdown_query: Option<adashi_storage_api::markdown::MarkdownQuery>,
    operation: DesignOperation,
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    /// get_overview: how deep to expand the C4/UML tree.
    #[serde(default)]
    #[schemars(schema_with = "nonnegative_count_schema")]
    max_depth: Option<usize>,
    /// get_scope: root element or Markdown external id.
    element_id: Option<String>,
    /// get_scope: include ancestors.
    include_ancestors: Option<bool>,
    /// get_scope: child expansion depth.
    #[serde(default)]
    #[schemars(schema_with = "nonnegative_count_schema")]
    children_depth: Option<usize>,
    /// get_scope: include canonical source.
    include_source: Option<bool>,
    /// search: text query.
    query: Option<String>,
    /// search: kinds to restrict results to.
    kinds: Option<Vec<String>>,
    /// search: result limit.
    #[serde(default)]
    #[schemars(schema_with = "nonnegative_count_schema")]
    limit: Option<usize>,
    /// get_by_ids: stored design ids. get_documents: opaque documentId values from retrieval.
    ids: Option<Vec<String>>,
    /// get_bindings: files to resolve bindings for.
    files: Option<Vec<String>>,
    /// get_bindings: symbols to resolve bindings for.
    symbols: Option<Vec<String>>,
    /// save/set_element_descriptions: unique mutation id; reuse only for an identical retry.
    operation_id: Option<String>,
    /// save/set_element_descriptions: copy documentId/readToken pairs from documents in
    /// get_by_ids/get_scope/get_bindings/get_documents. Required for existing targets,
    /// including documents removed by cascading deletes. Creates need no token.
    read_tokens: Option<Vec<design::documents::DocumentReadToken>>,
    /// save: human intent for the change.
    change_intent: Option<String>,
    /// save: heterogeneous changeset.
    changes: Option<Vec<DesignChange>>,
    /// set_element_descriptions: exact externalId/description updates.
    updates: Option<Vec<ElementDescriptionUpdate>>,
    /// mockup_get_revision_context: mockup external id.
    external_id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TasksParams {
    operation: TasksOperation,
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    /// create/update/finish/close/delete: idempotency key.
    operation_id: Option<String>,
    /// create (required) / update: task title.
    title: Option<String>,
    /// create/update: task description.
    description: Option<String>,
    /// create/update: ordered design specification links. A task is a unit of work, so keep the
    /// list to the specifications this task implements; supporting context belongs in the task
    /// description or in a linked design scope you retrieve when you need it.
    design_specification_links: Option<Vec<TaskDesignSpecificationLinkInput>>,
    /// update: the state to move the task to. Allowed values: todo, active, finished, closed.
    /// A task is created as todo, worked on as active, reported complete as finished, and only a
    /// review closes it as closed. Setting a finished task back to active clears its completion
    /// timestamp; todo cannot be re-entered, and closed can only be re-opened to active.
    state: Option<String>,
    /// update/finish/close/delete: expected task version.
    expected_version: Option<i64>,
    /// update/finish/close/delete/get: task id.
    task_id: Option<i64>,
    /// finish: completion memo.
    completion_memo: Option<String>,
    /// finish: files created.
    created_files: Option<Vec<String>>,
    /// finish: files changed.
    changed_files: Option<Vec<String>>,
    /// list: states filter. Omitted returns todo, active and finished, so closed history stays
    /// out of the way; pass ["closed"] or all four values to include it. [] selects nothing.
    states: Option<Vec<tasks::TaskState>>,
    /// get: also inline each linked design specification's scope and mockup content. Off by
    /// default, because a task with many links otherwise returns a very large payload; prefer
    /// retrieving the one or two scopes the work actually needs.
    #[serde(default)]
    include_design_scopes: bool,
    /// list: page size (default 25, 1..=100).
    limit: Option<u32>,
    /// list: opaque continuation cursor.
    cursor: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QaParams {
    operation: QaOperation,
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    /// list_jobs / run_jobs: selection filters.
    query: Option<QaJobQuery>,
    /// get_job/update_job/delete_job: job id.
    qa_job_id: Option<i64>,
    /// get_run: run id.
    qa_run_id: Option<i64>,
    /// create_job/update_job/delete_job/run_jobs: idempotency key.
    operation_id: Option<String>,
    /// update_job/delete_job: expected job version.
    expected_version: Option<i64>,
    /// create_job/update_job: name, command, etc.
    name: Option<String>,
    description: Option<String>,
    command: Option<String>,
    working_directory: Option<String>,
    shell: Option<String>,
    timeout_seconds: Option<i64>,
    enabled: Option<bool>,
    design_specification_links: Option<Vec<QaDesignLinkInput>>,
    task_ids: Option<Vec<i64>>,
    tags: Option<Vec<String>>,
    /// run_jobs: trigger source label.
    trigger_source: Option<String>,
    /// list_jobs (default 25) / list_runs (default 20): page size, range 1..=100.
    limit: Option<i64>,
    /// list_jobs: opaque continuation cursor.
    cursor: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MemoryParams {
    operation: MemoryOperation,
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    /// get: literal case-insensitive substring in note bodies.
    query: Option<String>,
    /// get (filter) / append (note run id).
    run_id: Option<String>,
    /// get (filter) / append (note task id).
    task_id: Option<i64>,
    /// get: include resolved notes with provenance.
    #[serde(default)]
    include_superseded: bool,
    /// append: note id.
    note_id: Option<String>,
    /// append: note body.
    body: Option<String>,
    /// append/update/update_rule: idempotency key.
    operation_id: Option<String>,
    /// update/update_rule: expected version.
    expected_version: Option<i64>,
    /// update: replacement shared summary.
    memory: Option<String>,
    /// update: reviewed note ids resolved by this summary.
    #[serde(default)]
    superseded_note_ids: Vec<String>,
    /// update_rule: replacement memory protocol rule.
    rule: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RulesParams {
    operation: RulesOperation,
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    /// get_rule_injections/create/update: rule intend.
    intend: Option<String>,
    /// get_rule_injections/create/update: lifecycle hook.
    hook: Option<String>,
    /// get_rule_injections: memory context mode.
    #[serde(default)]
    memory_context: MemoryContext,
    /// create/update/delete: idempotency key.
    operation_id: Option<String>,
    /// create/update: rule name.
    name: Option<String>,
    /// create/update: enabled flag.
    enabled: Option<bool>,
    /// create/update: rule prompt.
    prompt: Option<String>,
    /// update/delete: expected rule version.
    expected_version: Option<i64>,
    /// update/delete: rule id.
    rule_id: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct IntentsParams {
    operation: IntentsOperation,
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    /// publish: agent run id.
    agent_run_id: Option<String>,
    /// publish: resource kind.
    resource_kind: Option<String>,
    /// publish: resource id.
    resource_id: Option<String>,
    /// publish: time-to-live in seconds.
    ttl_seconds: Option<i64>,
}

#[tool_router]
impl AdashiMcpServer {
    fn list_rules(
        &self,
        Parameters(params): Parameters<ProjectParams>,
    ) -> Result<Json<RuleListResult>, ErrorData> {
        let (project, mut store) = self.open_project_storage(Some(params.project_name.as_str()))?;
        let rules = store
            .snapshot()
            .map_err(storage_error)?
            .rules()
            .map_err(storage_error)?;
        Ok(Json(RuleListResult {
            project_id: project.id,
            project_name: project.name,
            rules,
        }))
    }

    fn get_rule_injections(
        &self,
        Parameters(params): Parameters<RuleInjectionParams>,
    ) -> Result<Json<RuleInjectionResult>, ErrorData> {
        let (project, mut store) = self.open_project_storage(Some(params.project_name.as_str()))?;
        let db = store.snapshot().map_err(storage_error)?;
        context::build(
            db.as_ref(),
            project,
            &params.intend,
            &params.hook,
            params.memory_context,
        )
        .map(Json)
        .map_err(tool_error)
    }

    fn list_tasks(
        &self,
        Parameters(mut params): Parameters<ListTasksParams>,
    ) -> Result<Json<TaskListResult>, ErrorData> {
        let limit = params.limit.unwrap_or(25);
        if !(1..=100).contains(&limit) {
            return Err(tool_error(
                "tasks.invalid_limit: limit must be 1..=100".into(),
            ));
        }
        if let Some(states) = &mut params.states {
            states.sort();
            states.dedup();
        }
        // The resolved filter travels in the cursor too, so a paged listing keeps the same view
        // it started with, default or explicit.
        let state_filter = tasks::default_state_filter(params.states.as_deref());
        let (project, mut store) = self.open_project_storage(Some(params.project_name.as_str()))?;
        let db = store.snapshot().map_err(storage_error)?;
        // A default listing withholds closed work; state how much, so the omission is visible
        // rather than something the caller has to discover.

        let revision = db.metadata().revision;
        let after_id = if let Some(encoded) = params.cursor {
            if encoded.len() > 4096 {
                return Err(tool_error("tasks.invalid_cursor".into()));
            }
            let cursor: TaskCursor = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(encoded)
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .ok_or_else(|| tool_error("tasks.invalid_cursor".into()))?;
            if cursor.contract_version != 2
                || cursor.project_id != project.id
                || cursor.states.as_deref() != Some(state_filter.as_slice())
                || cursor.after_id <= 0
            {
                return Err(tool_error(
                    "tasks.invalid_cursor: keep the original project and states".into(),
                ));
            }
            if cursor.revision != revision {
                return Err(tool_error(
                    "tasks.stale_cursor: project changed; restart without cursor".into(),
                ));
            }
            cursor.after_id
        } else {
            0
        };
        let page = db
            .task_page(&TaskQuery {
                states: state_filter.clone(),
                after_id: Some(after_id),
                limit: limit as usize + 1,
            })
            .map_err(storage_error)?;
        let (mut tasks, filtered_total) = (page.tasks, page.total);
        let closed_hidden = if params.states.is_none() {
            page.closed_count
        } else {
            0
        };
        let has_more = tasks.len() > limit as usize;
        tasks.truncate(limit as usize);
        let next_cursor = if has_more {
            let cursor = TaskCursor {
                contract_version: 2,
                project_id: project.id.clone(),
                revision,
                states: Some(state_filter.clone()),
                after_id: tasks.last().unwrap().id,
            };
            Some(
                base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .encode(serde_json::to_vec(&cursor).map_err(internal_error)?),
            )
        } else {
            None
        };
        Ok(Json(TaskListResult {
            contract_version: 2,
            project_id: project.id,
            project_name: project.name,
            revision,
            tasks,
            filtered_total,
            closed_hidden,
            has_more,
            next_cursor,
        }))
    }

    fn get_task(
        &self,
        Parameters(params): Parameters<TaskIdParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let (project, mut store) = self.open_project_storage(Some(params.project_name.as_str()))?;
        let db = store.snapshot().map_err(storage_error)?;
        let revision = db.metadata();
        let task = db.task(params.task_id).map_err(storage_error)?;
        let mut design_specifications =
            load_task_design_specifications(db.as_ref(), &task, params.include_design_scopes)
                .map_err(tool_error)?;
        let mut previews = Vec::new();
        for specification in &mut design_specifications {
            let Some(mockup) = specification.mockup.as_ref() else {
                continue;
            };
            let png = mockups::preview_base64_uncached(mockup, "accepted").map_err(tool_error)?;
            specification.mockup_preview = Some(TaskMockupPreview {
                variant: "accepted".to_string(),
                mime_type: "image/png".to_string(),
                content_index: previews.len() + 1,
            });
            previews.push(ContentBlock::image(png, "image/png"));
        }

        let design_scope_hint = if params.include_design_scopes {
            "Scopes are inlined for the links above because includeDesignScopes was set."
                .to_string()
        } else {
            format!(
                "Link metadata only. Retrieve just the scopes this work needs with the adashi_design \
                 get_scope operation, or repeat this read with includeDesignScopes: true for all {} \
                 linked scope(s) at once.",
                design_specifications.len()
            )
        };

        let payload = TaskReadResult {
            project_id: project.id,
            project_name: project.name,
            revision: revision.revision,
            task,
            design_specifications,
            design_scope_hint,
        };
        let structured = serde_json::to_value(&payload).map_err(internal_error)?;
        let mut result = CallToolResult::structured(structured);
        result.content.extend(previews);
        Ok(result)
    }

    fn get_memory(
        &self,
        Parameters(params): Parameters<GetMemoryParams>,
    ) -> Result<Json<MemoryReadResult>, ErrorData> {
        let (project, mut store) = self.open_project_storage(Some(params.project_name.as_str()))?;
        let db = store.snapshot().map_err(storage_error)?;
        let mut memory = db.memory().map_err(storage_error)?;
        let retained = db.retained_memory_notes().map_err(storage_error)?;
        let retained_notes = retained.len() as u32;
        let query = params.query.as_deref().map(str::to_lowercase);
        let note_id = params.note_id.as_deref().map(str::trim);
        memory.notes = retained
            .into_iter()
            .filter(|note| {
                (params.include_superseded || note.superseded_by_version.is_none())
                    && note_id.is_none_or(|id| note.note_id == id)
                    && query
                        .as_ref()
                        .is_none_or(|q| note.body.to_lowercase().contains(q))
                    && params.run_id.as_ref().is_none_or(|id| &note.run_id == id)
                    && params.task_id.is_none_or(|id| note.task_id == Some(id))
            })
            .collect();
        let matched_notes = memory.notes.len() as u32;
        let revision = db.metadata();

        Ok(Json(MemoryReadResult {
            project_id: project.id,
            project_name: project.name,
            revision: revision.revision,
            memory,
            retained_notes,
            matched_notes,
        }))
    }

    fn create_rule(
        &self,
        Parameters(params): Parameters<CreateRuleParams>,
    ) -> Result<Json<CreateRuleResult>, ErrorData> {
        let (project, mut store) = self.open_project_storage(Some(params.project_name.as_str()))?;
        let result = store
            .commit(Mutation {
                operation_id: params.operation_id.clone(),
                changes: vec![Change::Rule(RuleWrite::Create {
                    input: NewRule {
                        name: params.name,
                        enabled: params.enabled,
                        intend: params.intend,
                        hook: params.hook,
                        prompt: params.prompt,
                    },
                })],
            })
            .map_err(storage_error)?;
        let Some(ChangeOutcome::Rule(Some(rule))) = result.outcomes.into_iter().next() else {
            return Err(internal_error("Storage returned an unexpected outcome"));
        };
        Ok(Json(CreateRuleResult {
            project_id: project.id,
            rule_id: rule.id,
        }))
    }

    fn update_rule(
        &self,
        Parameters(params): Parameters<UpdateRuleParams>,
    ) -> Result<Json<UpdateRuleResult>, ErrorData> {
        let (project, mut store) = self.open_project_storage(Some(params.project_name.as_str()))?;
        let result = store
            .commit(Mutation {
                operation_id: params.operation_id.clone(),
                changes: vec![Change::Rule(RuleWrite::Update {
                    id: params.rule_id,
                    expected_version: params.expected_version,
                    input: NewRule {
                        name: params.name,
                        enabled: params.enabled,
                        intend: params.intend,
                        hook: params.hook,
                        prompt: params.prompt,
                    },
                })],
            })
            .map_err(storage_error)?;
        let Some(ChangeOutcome::Rule(Some(rule))) = result.outcomes.into_iter().next() else {
            return Err(internal_error("Storage returned an unexpected outcome"));
        };
        Ok(Json(UpdateRuleResult {
            project_id: project.id,
            updated_rule_id: rule.id,
            revision: result.revision,
        }))
    }

    fn delete_rule(
        &self,
        Parameters(params): Parameters<DeleteRuleParams>,
    ) -> Result<Json<DeleteRuleResult>, ErrorData> {
        let (project, mut store) = self.open_project_storage(Some(params.project_name.as_str()))?;
        let result = store
            .commit(Mutation {
                operation_id: params.operation_id.clone(),
                changes: vec![Change::Rule(RuleWrite::Delete {
                    id: params.rule_id,
                    expected_version: params.expected_version,
                })],
            })
            .map_err(storage_error)?;
        Ok(Json(DeleteRuleResult {
            project_id: project.id,
            revision: result.revision,
            deleted_rule_id: params.rule_id,
        }))
    }

    fn create_task(
        &self,
        Parameters(params): Parameters<CreateTaskParams>,
    ) -> Result<Json<TaskMutationResult>, ErrorData> {
        let (project, mut store) = self.open_project_storage(Some(params.project_name.as_str()))?;
        let result = store
            .commit(Mutation {
                operation_id: params.operation_id.clone(),
                changes: vec![Change::Task(TaskWrite::Create {
                    input: NewTask {
                        title: params.title,
                        description: params.description,
                        design_specification_links: params.design_specification_links,
                    },
                })],
            })
            .map_err(storage_error)?;
        let Some(ChangeOutcome::Task(Some(task))) = result.outcomes.into_iter().next() else {
            return Err(internal_error("Storage returned an unexpected outcome"));
        };
        let design_specifications = task_link_branches(&task);
        Ok(Json(TaskMutationResult {
            project_id: project.id,
            project_name: project.name,
            revision: result.revision,
            task,
            design_specifications,
        }))
    }

    fn update_task(
        &self,
        Parameters(params): Parameters<UpdateTaskParams>,
    ) -> Result<Json<TaskMutationResult>, ErrorData> {
        let (project, mut store) = self.open_project_storage(Some(params.project_name.as_str()))?;
        let result = store
            .commit(Mutation {
                operation_id: params.operation_id.clone(),
                changes: vec![Change::Task(TaskWrite::Update {
                    expected_version: params.expected_version,
                    input: UpdateTask {
                        task_id: params.task_id,
                        title: params.title,
                        description: params.description,
                        state: params.state,
                        design_specification_links: params.design_specification_links,
                    },
                })],
            })
            .map_err(storage_error)?;
        let Some(ChangeOutcome::Task(Some(task))) = result.outcomes.into_iter().next() else {
            return Err(internal_error("Storage returned an unexpected outcome"));
        };
        let design_specifications = task_link_branches(&task);
        Ok(Json(TaskMutationResult {
            project_id: project.id,
            project_name: project.name,
            revision: result.revision,
            task,
            design_specifications,
        }))
    }

    fn finish_task(
        &self,
        Parameters(params): Parameters<FinishTaskParams>,
    ) -> Result<Json<TaskMutationResult>, ErrorData> {
        let (project, mut store) = self.open_project_storage(Some(params.project_name.as_str()))?;
        let result = store
            .commit(Mutation {
                operation_id: params.operation_id.clone(),
                changes: vec![Change::Task(TaskWrite::Finish {
                    expected_version: params.expected_version,
                    input: FinishTask {
                        task_id: params.task_id,
                        completion_memo: params.completion_memo,
                        created_files: params.created_files.unwrap_or_default(),
                        changed_files: params.changed_files.unwrap_or_default(),
                    },
                })],
            })
            .map_err(storage_error)?;
        let Some(ChangeOutcome::Task(Some(task))) = result.outcomes.into_iter().next() else {
            return Err(internal_error("Storage returned an unexpected outcome"));
        };
        let design_specifications = task_link_branches(&task);
        Ok(Json(TaskMutationResult {
            project_id: project.id,
            project_name: project.name,
            revision: result.revision,
            task,
            design_specifications,
        }))
    }

    /// Closes a reviewed task. Only finished work can be closed, and the only way back out is a
    /// deliberate reopen to active, so a review verdict is never guessed at.
    fn close_task(
        &self,
        Parameters(params): Parameters<CloseTaskParams>,
    ) -> Result<Json<TaskMutationResult>, ErrorData> {
        let (project, mut store) = self.open_project_storage(Some(params.project_name.as_str()))?;
        let result = store
            .commit(Mutation {
                operation_id: params.operation_id.clone(),
                changes: vec![Change::Task(TaskWrite::Close {
                    id: params.task_id,
                    expected_version: params.expected_version,
                })],
            })
            .map_err(storage_error)?;
        let Some(ChangeOutcome::Task(Some(task))) = result.outcomes.into_iter().next() else {
            return Err(internal_error("Storage returned an unexpected outcome"));
        };
        let design_specifications = task_link_branches(&task);
        Ok(Json(TaskMutationResult {
            project_id: project.id,
            project_name: project.name,
            revision: result.revision,
            task,
            design_specifications,
        }))
    }

    fn delete_task(
        &self,
        Parameters(params): Parameters<DeleteTaskParams>,
    ) -> Result<Json<DeleteTaskResult>, ErrorData> {
        let (project, mut store) = self.open_project_storage(Some(params.project_name.as_str()))?;
        let result = store
            .commit(Mutation {
                operation_id: params.operation_id.clone(),
                changes: vec![Change::Task(TaskWrite::Delete {
                    id: params.task_id,
                    expected_version: params.expected_version,
                })],
            })
            .map_err(storage_error)?;
        Ok(Json(DeleteTaskResult {
            project_id: project.id,
            project_name: project.name,
            revision: result.revision,
            deleted_task_id: params.task_id,
        }))
    }

    fn list_qa_jobs(
        &self,
        Parameters(params): Parameters<ListQaJobsParams>,
    ) -> Result<Json<QaJobListResult>, ErrorData> {
        let limit = params.limit.unwrap_or(QA_JOB_LIST_DEFAULT_LIMIT);
        if !(1..=QA_JOB_LIST_MAX_LIMIT).contains(&limit) {
            return Err(tool_error(format!(
                "qa.invalid_limit: limit must be 1..={QA_JOB_LIST_MAX_LIMIT}"
            )));
        }
        let (project, mut store) = self.open_project_storage(Some(params.project_name.as_str()))?;
        let db = store.snapshot().map_err(storage_error)?;
        let revision = db.metadata().revision;
        let after = if let Some(encoded) = params.cursor {
            if encoded.len() > 4096 {
                return Err(tool_error("qa.invalid_cursor".into()));
            }
            let cursor: QaJobCursor = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(encoded)
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .ok_or_else(|| tool_error("qa.invalid_cursor".into()))?;
            if cursor.contract_version != 1
                || cursor.project_id != project.id
                || cursor.number <= 0
                || cursor.state_rank < 0
            {
                return Err(tool_error(
                    "qa.invalid_cursor: keep the original project".into(),
                ));
            }
            if cursor.revision != revision {
                return Err(tool_error(
                    "qa.stale_cursor: project changed; restart without cursor".into(),
                ));
            }
            Some((cursor.state_rank, cursor.number))
        } else {
            None
        };
        let all = db
            .qa_job_summaries(&params.query.unwrap_or_default())
            .map_err(storage_error)?;
        let filtered_total = all.len() as i64;
        let mut jobs = all
            .into_iter()
            .filter(|job| {
                after.is_none_or(|pos| (qa::state_rank(&job.derived_state), job.number) > pos)
            })
            .take(limit as usize + 1)
            .collect::<Vec<_>>();
        let has_more = jobs.len() > limit as usize;
        jobs.truncate(limit as usize);
        let next_cursor = if has_more {
            let last = jobs.last().expect("a page with more rows is never empty");
            let cursor = QaJobCursor {
                contract_version: 1,
                project_id: project.id.clone(),
                revision,
                state_rank: qa::state_rank(&last.derived_state),
                number: last.number,
            };
            Some(
                base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .encode(serde_json::to_vec(&cursor).map_err(internal_error)?),
            )
        } else {
            None
        };

        Ok(Json(QaJobListResult {
            contract_version: 1,
            project_id: project.id,
            project_name: project.name,
            revision,
            jobs,
            filtered_total,
            has_more,
            next_cursor,
        }))
    }

    fn get_qa_job(
        &self,
        Parameters(params): Parameters<QaJobIdParams>,
    ) -> Result<Json<QaJobResult>, ErrorData> {
        let (project, mut store) = self.open_project_storage(Some(params.project_name.as_str()))?;
        let db = store.snapshot().map_err(storage_error)?;
        let revision = db.metadata();
        let job = db.qa_job(params.qa_job_id).map_err(storage_error)?;

        Ok(Json(QaJobResult {
            project_id: project.id,
            project_name: project.name,
            revision: revision.revision,
            job,
        }))
    }

    fn get_qa_run(
        &self,
        Parameters(params): Parameters<QaRunIdParams>,
    ) -> Result<Json<QaRunResult>, ErrorData> {
        let (project, mut store) = self.open_project_storage(Some(params.project_name.as_str()))?;
        let db = store.snapshot().map_err(storage_error)?;
        let revision = db.metadata();
        let run = db.qa_run(params.qa_run_id).map_err(storage_error)?;

        Ok(Json(QaRunResult {
            project_id: project.id,
            project_name: project.name,
            revision: revision.revision,
            run,
        }))
    }

    fn create_qa_job(
        &self,
        Parameters(params): Parameters<CreateQaJobParams>,
    ) -> Result<Json<QaJobResult>, ErrorData> {
        let (project, mut store) = self.open_project_storage(Some(params.project_name.as_str()))?;
        let result = store
            .commit(Mutation {
                operation_id: params.operation_id.clone(),
                changes: vec![Change::Qa(QaWrite::CreateJob {
                    input: NewQaJob {
                        name: params.name,
                        description: params.description,
                        command: params.command,
                        working_directory: params.working_directory,
                        shell: params.shell,
                        timeout_seconds: params.timeout_seconds,
                        enabled: params.enabled,
                        created_by: Some("codex".to_string()),
                        design_specification_links: params.design_specification_links,
                        task_ids: params.task_ids,
                        tags: params.tags,
                    },
                })],
            })
            .map_err(storage_error)?;
        let Some(ChangeOutcome::QaJob(Some(job))) = result.outcomes.into_iter().next() else {
            return Err(internal_error("Storage returned an unexpected outcome"));
        };
        Ok(Json(QaJobResult {
            project_id: project.id,
            project_name: project.name,
            revision: result.revision,
            job,
        }))
    }

    fn update_qa_job(
        &self,
        Parameters(params): Parameters<UpdateQaJobParams>,
    ) -> Result<Json<QaJobResult>, ErrorData> {
        let (project, mut store) = self.open_project_storage(Some(params.project_name.as_str()))?;
        let result = store
            .commit(Mutation {
                operation_id: params.operation_id.clone(),
                changes: vec![Change::Qa(QaWrite::UpdateJob {
                    expected_version: params.expected_version,
                    input: UpdateQaJob {
                        qa_job_id: params.qa_job_id,
                        name: params.name,
                        description: params.description,
                        command: params.command,
                        working_directory: params.working_directory,
                        shell: params.shell,
                        timeout_seconds: params.timeout_seconds,
                        enabled: params.enabled,
                        design_specification_links: params.design_specification_links,
                        task_ids: params.task_ids,
                        tags: params.tags,
                    },
                })],
            })
            .map_err(storage_error)?;
        let Some(ChangeOutcome::QaJob(Some(job))) = result.outcomes.into_iter().next() else {
            return Err(internal_error("Storage returned an unexpected outcome"));
        };
        Ok(Json(QaJobResult {
            project_id: project.id,
            project_name: project.name,
            revision: result.revision,
            job,
        }))
    }

    fn delete_qa_job(
        &self,
        Parameters(params): Parameters<DeleteQaJobParams>,
    ) -> Result<Json<DeleteQaJobResult>, ErrorData> {
        let (project, mut store) = self.open_project_storage(Some(params.project_name.as_str()))?;
        let result = store
            .commit(Mutation {
                operation_id: params.operation_id.clone(),
                changes: vec![Change::Qa(QaWrite::DeleteJob {
                    id: params.qa_job_id,
                    expected_version: params.expected_version,
                })],
            })
            .map_err(storage_error)?;
        Ok(Json(DeleteQaJobResult {
            project_id: project.id,
            project_name: project.name,
            revision: result.revision,
            deleted_qa_job_id: params.qa_job_id,
        }))
    }

    fn run_qa_jobs(
        &self,
        Parameters(params): Parameters<RunQaJobsParams>,
    ) -> Result<Json<QaRunResult>, ErrorData> {
        let (project, mut store) = self.open_project_storage(Some(params.project_name.as_str()))?;
        let run = crate::qa_runner::run(
            &mut store,
            &project.folder,
            &params.operation_id,
            params.query,
            params.trigger_source.as_deref().unwrap_or("mcp"),
        )
        .map_err(storage_error)?;
        let revision = store.snapshot().map_err(storage_error)?.metadata().revision;
        Ok(Json(QaRunResult {
            project_id: project.id,
            project_name: project.name,
            revision,
            run,
        }))
    }

    fn list_qa_runs(
        &self,
        Parameters(params): Parameters<ListQaRunsParams>,
    ) -> Result<Json<QaRunListResult>, ErrorData> {
        let (project, mut store) = self.open_project_storage(Some(params.project_name.as_str()))?;
        let db = store.snapshot().map_err(storage_error)?;
        let revision = db.metadata();
        let runs = db
            .qa_run_summaries(params.limit.unwrap_or(20).clamp(1, 100) as usize)
            .map_err(storage_error)?;

        Ok(Json(QaRunListResult {
            project_id: project.id,
            project_name: project.name,
            revision: revision.revision,
            runs,
        }))
    }

    fn update_memory(
        &self,
        Parameters(params): Parameters<UpdateMemoryParams>,
    ) -> Result<Json<MemoryResult>, ErrorData> {
        let (project, mut store) = self.open_project_storage(Some(params.project_name.as_str()))?;
        let result = store
            .commit(Mutation {
                operation_id: params.operation_id.clone(),
                changes: vec![Change::Memory(MemoryWrite::Compact {
                    expected_version: params.expected_version,
                    summary: params.memory,
                    superseded_note_ids: params.superseded_note_ids,
                })],
            })
            .map_err(storage_error)?;
        let Some(ChangeOutcome::Memory(memory)) = result.outcomes.into_iter().next() else {
            return Err(internal_error("Storage returned an unexpected outcome"));
        };
        Ok(Json(MemoryResult {
            project_id: project.id,
            project_name: project.name,
            revision: result.revision,
            memory,
        }))
    }

    fn update_memory_rule(
        &self,
        Parameters(params): Parameters<UpdateMemoryRuleParams>,
    ) -> Result<Json<MemoryResult>, ErrorData> {
        let (project, mut store) = self.open_project_storage(Some(params.project_name.as_str()))?;
        let result = store
            .commit(Mutation {
                operation_id: params.operation_id.clone(),
                changes: vec![Change::Memory(MemoryWrite::Protocol {
                    expected_version: params.expected_version,
                    rule: params.rule,
                })],
            })
            .map_err(storage_error)?;
        let Some(ChangeOutcome::Memory(memory)) = result.outcomes.into_iter().next() else {
            return Err(internal_error("Storage returned an unexpected outcome"));
        };
        Ok(Json(MemoryResult {
            project_id: project.id,
            project_name: project.name,
            revision: result.revision,
            memory,
        }))
    }

    fn append_memory_note(
        &self,
        Parameters(params): Parameters<AppendMemoryNoteParams>,
    ) -> Result<Json<MemoryNoteResult>, ErrorData> {
        let (project, mut store) = self.open_project_storage(Some(params.project_name.as_str()))?;
        let result = store
            .commit(Mutation {
                operation_id: params.operation_id.clone(),
                changes: vec![Change::Memory(MemoryWrite::Append {
                    note: AppendMemoryNote {
                        note_id: params.note_id.clone(),
                        operation_id: params.operation_id.clone(),
                        run_id: params.run_id,
                        task_id: params.task_id,
                        body: params.body,
                    },
                })],
            })
            .map_err(storage_error)?;
        let Some(ChangeOutcome::Memory(memory)) = result.outcomes.into_iter().next() else {
            return Err(internal_error("Storage returned an unexpected outcome"));
        };
        let note = memory
            .notes
            .into_iter()
            .find(|note| note.note_id == params.note_id)
            .ok_or_else(|| internal_error("Stored memory note is missing"))?;
        Ok(Json(MemoryNoteResult {
            project_id: project.id,
            project_name: project.name,
            revision: result.revision,
            note,
        }))
    }

    fn publish_resource_intent(
        &self,
        Parameters(params): Parameters<PublishIntentParams>,
    ) -> Result<Json<ResourceIntentResult>, ErrorData> {
        let (project, mut store) = self.open_project_storage(Some(params.project_name.as_str()))?;
        let ttl = u32::try_from(params.ttl_seconds)
            .map_err(|_| tool_error("ttlSeconds must be between 1 and 86400".into()))?;
        if ttl == 0 {
            return Err(tool_error("ttlSeconds must be between 1 and 86400".into()));
        }
        let intents = store
            .publish_intents(&IntentUpdate {
                agent_run_id: params.agent_run_id.clone(),
                resources: vec![ResourceKey {
                    kind: params.resource_kind.clone(),
                    id: params.resource_id.clone(),
                }],
                ttl_seconds: ttl,
            })
            .map_err(storage_error)?;
        let intent = intents
            .into_iter()
            .find(|v| {
                v.agent_run_id == params.agent_run_id
                    && v.resource_kind == params.resource_kind
                    && v.resource_id == params.resource_id
            })
            .ok_or_else(|| internal_error("Published intent is missing"))?;
        Ok(Json(ResourceIntentResult {
            project_id: project.id,
            project_name: project.name,
            intent,
        }))
    }

    fn list_resource_intents(
        &self,
        Parameters(params): Parameters<ProjectParams>,
    ) -> Result<Json<ResourceIntentListResult>, ErrorData> {
        let (project, mut store) = self.open_project_storage(Some(params.project_name.as_str()))?;
        let db = store.snapshot().map_err(storage_error)?;
        let intents = db.live_intents().map_err(storage_error)?;
        Ok(Json(ResourceIntentListResult {
            project_id: project.id,
            project_name: project.name,
            intents,
        }))
    }

    fn design_get_overview(
        &self,
        Parameters(params): Parameters<DesignOverviewParams>,
    ) -> Result<Json<DesignOverviewResult>, ErrorData> {
        let (_project, mut store) =
            self.open_project_storage(Some(params.project_name.as_str()))?;
        let db = store.snapshot().map_err(storage_error)?;
        let overview = db
            .design_overview(params.max_depth)
            .map_err(storage_error)?;
        Ok(Json(overview))
    }

    fn design_get_scope(
        &self,
        Parameters(params): Parameters<DesignScopeParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let (_project, mut store) =
            self.open_project_storage(Some(params.project_name.as_str()))?;
        let db = store.snapshot().map_err(storage_error)?;
        let mut scope = db
            .design_scope(&ScopeQuery {
                element_id: params.element_id,
                include_ancestors: params.include_ancestors.unwrap_or(true),
                children_depth: params.children_depth,
            })
            .map_err(storage_error)?;
        if !params.include_source.unwrap_or(false) {
            scope.structurizr_dsl = None;
        }
        Ok(Json(
            crate::storage::documents::with_documents(db.as_ref(), scope).map_err(tool_error)?,
        ))
    }

    fn design_search(
        &self,
        Parameters(params): Parameters<DesignSearchParams>,
    ) -> Result<Json<DesignSearchResult>, ErrorData> {
        let (_project, mut store) =
            self.open_project_storage(Some(params.project_name.as_str()))?;
        let db = store.snapshot().map_err(storage_error)?;
        let result = db
            .design_search(&DesignSearchQuery {
                query: params.query,
                kinds: params.kinds.unwrap_or_default(),
                limit: params.limit.unwrap_or(20),
            })
            .map_err(storage_error)?;
        Ok(Json(result))
    }

    fn design_get_by_ids(
        &self,
        Parameters(params): Parameters<DesignByIdsParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let (_project, mut store) =
            self.open_project_storage(Some(params.project_name.as_str()))?;
        let db = store.snapshot().map_err(storage_error)?;
        let result = db.design_by_ids(&params.ids).map_err(storage_error)?;
        Ok(Json(
            crate::storage::documents::with_documents(db.as_ref(), result).map_err(tool_error)?,
        ))
    }

    fn design_get_bindings(
        &self,
        Parameters(params): Parameters<DesignBindingsParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let (_project, mut store) =
            self.open_project_storage(Some(params.project_name.as_str()))?;
        let db = store.snapshot().map_err(storage_error)?;
        let result = db
            .design_bindings(&BindingQuery {
                files: params.files.unwrap_or_default(),
                symbols: params.symbols.unwrap_or_default(),
            })
            .map_err(storage_error)?;
        Ok(Json(
            crate::storage::documents::with_documents(db.as_ref(), result).map_err(tool_error)?,
        ))
    }

    fn design_save(
        &self,
        Parameters(params): Parameters<DesignSaveParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let (project, mut store) =
            self.open_project_storage(Some(params.project_name.as_str()))?;
        let result = match store.commit(Mutation {
            operation_id: params.operation_id,
            changes: vec![Change::Design(DesignWrite::Save {
                change_intent: params.change_intent,
                changes: params.changes,
                read_tokens: params.read_tokens,
            })],
        }) {
            Ok(result) => result,
            Err(StorageError::DesignRejected(result)) => return Ok(Json(json!(result))),
            Err(error) => return Err(storage_error(error)),
        };
        let Some(ChangeOutcome::Design(saved)) = result.outcomes.into_iter().next() else {
            return Err(internal_error("Storage returned an unexpected outcome"));
        };
        let mut response = json!(saved);
        if saved.stored {
            // Canonical commit already succeeded. Publication errors are separate status.
            response["projection"] = markdown::refresh(self, &project, &mut store);
        }
        Ok(Json(response))
    }

    fn design_set_element_descriptions(
        &self,
        Parameters(params): Parameters<SetElementDescriptionsParams>,
    ) -> Result<Json<DesignSaveResult>, ErrorData> {
        let (_project, mut store) =
            self.open_project_storage(Some(params.project_name.as_str()))?;
        let result = match store.commit(Mutation {
            operation_id: params.operation_id,
            changes: vec![Change::Design(DesignWrite::Describe {
                updates: params.updates,
                read_tokens: params.read_tokens,
            })],
        }) {
            Ok(result) => result,
            Err(StorageError::DesignRejected(result)) => return Ok(Json(result)),
            Err(error) => return Err(storage_error(error)),
        };
        let Some(ChangeOutcome::Design(saved)) = result.outcomes.into_iter().next() else {
            return Err(internal_error("Storage returned an unexpected outcome"));
        };
        Ok(Json(saved))
    }

    /// Design-to-code correspondence. Reads the project's source files, so it reports what the
    /// model currently claims and whether the code still supports it.
    fn design_health_check(
        &self,
        Parameters(params): Parameters<ProjectParams>,
    ) -> Result<Json<design_health::DesignHealthResult>, ErrorData> {
        let (project, mut store) = self.open_project_storage(Some(params.project_name.as_str()))?;
        let db = store.snapshot().map_err(storage_error)?;
        design_health::scan_snapshot(db.as_ref(), std::path::Path::new(&project.folder))
            .map(Json)
            .map_err(tool_error)
    }

    fn mockup_list_pending_revisions(
        &self,
        Parameters(params): Parameters<ProjectParams>,
    ) -> Result<Json<MockupPendingResult>, ErrorData> {
        let (project, mut store) = self.open_project_storage(Some(params.project_name.as_str()))?;
        let db = store.snapshot().map_err(storage_error)?;
        let revision = db.metadata().revision;
        let mockups = db.mockups(true).map_err(storage_error)?;
        Ok(Json(MockupPendingResult {
            project_id: project.id,
            project_name: project.name,
            revision,
            mockups,
        }))
    }

    fn mockup_get_revision_context(
        &self,
        Parameters(params): Parameters<MockupContextParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let (project, mut store) = self.open_project_storage(Some(params.project_name.as_str()))?;
        let db = store.snapshot().map_err(storage_error)?;
        let revision = db.metadata().revision;
        let mockup = db
            .mockup(params.external_id.trim())
            .map_err(storage_error)?;
        let preview_variant = if mockup.working_svg.is_some() {
            "working"
        } else {
            "accepted"
        };
        let png = mockups::preview_base64_uncached(&mockup, preview_variant).map_err(tool_error)?;
        let structured = json!({
            "projectId": project.id,
            "projectName": project.name,
            "revision": revision,
            "previewVariant": preview_variant,
            "mockup": mockup,
        });
        let mut result = CallToolResult::structured(structured);
        result.content.push(ContentBlock::image(png, "image/png"));
        Ok(result)
    }

    #[tool(
        name = "adashi_help",
        description = "Read exact requirements, schema, examples and workflow for one tool operation before using it. Omit operation to list operations; changeTypes narrows design-save help. No project or writes required.",
        annotations(read_only_hint = true, destructive_hint = false)
    )]
    fn help(
        &self,
        Parameters(params): Parameters<help::HelpParams>,
    ) -> Result<CallToolResult, ErrorData> {
        help::get(params)
    }

    #[tool(
        name = "adashi_design",
        description = "Read and edit formal C4/UML, Markdown designs, bindings and UI mockups. Use adashi_help with tool and operation for exact requirements and examples before an unfamiliar call.",
        annotations(read_only_hint = true, destructive_hint = false)
    )]
    fn design(
        &self,
        Parameters(params): Parameters<DesignParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match params.operation {
            DesignOperation::ListMarkdown => {
                let (_, mut store) = self.open_project_storage(Some(&params.project_name))?;
                let snapshot = store.snapshot().map_err(storage_error)?;
                let page = snapshot.markdown_documents(&params.markdown_query.unwrap_or_default()).map_err(storage_error)?;
                Ok(CallToolResult::structured(json!({"markdown":page,"guidance":"Metadata only. Continue with markdownQuery.afterId=nextAfterId; retrieve markdown:<externalId> using get_documents before editing."})))
            }
            DesignOperation::Save => {
                let operation_id = required(params.operation_id, "operationId")?;
                let change_intent = required(params.change_intent, "changeIntent")?;
                let changes = required(params.changes, "changes")?;
                self.design_save(Parameters(DesignSaveParams {
                    project_name: params.project_name,
                    operation_id,
                    read_tokens: params.read_tokens.unwrap_or_default(),
                    change_intent,
                    changes,
                }))?
                .into_call_tool_result()
            }
            DesignOperation::GetScope => {
                let element_id = required(params.element_id, "elementId")?;
                self.design_get_scope(Parameters(DesignScopeParams {
                    project_name: params.project_name,
                    element_id,
                    include_ancestors: params.include_ancestors,
                    children_depth: params.children_depth,
                    include_source: params.include_source,
                }))?
                .into_call_tool_result()
            }
            DesignOperation::GetByIds => {
                let ids = required(params.ids, "ids")?;
                self.design_get_by_ids(Parameters(DesignByIdsParams {
                    project_name: params.project_name,
                    ids,
                }))?
                .into_call_tool_result()
            }
            DesignOperation::GetDocuments => {
                let ids = required(params.ids, "ids")?;
                let (_project, mut store) =
                    self.open_project_storage(Some(params.project_name.as_str()))?;
                let snapshot = store.snapshot().map_err(storage_error)?;
                let documents = snapshot.design_documents(&ids).map_err(storage_error)?;
                Ok(CallToolResult::structured(json!({"documents":documents})))
            }
            DesignOperation::Search => {
                let query = required(params.query, "query")?;
                self.design_search(Parameters(DesignSearchParams {
                    project_name: params.project_name,
                    query,
                    kinds: params.kinds,
                    limit: params.limit,
                }))?
                .into_call_tool_result()
            }
            DesignOperation::GetOverview => self
                .design_get_overview(Parameters(DesignOverviewParams {
                    project_name: params.project_name,
                    max_depth: params.max_depth,
                }))?
                .into_call_tool_result(),
            DesignOperation::GetBindings => self
                .design_get_bindings(Parameters(DesignBindingsParams {
                    project_name: params.project_name,
                    files: params.files,
                    symbols: params.symbols,
                }))?
                .into_call_tool_result(),
            DesignOperation::SetElementDescriptions => {
                let operation_id = required(params.operation_id, "operationId")?;
                let updates = required(params.updates, "updates")?;
                self.design_set_element_descriptions(Parameters(SetElementDescriptionsParams {
                    project_name: params.project_name,
                    operation_id,
                    read_tokens: required(params.read_tokens, "readTokens")?,
                    updates,
                }))?
                .into_call_tool_result()
            }
            DesignOperation::Health => self
                .design_health_check(Parameters(ProjectParams {
                    project_name: params.project_name,
                }))?
                .into_call_tool_result(),
            DesignOperation::MockupListPendingRevisions => self
                .mockup_list_pending_revisions(Parameters(ProjectParams {
                    project_name: params.project_name,
                }))?
                .into_call_tool_result(),
            DesignOperation::MockupGetRevisionContext => {
                let external_id = required(params.external_id, "externalId")?;
                self.mockup_get_revision_context(Parameters(MockupContextParams {
                    project_name: params.project_name,
                    external_id,
                }))?
                .into_call_tool_result()
            }
        }
    }

    #[tool(
        name = "adashi_grep",
        description = "Search design, tasks and memory using bounded grep-style results with drillable locators. Use adashi_help with tool=adashi_grep for pattern syntax.",
        annotations(read_only_hint = true, destructive_hint = false)
    )]
    fn grep(
        &self,
        Parameters(params): Parameters<GrepParams>,
    ) -> Result<Json<grep::GrepResult>, ErrorData> {
        let (_project, mut store) =
            self.open_project_storage(Some(params.project_name.as_str()))?;
        let db = store.snapshot().map_err(storage_error)?;
        db.search(&params).map(Json).map_err(storage_error)
    }

    #[tool(
        name = "adashi_tasks",
        description = "Create, track, finish and close project tasks. Use adashi_help with tool and operation for exact requirements, transitions and examples before an unfamiliar call.",
        annotations(read_only_hint = true, destructive_hint = false)
    )]
    fn tasks(
        &self,
        Parameters(params): Parameters<TasksParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match params.operation {
            TasksOperation::Create => {
                let operation_id = required(params.operation_id, "operationId")?;
                let title = required(params.title, "title")?;
                self.create_task(Parameters(CreateTaskParams {
                    project_name: params.project_name,
                    operation_id,
                    title,
                    description: params.description,
                    design_specification_links: params.design_specification_links,
                }))?
                .into_call_tool_result()
            }
            TasksOperation::List => self
                .list_tasks(Parameters(ListTasksParams {
                    project_name: params.project_name,
                    states: params.states,
                    limit: params.limit,
                    cursor: params.cursor,
                }))?
                .into_call_tool_result(),
            TasksOperation::Update => {
                let operation_id = required(params.operation_id, "operationId")?;
                let expected_version = required(params.expected_version, "expectedVersion")?;
                let task_id = required(params.task_id, "taskId")?;
                self.update_task(Parameters(UpdateTaskParams {
                    project_name: params.project_name,
                    operation_id,
                    expected_version,
                    task_id,
                    title: params.title,
                    description: params.description,
                    state: params.state,
                    design_specification_links: params.design_specification_links,
                }))?
                .into_call_tool_result()
            }
            TasksOperation::Finish => {
                let operation_id = required(params.operation_id, "operationId")?;
                let expected_version = required(params.expected_version, "expectedVersion")?;
                let task_id = required(params.task_id, "taskId")?;
                let completion_memo = required(params.completion_memo, "completionMemo")?;
                self.finish_task(Parameters(FinishTaskParams {
                    project_name: params.project_name,
                    operation_id,
                    expected_version,
                    task_id,
                    completion_memo,
                    created_files: params.created_files,
                    changed_files: params.changed_files,
                }))?
                .into_call_tool_result()
            }
            TasksOperation::Close => {
                let operation_id = required(params.operation_id, "operationId")?;
                let expected_version = required(params.expected_version, "expectedVersion")?;
                let task_id = required(params.task_id, "taskId")?;
                self.close_task(Parameters(CloseTaskParams {
                    project_name: params.project_name,
                    operation_id,
                    expected_version,
                    task_id,
                }))?
                .into_call_tool_result()
            }
            TasksOperation::Delete => {
                let operation_id = required(params.operation_id, "operationId")?;
                let expected_version = required(params.expected_version, "expectedVersion")?;
                let task_id = required(params.task_id, "taskId")?;
                self.delete_task(Parameters(DeleteTaskParams {
                    project_name: params.project_name,
                    task_id,
                    operation_id,
                    expected_version,
                }))?
                .into_call_tool_result()
            }
            TasksOperation::Get => {
                let task_id = required(params.task_id, "taskId")?;
                self.get_task(Parameters(TaskIdParams {
                    project_name: params.project_name,
                    task_id,
                    include_design_scopes: params.include_design_scopes,
                }))?
                .into_call_tool_result()
            }
        }
    }

    #[tool(
        name = "adashi_qa",
        description = "Define QA jobs and execute selected jobs. Use adashi_help with tool and operation for exact requirements and examples before an unfamiliar call.",
        annotations(read_only_hint = false, destructive_hint = true)
    )]
    fn qa(&self, Parameters(params): Parameters<QaParams>) -> Result<CallToolResult, ErrorData> {
        match params.operation {
            QaOperation::ListJobs => self
                .list_qa_jobs(Parameters(ListQaJobsParams {
                    project_name: params.project_name,
                    query: params.query,
                    limit: params.limit,
                    cursor: params.cursor,
                }))?
                .into_call_tool_result(),
            QaOperation::GetJob => {
                let qa_job_id = required(params.qa_job_id, "qaJobId")?;
                self.get_qa_job(Parameters(QaJobIdParams {
                    project_name: params.project_name,
                    qa_job_id,
                }))?
                .into_call_tool_result()
            }
            QaOperation::CreateJob => {
                let operation_id = required(params.operation_id, "operationId")?;
                let name = required(params.name, "name")?;
                let command = required(params.command, "command")?;
                self.create_qa_job(Parameters(CreateQaJobParams {
                    project_name: params.project_name,
                    operation_id,
                    name,
                    description: params.description,
                    command,
                    working_directory: params.working_directory,
                    shell: params.shell,
                    timeout_seconds: params.timeout_seconds,
                    enabled: params.enabled,
                    design_specification_links: params.design_specification_links,
                    task_ids: params.task_ids,
                    tags: params.tags,
                }))?
                .into_call_tool_result()
            }
            QaOperation::UpdateJob => {
                let operation_id = required(params.operation_id, "operationId")?;
                let expected_version = required(params.expected_version, "expectedVersion")?;
                let qa_job_id = required(params.qa_job_id, "qaJobId")?;
                self.update_qa_job(Parameters(UpdateQaJobParams {
                    project_name: params.project_name,
                    operation_id,
                    expected_version,
                    qa_job_id,
                    name: params.name,
                    description: params.description,
                    command: params.command,
                    working_directory: params.working_directory,
                    shell: params.shell,
                    timeout_seconds: params.timeout_seconds,
                    enabled: params.enabled,
                    design_specification_links: params.design_specification_links,
                    task_ids: params.task_ids,
                    tags: params.tags,
                }))?
                .into_call_tool_result()
            }
            QaOperation::DeleteJob => {
                let operation_id = required(params.operation_id, "operationId")?;
                let expected_version = required(params.expected_version, "expectedVersion")?;
                let qa_job_id = required(params.qa_job_id, "qaJobId")?;
                self.delete_qa_job(Parameters(DeleteQaJobParams {
                    project_name: params.project_name,
                    qa_job_id,
                    operation_id,
                    expected_version,
                }))?
                .into_call_tool_result()
            }
            QaOperation::RunJobs => {
                let operation_id = required(params.operation_id, "operationId")?;
                let query = required(params.query, "query")?;
                self.run_qa_jobs(Parameters(RunQaJobsParams {
                    project_name: params.project_name,
                    operation_id,
                    query,
                    trigger_source: params.trigger_source,
                }))?
                .into_call_tool_result()
            }
            QaOperation::ListRuns => self
                .list_qa_runs(Parameters(ListQaRunsParams {
                    project_name: params.project_name,
                    limit: params.limit,
                }))?
                .into_call_tool_result(),
            QaOperation::GetRun => {
                let qa_run_id = required(params.qa_run_id, "qaRunId")?;
                self.get_qa_run(Parameters(QaRunIdParams {
                    project_name: params.project_name,
                    qa_run_id,
                }))?
                .into_call_tool_result()
            }
        }
    }

    #[tool(
        name = "adashi_memory",
        description = "Read project memory and record durable handovers. Use adashi_help with tool and operation for exact requirements, retention and examples before an unfamiliar call.",
        annotations(read_only_hint = true, destructive_hint = false)
    )]
    fn memory(
        &self,
        Parameters(params): Parameters<MemoryParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match params.operation {
            MemoryOperation::Get => self
                .get_memory(Parameters(GetMemoryParams {
                    project_name: params.project_name,
                    query: params.query,
                    note_id: params.note_id,
                    run_id: params.run_id,
                    task_id: params.task_id,
                    include_superseded: params.include_superseded,
                }))?
                .into_call_tool_result(),
            MemoryOperation::Append => {
                let note_id = required_with_help(
                    params.note_id,
                    "noteId",
                    "append",
                    "pass a stable note id such as handover-<topic>-<yyyymmdd>",
                )?;
                let operation_id = required_with_help(
                    params.operation_id,
                    "operationId",
                    "append",
                    "pass a unique idempotency key for this append, e.g. <noteId>-a",
                )?;
                let body = required_with_help(
                    params.body,
                    "body",
                    "append",
                    "pass the handover text (at most 1000 characters)",
                )?;
                // The published schema cannot require runId for one operation of many, and a
                // client that treats optional fields as nullable sends an explicit null. The note
                // is still a complete, idempotent handover, so provenance falls back to the
                // operationId — which is unique per append — instead of rejecting the note.
                let run_id = match params.run_id.map(|value| value.trim().to_string()) {
                    Some(value) if !value.is_empty() => value,
                    _ => operation_id.clone(),
                };
                self.append_memory_note(Parameters(AppendMemoryNoteParams {
                    project_name: params.project_name,
                    note_id,
                    operation_id,
                    run_id,
                    task_id: params.task_id,
                    body,
                }))?
                .into_call_tool_result()
            }
            MemoryOperation::Update => {
                let operation_id = required(params.operation_id, "operationId")?;
                let expected_version = required(params.expected_version, "expectedVersion")?;
                let memory = required(params.memory, "memory")?;
                self.update_memory(Parameters(UpdateMemoryParams {
                    project_name: params.project_name,
                    operation_id,
                    expected_version,
                    memory,
                    superseded_note_ids: params.superseded_note_ids,
                }))?
                .into_call_tool_result()
            }
            MemoryOperation::UpdateRule => {
                let operation_id = required(params.operation_id, "operationId")?;
                let expected_version = required(params.expected_version, "expectedVersion")?;
                let rule = required(params.rule, "rule")?;
                self.update_memory_rule(Parameters(UpdateMemoryRuleParams {
                    project_name: params.project_name,
                    operation_id,
                    expected_version,
                    rule,
                }))?
                .into_call_tool_result()
            }
        }
    }

    #[tool(
        name = "adashi_rules",
        description = "Retrieve lifecycle injections and manage project-specific rules. Use adashi_help with tool and operation for exact requirements and examples before an unfamiliar call.",
        annotations(read_only_hint = true, destructive_hint = false)
    )]
    fn rules(
        &self,
        Parameters(params): Parameters<RulesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match params.operation {
            RulesOperation::List => self
                .list_rules(Parameters(ProjectParams {
                    project_name: params.project_name,
                }))?
                .into_call_tool_result(),
            RulesOperation::Create => {
                let operation_id = required(params.operation_id, "operationId")?;
                let name = required(params.name, "name")?;
                let enabled = required(params.enabled, "enabled")?;
                let intend = required(params.intend, "intend")?;
                let hook = required(params.hook, "hook")?;
                let prompt = required(params.prompt, "prompt")?;
                self.create_rule(Parameters(CreateRuleParams {
                    project_name: params.project_name,
                    operation_id,
                    name,
                    enabled,
                    intend,
                    hook,
                    prompt,
                }))?
                .into_call_tool_result()
            }
            RulesOperation::Update => {
                let operation_id = required(params.operation_id, "operationId")?;
                let expected_version = required(params.expected_version, "expectedVersion")?;
                let rule_id = required(params.rule_id, "ruleId")?;
                let name = required(params.name, "name")?;
                let enabled = required(params.enabled, "enabled")?;
                let intend = required(params.intend, "intend")?;
                let hook = required(params.hook, "hook")?;
                let prompt = required(params.prompt, "prompt")?;
                self.update_rule(Parameters(UpdateRuleParams {
                    project_name: params.project_name,
                    operation_id,
                    expected_version,
                    rule_id,
                    name,
                    enabled,
                    intend,
                    hook,
                    prompt,
                }))?
                .into_call_tool_result()
            }
            RulesOperation::Delete => {
                let operation_id = required(params.operation_id, "operationId")?;
                let expected_version = required(params.expected_version, "expectedVersion")?;
                let rule_id = required(params.rule_id, "ruleId")?;
                self.delete_rule(Parameters(DeleteRuleParams {
                    project_name: params.project_name,
                    operation_id,
                    expected_version,
                    rule_id,
                }))?
                .into_call_tool_result()
            }
            RulesOperation::GetRuleInjections => {
                let intend = required(params.intend, "intend")?;
                let hook = required(params.hook, "hook")?;
                self.get_rule_injections(Parameters(RuleInjectionParams {
                    project_name: params.project_name,
                    intend,
                    hook,
                    memory_context: params.memory_context,
                }))?
                .into_call_tool_result()
            }
        }
    }

    #[tool(
        name = "adashi_intents",
        description = "Publish or list expiring advisory resource intents. These are not locks. Use adashi_help with tool and operation for exact requirements and examples.",
        annotations(read_only_hint = true, destructive_hint = false)
    )]
    fn intents(
        &self,
        Parameters(params): Parameters<IntentsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match params.operation {
            IntentsOperation::Publish => {
                let agent_run_id = required(params.agent_run_id, "agentRunId")?;
                let resource_kind = required(params.resource_kind, "resourceKind")?;
                let resource_id = required(params.resource_id, "resourceId")?;
                let ttl_seconds = required(params.ttl_seconds, "ttlSeconds")?;
                self.publish_resource_intent(Parameters(PublishIntentParams {
                    project_name: params.project_name,
                    agent_run_id,
                    resource_kind,
                    resource_id,
                    ttl_seconds,
                }))?
                .into_call_tool_result()
            }
            IntentsOperation::List => self
                .list_resource_intents(Parameters(ProjectParams {
                    project_name: params.project_name,
                }))?
                .into_call_tool_result(),
        }
    }
}

#[rmcp::tool_handler]
impl rmcp::ServerHandler for AdashiMcpServer {
    async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParams,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let router = Self::tool_router();
        // Unknown tools remain protocol errors. Errors from a known tool, including rmcp's
        // parameter decoder, must reach the caller as terminal tool results with repair help.
        let Some(tool) = router.get(&request.name).cloned() else {
            return Err(ErrorData::invalid_params(
                format!("Unknown Adashi tool '{}'", request.name),
                None,
            ));
        };
        let arguments = request.arguments.clone().unwrap_or_default();
        let call = rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
        Ok(errors::complete(
            self,
            &tool,
            &arguments,
            router.call(call).await,
        ))
    }
}

pub fn run_stdio_server() -> Result<(), Box<dyn std::error::Error>> {
    let settings_path = settings::settings_path();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    runtime.block_on(async move {
        let service = serve_server(AdashiMcpServer::new(settings_path), stdio()).await?;
        service.waiting().await?;
        Ok::<(), Box<dyn std::error::Error>>(())
    })
}

#[cfg(test)]
fn project_row_id(db: &rusqlite::Connection) -> Result<i64, String> {
    db.query_row("SELECT id FROM projects ORDER BY id LIMIT 1", [], |row| {
        row.get(0)
    })
    .map_err(|err| err.to_string())
}

/// One branch per linked design specification.
///
/// `include_scopes` is off by default: a task that carries many links would otherwise inline
/// every linked branch at once, which is both a large payload and a poor answer, since the work
/// usually needs one or two of them. With scopes off the branches still name what is linked, and
/// the caller retrieves the detail it needs with the design get_scope operation or by asking for
/// the scopes explicitly.
fn task_link_branches(task: &Task) -> Vec<TaskDesignSpecificationBranch> {
    task.design_specification_links
        .iter()
        .map(|link| TaskDesignSpecificationBranch {
            documents: Vec::new(),
            link: link.clone(),
            scope: None,
            mockup: None,
            mockup_preview: None,
            note: None,
        })
        .collect()
}

fn load_task_design_specifications(
    db: &dyn ReadSnapshot,
    task: &Task,
    include_scopes: bool,
) -> Result<Vec<TaskDesignSpecificationBranch>, String> {
    if !include_scopes {
        return Ok(task_link_branches(task));
    }
    task.design_specification_links
        .iter()
        .map(|link| {
            let mockup = if link.target_type == "mockup" {
                Some(
                    db.mockup(&link.design_external_id)
                        .map_err(|e| e.to_string())?,
                )
            } else {
                None
            };
            let root = match link.target_type.as_str() {
                "element" | "markdown" => Some(link.design_external_id.clone()),
                "uml" => db
                    .design_by_ids(&[link.design_external_id.clone()])
                    .map_err(|e| e.to_string())?
                    .diagrams
                    .into_iter()
                    .next()
                    .and_then(|v| v.attached_to_external_id),
                "mockup" => mockup
                    .as_ref()
                    .map(|m| m.manifest.attached_to_external_id.clone()),
                _ => None,
            };
            let scope = if let Some(element_id) = root {
                let mut scope = db
                    .design_scope(&ScopeQuery {
                        element_id,
                        include_ancestors: true,
                        children_depth: Some(2),
                    })
                    .map_err(|e| e.to_string())?;
                scope.structurizr_dsl = None;
                Some(scope)
            } else {
                None
            };
            let note = if scope.is_none() {
                Some("This link does not resolve to an element-rooted design branch.".into())
            } else {
                None
            };
            let documents = if let Some(scope) = &scope {
                db.design_documents(&scope.markdown.documents.iter().map(|d|format!("markdown:{}",d.external_id)).collect::<Vec<_>>()).map_err(|e|e.to_string())?
            } else { Vec::new() };
            Ok(TaskDesignSpecificationBranch {
                documents,
                link: link.clone(),
                scope,
                mockup,
                mockup_preview: None,
                note,
            })
        })
        .collect()
}

fn missing_field(field: &str) -> ErrorData {
    ErrorData::invalid_params(
        format!("missing required field '{field}' for this operation"),
        None,
    )
}

/// An omitted argument must not be a dead end. A caller that cannot see the published schema
/// (or whose client dropped an argument) only learns which value to supply from the error
/// itself, so the field is named together with the operation that needs it and what it means.
fn missing_field_help(field: &str, operation: &str, help: &str) -> ErrorData {
    ErrorData::invalid_params(
        format!("missing required field '{field}' for operation '{operation}': {help}"),
        None,
    )
}

fn required<T>(value: Option<T>, field: &str) -> Result<T, ErrorData> {
    value.ok_or_else(|| missing_field(field))
}

fn required_with_help<T>(
    value: Option<T>,
    field: &str,
    operation: &str,
    help: &str,
) -> Result<T, ErrorData> {
    value.ok_or_else(|| missing_field_help(field, operation, help))
}

/// A domain failure. The specific reason travels in the top-level `message` because an MCP
/// client relays only that to the model — a reason that exists solely in `data` reads to the
/// caller as an opaque failure it cannot act on. Typed payloads such as resource conflicts keep
/// their exact JSON shape in `data` and keep the generic top-level text.
fn storage_error(error: StorageError) -> ErrorData {
    match error {
        StorageError::Backend(_) => internal_error(error),
        StorageError::Conflict(conflicts) => {
            tool_error(json!({"code": "resource.conflict", "conflicts": conflicts}).to_string())
        }
        StorageError::Documents(conflicts) => {
            let stale = conflicts
                .iter()
                .any(|c| c.code == crate::storage::api::DocumentConflictCode::OutOfDate);
            let mut value = json!({"code":if stale {"out_of_date"} else {"read_required"},"stored":false,
                "message":if stale {"The design document changed since you read it. Nothing was saved."} else {"Read every existing document before changing or deleting it. Nothing was saved."},
                "request":"Merge intended edits into each returned currentDocument and retry with its readToken and a new operationId. Do not only replace the token on an old payload."});
            if conflicts.len() == 1 {
                let item = &conflicts[0];
                value["documentId"] = json!(item.document_id);
                value["currentDocument"] = item.current_document.clone();
                value["readToken"] = json!(item.read_token);
            } else {
                value["conflicts"] = json!(conflicts);
            }
            tool_error(value.to_string())
        }
        _ => tool_error(error.to_string()),
    }
}

fn tool_error(message: String) -> ErrorData {
    match serde_json::from_str::<serde_json::Value>(&message) {
        Ok(value) => ErrorData::invalid_params("Adashi MCP request failed", Some(value)),
        Err(_) => ErrorData::invalid_params(message, None),
    }
}

fn internal_error(err: impl std::fmt::Display) -> ErrorData {
    let value = json!({ "message": err.to_string() });
    ErrorData::internal_error("Adashi MCP server failed", Some(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mockups::CreateMockupInput;
    use crate::settings::{AppSettings, ProjectSettings, WindowSettings};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn mcp_resolves_project_backend_without_rewriting_settings_or_falling_back() {
        let root = tempfile::tempdir().unwrap();
        let project = ProjectSettings {
            id: "storage-selection".into(),
            name: "Storage selection".into(),
            folder: root.path().join("project").to_string_lossy().into_owned(),
        };
        let settings_path = root.path().join("settings.json");
        settings::save(
            &settings_path,
            &AppSettings {
                window: WindowSettings::default(),
                projects: vec![project.clone()],
                last_active_project_id: Some(project.id.clone()),
                rule_templates: vec![],
                architecture_projection: Default::default(),
            },
        )
        .unwrap();
        let before = std::fs::read(&settings_path).unwrap();
        let server = AdashiMcpServer::new(settings_path.clone());
        let (_, mut store) = server.open_project_storage(Some(&project.id)).unwrap();
        assert_eq!(store.rules_snapshot().unwrap().project.id, project.id);
        drop(store);
        let db_path = settings::project_database_path(&project);
        let db_before = std::fs::read(&db_path).unwrap();
        std::fs::write(
            settings::project_data_dir(&project).join("storage.json"),
            r#"{"schemaVersion":1,"backend":{"kind":"text"}}"#,
        )
        .unwrap();
        let error = server
            .open_project_storage(Some(&project.id))
            .err()
            .unwrap();
        assert!(error.message.contains("storage.validation"), "{error}");
        assert_eq!(std::fs::read(&db_path).unwrap(), db_before);
        assert_eq!(std::fs::read(&settings_path).unwrap(), before);
    }

    fn resolve_ref<'a>(schema: &'a serde_json::Value, reference: &str) -> &'a serde_json::Value {
        let path = reference.strip_prefix("#/").unwrap_or(reference);
        let mut current = schema;
        for segment in path.split('/') {
            current = current
                .get(segment)
                .unwrap_or_else(|| panic!("missing {segment} in schema"));
        }
        current
    }

    #[test]
    fn capability_operation_enums_are_self_describing() {
        let cases = [
            (
                serde_json::to_value(rmcp::schemars::schema_for!(DesignParams)).unwrap(),
                vec![
                    "list_markdown",
                    "save",
                    "get_scope",
                    "get_by_ids",
                    "get_documents",
                    "search",
                    "get_overview",
                    "get_bindings",
                    "set_element_descriptions",
                    "health",
                    "mockup_list_pending_revisions",
                    "mockup_get_revision_context",
                ],
            ),
            (
                serde_json::to_value(rmcp::schemars::schema_for!(TasksParams)).unwrap(),
                vec![
                    "create", "list", "update", "finish", "close", "delete", "get",
                ],
            ),
            (
                serde_json::to_value(rmcp::schemars::schema_for!(QaParams)).unwrap(),
                vec![
                    "create_job",
                    "update_job",
                    "delete_job",
                    "list_jobs",
                    "run_jobs",
                    "list_runs",
                    "get_job",
                    "get_run",
                ],
            ),
            (
                serde_json::to_value(rmcp::schemars::schema_for!(MemoryParams)).unwrap(),
                vec!["get", "append", "update", "update_rule"],
            ),
            (
                serde_json::to_value(rmcp::schemars::schema_for!(RulesParams)).unwrap(),
                vec!["list", "create", "update", "delete", "get_rule_injections"],
            ),
            (
                serde_json::to_value(rmcp::schemars::schema_for!(IntentsParams)).unwrap(),
                vec!["publish", "list"],
            ),
        ];

        for (schema, expected) in cases {
            let operation = &schema["properties"]["operation"];
            let resolved = match operation.get("$ref").and_then(|value| value.as_str()) {
                Some(reference) => resolve_ref(&schema, reference),
                None => operation,
            };
            assert_eq!(resolved.get("type"), Some(&json!("string")));
            let enum_values = resolved["enum"]
                .as_array()
                .expect("operation must be a string enum");
            let values: Vec<&str> = enum_values
                .iter()
                .map(|value| value.as_str().unwrap())
                .collect();
            assert_eq!(values, expected);
        }
    }

    /// The prompt-hygiene registry must match the surface the router actually exposes, so a
    /// renamed tool cannot silently leave stored prompts pointing at a dead name.
    #[test]
    fn tool_name_registry_matches_the_router() {
        let mut advertised = AdashiMcpServer::tool_router()
            .list_all()
            .into_iter()
            .map(|tool| tool.name.to_string())
            .collect::<Vec<_>>();
        advertised.sort();

        let mut registered = crate::prompt_hygiene::MCP_TOOL_NAMES
            .iter()
            .map(|name| (*name).to_string())
            .collect::<Vec<_>>();
        registered.sort();

        assert_eq!(advertised, registered);
    }

    #[test]
    fn compact_descriptions_make_operation_help_discoverable() {
        for tool in AdashiMcpServer::tool_router().list_all() {
            let description = tool.description.as_deref().unwrap();
            assert!(description.len() <= 240, "{}", tool.name);
            if tool.name != "adashi_help" {
                assert!(description.contains("adashi_help"));
            }
        }
    }

    /// An omitted argument has to be self-correcting: the caller learns the value it must
    /// supply from the error, not from a schema it never received.
    #[test]
    fn missing_append_field_names_the_value_to_supply() {
        let server = AdashiMcpServer::new(std::path::PathBuf::from("unused-settings.json"));
        let params = |project_name: &str, run_id: Option<String>, body: Option<String>| {
            Parameters(MemoryParams {
                operation: MemoryOperation::Append,
                project_name: project_name.into(),
                note_id: Some("note".into()),
                operation_id: Some("operation".into()),
                body,
                run_id,
                task_id: None,
                query: None,
                include_superseded: false,
                expected_version: None,
                memory: None,
                superseded_note_ids: Vec::new(),
                rule: None,
            })
        };
        let error = server
            .memory(params(
                "missing-project-fields",
                Some("run.example".into()),
                None,
            ))
            .expect_err("append without a body must fail");
        let message = error.message.to_string();
        assert!(
            message.contains("'body'"),
            "error must name the field: {message}"
        );
        assert!(
            message.contains("'append'"),
            "error must name the operation: {message}"
        );
        assert!(
            message.contains("1000 characters"),
            "error must state the real limit: {message}"
        );

        // A client that treats an optional field as nullable sends an explicit null. The append
        // must stay usable, so runId falls back to the operationId rather than failing the call.
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("adashi-mcp-append-run-id-{suffix}"));
        let settings_path = root.join("settings.json");
        let project_folder = root.join("project");
        let project = ProjectSettings {
            id: "append-run-id-test".into(),
            name: "Append Run Id Test".into(),
            folder: project_folder.to_string_lossy().into_owned(),
        };
        settings::save(
            &settings_path,
            &AppSettings {
                window: WindowSettings {
                    width: 1000,
                    height: 700,
                    x: None,
                    y: None,
                },
                projects: vec![project.clone()],
                last_active_project_id: Some(project.id.clone()),
                rule_templates: vec![],
                architecture_projection: Default::default(),
            },
        )
        .unwrap();
        let server = AdashiMcpServer::new(settings_path);
        for run_id in [None, Some("   ".to_string())] {
            server
                .memory(params(
                    "append-run-id-test",
                    run_id,
                    Some("handover body".into()),
                ))
                .unwrap_or_else(|error| {
                    panic!("append must survive a null runId: {}", error.message)
                });
        }
        let (_project, mut db) = server
            .open_project_storage(Some("append-run-id-test"))
            .unwrap();
        let notes = db.snapshot().unwrap().retained_memory_notes().unwrap();
        let note = notes
            .iter()
            .find(|note| note.note_id == "note")
            .expect("the appended note must be retained");
        assert_eq!(note.run_id, "operation");
        drop(db);
        let _ = std::fs::remove_dir_all(root);
    }

    /// A client relays only the top-level error text, so a plain domain reason must travel there
    /// instead of hiding in `data`, while a typed conflict payload keeps its exact JSON shape.
    #[test]
    fn domain_failures_state_the_reason_in_the_relayed_message() {
        let plain = tool_error("tasks.invalid_cursor: keep the original project and states".into());
        assert_eq!(
            plain.message.to_string(),
            "tasks.invalid_cursor: keep the original project and states"
        );
        assert!(plain.data.is_none());

        let conflict = tool_error(
            json!({
                "code": "resource.conflict",
                "conflicts": [{"resourceKind": "design.element", "resourceId": "a", "expectedVersion": 1, "currentVersion": 2}]
            })
            .to_string(),
        );
        assert_eq!(conflict.message.to_string(), "Adashi MCP request failed");
        assert_eq!(
            conflict.data.as_ref().unwrap()["code"],
            json!("resource.conflict")
        );
    }

    #[test]
    fn qa_listings_stay_bounded_and_never_inline_console_output() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("adashi-mcp-qa-listing-{suffix}"));
        let settings_path = root.join("settings.json");
        let project_folder = root.join("project");
        let project = ProjectSettings {
            id: "qa-listing-test".into(),
            name: "QA Listing Test".into(),
            folder: project_folder.to_string_lossy().into_owned(),
        };
        settings::save(
            &settings_path,
            &AppSettings {
                window: WindowSettings {
                    width: 1000,
                    height: 700,
                    x: None,
                    y: None,
                },
                projects: vec![project.clone()],
                last_active_project_id: Some(project.id.clone()),
                rule_templates: vec![],
                architecture_projection: Default::default(),
            },
        )
        .unwrap();
        let db = crate::open_project_database(&project).unwrap();
        let project_row_id = project_row_id(&db).unwrap();
        let job = qa::create_job(
            &db,
            project_row_id,
            NewQaJob {
                name: "Verbose suite".into(),
                description: Some("prints a lot".into()),
                command: "run-tests".into(),
                working_directory: None,
                shell: None,
                timeout_seconds: None,
                enabled: Some(true),
                created_by: None,
                design_specification_links: None,
                task_ids: None,
                tags: Some(vec!["suite".into()]),
            },
        )
        .unwrap();
        // Stored evidence far larger than any bounded listing may return.
        let console_output = "console line\n".repeat(20_000);
        db.execute(
            "INSERT INTO qa_runs(
                project_id, trigger_source, query_snapshot, status, finished_at, summary
             ) VALUES(?1, 'mcp', '{}', 'failed', '2999-01-01 00:00:00', '0 passed, 1 failed')",
            rusqlite::params![project_row_id],
        )
        .unwrap();
        let qa_run_id = db.last_insert_rowid();
        db.execute(
            "INSERT INTO qa_job_runs(
                qa_run_id, qa_job_id, command_snapshot, status, exit_code,
                finished_at, duration_ms, output
             ) VALUES(?1, ?2, '{\"command\":\"run-tests\"}', 'failed', 1,
                '2999-01-01 00:00:00', 42, ?3)",
            rusqlite::params![qa_run_id, job.id, console_output],
        )
        .unwrap();
        drop(db);

        let server = AdashiMcpServer::new(settings_path);

        let jobs = serde_json::to_value(
            server
                .list_qa_jobs(Parameters(ListQaJobsParams {
                    project_name: project.id.clone(),
                    query: None,
                    limit: None,
                    cursor: None,
                }))
                .unwrap()
                .0,
        )
        .unwrap();
        let jobs_bytes = serde_json::to_string(&jobs).unwrap();
        assert!(
            !jobs_bytes.contains("console line"),
            "job listing leaked console output"
        );
        assert_eq!(jobs["filteredTotal"], json!(1));
        assert_eq!(jobs["contractVersion"], json!(1));
        assert_eq!(jobs["jobs"].as_array().unwrap().len(), 1);
        assert_eq!(jobs["jobs"][0]["derivedState"], json!("red"));
        assert_eq!(jobs["jobs"][0]["latestRun"]["status"], json!("failed"));
        assert_eq!(jobs["jobs"][0]["latestRun"]["durationMs"], json!(42));
        assert!(jobs["jobs"][0].get("runHistory").is_none());
        assert!(jobs["jobs"][0].get("command").is_none());
        assert!(jobs["jobs"][0]
            .get("latestRun")
            .unwrap()
            .get("output")
            .is_none());
        assert!(
            jobs_bytes.len() < 2_000,
            "bounded job listing must stay small, was {} bytes",
            jobs_bytes.len()
        );

        let runs = serde_json::to_value(
            server
                .list_qa_runs(Parameters(ListQaRunsParams {
                    project_name: project.id.clone(),
                    limit: None,
                }))
                .unwrap()
                .0,
        )
        .unwrap();
        let runs_bytes = serde_json::to_string(&runs).unwrap();
        assert!(
            !runs_bytes.contains("console line"),
            "run listing leaked console output"
        );
        assert_eq!(runs["runs"][0]["status"], json!("failed"));
        assert_eq!(runs["runs"][0]["jobs"][0]["status"], json!("failed"));
        assert!(runs["runs"][0]["jobs"][0].get("output").is_none());
        assert!(
            runs_bytes.len() < 2_000,
            "bounded run listing must stay small, was {} bytes",
            runs_bytes.len()
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn element_description_contract_is_closed_narrow_and_compact() {
        let schema =
            serde_json::to_value(rmcp::schemars::schema_for!(SetElementDescriptionsParams))
                .unwrap();
        assert_eq!(schema["additionalProperties"], json!(false));
        for field in ["projectName", "operationId", "readTokens", "updates"] {
            assert!(schema["required"]
                .as_array()
                .unwrap()
                .iter()
                .any(|required| required == field));
        }

        let update_ref = schema["properties"]["updates"]["items"]["$ref"]
            .as_str()
            .unwrap();
        let update_name = update_ref.strip_prefix("#/$defs/").unwrap();
        let update = &schema["$defs"][update_name];
        assert_eq!(update["additionalProperties"], json!(false));
        let properties = update["properties"].as_object().unwrap();
        assert_eq!(properties.len(), 2);
        assert!(properties.contains_key("externalId"));
        assert!(properties.contains_key("description"));
        assert!(update["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|required| required == "externalId"));
        assert!(update["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|required| required == "description"));

        let result_schema =
            serde_json::to_value(rmcp::schemars::schema_for!(DesignSaveResult)).unwrap();
        assert!(result_schema["properties"].get("structurizrDsl").is_none());
        assert!(result_schema["properties"].get("changedCount").is_some());
    }
    fn assert_portable_nonnegative_integer(schema: &serde_json::Value) {
        assert_eq!(schema["minimum"], json!(0));
        assert!(schema.get("format").is_none());
    }

    #[test]
    fn count_schemas_use_portable_nonnegative_validation() {
        let schemas = [
            (
                serde_json::to_value(rmcp::schemars::schema_for!(DesignOverviewParams)).unwrap(),
                "maxDepth",
            ),
            (
                serde_json::to_value(rmcp::schemars::schema_for!(DesignScopeParams)).unwrap(),
                "childrenDepth",
            ),
            (
                serde_json::to_value(rmcp::schemars::schema_for!(DesignSearchParams)).unwrap(),
                "limit",
            ),
            (
                serde_json::to_value(rmcp::schemars::schema_for!(DesignParams)).unwrap(),
                "maxDepth",
            ),
            (
                serde_json::to_value(rmcp::schemars::schema_for!(DesignParams)).unwrap(),
                "childrenDepth",
            ),
            (
                serde_json::to_value(rmcp::schemars::schema_for!(DesignParams)).unwrap(),
                "limit",
            ),
            (
                serde_json::to_value(rmcp::schemars::schema_for!(DesignSaveResult)).unwrap(),
                "changedCount",
            ),
            (
                serde_json::to_value(rmcp::schemars::schema_for!(TaskMockupPreview)).unwrap(),
                "contentIndex",
            ),
        ];

        for (schema, property) in schemas {
            assert_portable_nonnegative_integer(&schema["properties"][property]);
        }
    }

    /// `schema_with` replaces a field's type with a synthetic wrapper type, which defeats
    /// schemars' `Option` detection and would otherwise mark the field required. The
    /// `#[serde(default)]` attribute keeps these optional count fields optional.
    #[test]
    fn optional_count_fields_are_not_required() {
        let design = serde_json::to_value(rmcp::schemars::schema_for!(DesignParams)).unwrap();
        let required_names = |schema: &serde_json::Value| {
            schema["required"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|entry| entry.as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        };

        for (schema, property) in [
            (
                serde_json::to_value(rmcp::schemars::schema_for!(DesignOverviewParams)).unwrap(),
                "maxDepth",
            ),
            (
                serde_json::to_value(rmcp::schemars::schema_for!(DesignScopeParams)).unwrap(),
                "childrenDepth",
            ),
            (
                serde_json::to_value(rmcp::schemars::schema_for!(DesignSearchParams)).unwrap(),
                "limit",
            ),
        ] {
            assert!(
                !required_names(&schema).contains(&property.to_string()),
                "{property} must not be required"
            );
        }

        // The consolidated tool schema is what the model reads, so it must agree.
        assert_eq!(
            required_names(&design),
            vec!["operation".to_string(), "projectName".to_string()]
        );
    }

    /// The wire field is `projectName` only: no alias, no deprecation window. A caller still
    /// sending `projectId` must fail loudly rather than silently selecting a project.
    #[test]
    fn project_name_is_the_only_project_reference_field() {
        let schema = serde_json::to_value(rmcp::schemars::schema_for!(DesignParams)).unwrap();
        assert_eq!(schema["additionalProperties"], json!(false));
        assert!(schema["properties"].get("projectName").is_some());
        assert!(schema["properties"].get("projectId").is_none());

        let error = serde_json::from_value::<DesignParams>(json!({
            "operation": "search",
            "projectId": "adashi",
            "query": "revision",
        }))
        .expect_err("projectId must no longer be accepted");
        assert!(error.to_string().contains("projectId"), "{error}");
    }

    /// A task listing withholds closed work by default and says how much it withheld, and a task
    /// read returns link metadata rather than every linked design branch.
    #[test]
    fn task_listing_hides_closed_work_and_task_read_does_not_inline_scopes() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("adashi-mcp-task-states-{suffix}"));
        let settings_path = root.join("settings.json");
        let project_folder = root.join("project");
        let project = ProjectSettings {
            id: "task-states-test".into(),
            name: "Task States Test".into(),
            folder: project_folder.to_string_lossy().into_owned(),
        };
        settings::save(
            &settings_path,
            &AppSettings {
                window: WindowSettings {
                    width: 1000,
                    height: 700,
                    x: None,
                    y: None,
                },
                projects: vec![project.clone()],
                last_active_project_id: Some(project.id.clone()),
                rule_templates: vec![],
                architecture_projection: Default::default(),
            },
        )
        .unwrap();
        let db = crate::open_project_database(&project).unwrap();
        let project_row_id = project_row_id(&db).unwrap();
        let attachment: String = db
            .query_row(
                "SELECT external_id FROM c4_elements ORDER BY id LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let task = tasks::create_task(
            &db,
            project_row_id,
            NewTask {
                title: "Accepted work".into(),
                description: Some("Nothing to do here.".into()),
                design_specification_links: Some(vec![TaskDesignSpecificationLinkInput {
                    target_type: Some("element".into()),
                    design_external_id: attachment,
                }]),
            },
        )
        .unwrap();
        assert_eq!(task.state, "todo", "a new task starts unclaimed");
        drop(db);

        let server = AdashiMcpServer::new(settings_path);
        let listed = server
            .list_tasks(rmcp::handler::server::wrapper::Parameters(
                ListTasksParams {
                    project_name: project.name.clone(),
                    states: None,
                    limit: None,
                    cursor: None,
                },
            ))
            .unwrap()
            .0;
        assert_eq!(listed.tasks.len(), 1, "the unclaimed task is visible");
        assert_eq!(listed.closed_hidden, 0);

        // Close it through the lifecycle, then the default listing hides it and says so.
        let mut db = crate::open_project_database(&project).unwrap();
        let tx = db.transaction().unwrap();
        for state in ["active", "finished"] {
            tasks::update_task(
                &tx,
                project_row_id,
                UpdateTask {
                    task_id: task.id,
                    title: None,
                    description: None,
                    state: Some(state.into()),
                    design_specification_links: None,
                },
            )
            .unwrap();
        }
        tasks::close_task(&tx, project_row_id, task.id).unwrap();
        tx.commit().unwrap();
        drop(db);

        let hidden = server
            .list_tasks(rmcp::handler::server::wrapper::Parameters(
                ListTasksParams {
                    project_name: project.name.clone(),
                    states: None,
                    limit: None,
                    cursor: None,
                },
            ))
            .unwrap()
            .0;
        assert!(hidden.tasks.is_empty(), "{:?}", hidden.tasks);
        assert_eq!(hidden.closed_hidden, 1, "the omission must be stated");

        let explicit = server
            .list_tasks(rmcp::handler::server::wrapper::Parameters(
                ListTasksParams {
                    project_name: project.name.clone(),
                    states: Some(vec![tasks::TaskState::Closed]),
                    limit: None,
                    cursor: None,
                },
            ))
            .unwrap()
            .0;
        assert_eq!(explicit.tasks.len(), 1);
        assert_eq!(
            explicit.closed_hidden, 0,
            "an explicit filter hides nothing"
        );

        // A task read stays small: the linked branch is named, not inlined.
        let lean = server
            .get_task(rmcp::handler::server::wrapper::Parameters(TaskIdParams {
                project_name: project.name.clone(),
                task_id: task.id,
                include_design_scopes: false,
            }))
            .unwrap();
        let lean_json = serde_json::to_value(lean.structured_content.as_ref().unwrap()).unwrap();
        assert_eq!(lean_json["designSpecifications"][0]["scope"], json!(null));
        assert!(
            lean_json["designScopesIncluded"].is_null()
                || lean_json.get("designScopeHint").is_some(),
            "the read must say how to get the scopes"
        );
        let lean_bytes = serde_json::to_string(&lean_json).unwrap().len();
        assert!(lean_bytes < 4_000, "lean read was {lean_bytes} bytes");

        let full = server
            .get_task(rmcp::handler::server::wrapper::Parameters(TaskIdParams {
                project_name: project.name.clone(),
                task_id: task.id,
                include_design_scopes: true,
            }))
            .unwrap();
        let full_json = serde_json::to_value(full.structured_content.as_ref().unwrap()).unwrap();
        assert!(
            !full_json["designSpecifications"][0]["scope"].is_null(),
            "opting in must inline the scope"
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    /// A `memory:noteId` locator must resolve to exactly the addressed note.
    #[test]
    fn memory_get_resolves_an_exact_note_id() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("adashi-mcp-note-id-{suffix}"));
        let settings_path = root.join("settings.json");
        let project_folder = root.join("project");
        let project = ProjectSettings {
            id: "note-id-test".into(),
            name: "Note Id Test".into(),
            folder: project_folder.to_string_lossy().into_owned(),
        };
        settings::save(
            &settings_path,
            &AppSettings {
                window: WindowSettings {
                    width: 1000,
                    height: 700,
                    x: None,
                    y: None,
                },
                projects: vec![project.clone()],
                last_active_project_id: Some(project.id.clone()),
                rule_templates: vec![],
                architecture_projection: Default::default(),
            },
        )
        .unwrap();
        let mut db = crate::open_project_database(&project).unwrap();
        let project_row_id = project_row_id(&db).unwrap();
        for note_id in ["note-1", "note-2", "note-3"] {
            memory::append_note(
                &mut db,
                project_row_id,
                AppendMemoryNote {
                    note_id: note_id.to_string(),
                    operation_id: format!("op-{note_id}"),
                    run_id: format!("run-{note_id}"),
                    task_id: None,
                    body: format!("body of {note_id} sharing the word revision"),
                },
            )
            .unwrap();
        }
        drop(db);

        let server = AdashiMcpServer::new(settings_path);
        let read = |note_id: Option<&str>| {
            server
                .get_memory(Parameters(GetMemoryParams {
                    project_name: project.name.clone(),
                    query: None,
                    note_id: note_id.map(str::to_string),
                    run_id: None,
                    task_id: None,
                    include_superseded: false,
                }))
                .unwrap()
                .0
        };

        let addressed = read(Some("note-2"));
        assert_eq!(addressed.matched_notes, 1);
        assert_eq!(addressed.retained_notes, 3, "retention count is unchanged");
        assert_eq!(addressed.memory.notes.len(), 1);
        assert_eq!(addressed.memory.notes[0].note_id, "note-2");
        assert_eq!(
            addressed.memory.notes[0].body,
            "body of note-2 sharing the word revision"
        );

        assert_eq!(read(Some("missing")).matched_notes, 0);
        assert_eq!(read(None).matched_notes, 3);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mockup_revision_context_returns_structured_facts_and_png_image() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("adashi-mcp-mockup-{suffix}"));
        let settings_path = root.join("settings.json");
        let project_folder = root.join("project");
        let project = ProjectSettings {
            id: "mockup-test".into(),
            name: "Mockup Test".into(),
            folder: project_folder.to_string_lossy().into_owned(),
        };
        settings::save(
            &settings_path,
            &AppSettings {
                window: WindowSettings {
                    width: 1000,
                    height: 700,
                    x: None,
                    y: None,
                },
                projects: vec![project.clone()],
                last_active_project_id: Some(project.id.clone()),
                rule_templates: vec![],
                architecture_projection: Default::default(),
            },
        )
        .unwrap();
        let mut db = crate::open_project_database(&project).unwrap();
        let project_row_id = project_row_id(&db).unwrap();
        let attachment: String = db
            .query_row(
                "SELECT external_id FROM c4_elements ORDER BY id LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        mockups::create_mockup(&mut db, project_row_id, CreateMockupInput {
            external_id: "mockup-home".into(), title: "Home".into(), attached_to_external_id: attachment,
            viewport_width: 120, viewport_height: 80, screen: "Home".into(), state: "Default".into(), fidelity: "static".into(),
            schema_version: Some(1), accepted_svg: r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 120 80"><rect data-adashi-id="panel" width="120" height="80" fill="#fff"/></svg>"##.into(),
            operation_id: "mcp-mockup-fixture-1".to_string(),
            expected_accepted_version: 0,
            expected_working_version: 0,
        }).unwrap();
        drop(db);

        let server = AdashiMcpServer::new(settings_path);
        let result = server
            .mockup_get_revision_context(Parameters(MockupContextParams {
                project_name: project.id,
                external_id: "mockup-home".into(),
            }))
            .unwrap();
        assert!(result.structured_content.is_some());
        assert_eq!(result.content.len(), 2);
        assert!(matches!(result.content[1], ContentBlock::Image(_)));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn task_read_returns_full_direct_mockup_content_and_png_image() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("adashi-mcp-task-mockup-{suffix}"));
        let settings_path = root.join("settings.json");
        let project_folder = root.join("project");
        let project = ProjectSettings {
            id: "task-mockup-test".into(),
            name: "Task Mockup Test".into(),
            folder: project_folder.to_string_lossy().into_owned(),
        };
        settings::save(
            &settings_path,
            &AppSettings {
                window: WindowSettings {
                    width: 1000,
                    height: 700,
                    x: None,
                    y: None,
                },
                projects: vec![project.clone()],
                last_active_project_id: Some(project.id.clone()),
                rule_templates: vec![],
                architecture_projection: Default::default(),
            },
        )
        .unwrap();
        let mut db = crate::open_project_database(&project).unwrap();
        let project_row_id = project_row_id(&db).unwrap();
        let attachment: String = db
            .query_row(
                "SELECT external_id FROM c4_elements ORDER BY id LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let accepted_svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 120 80"><rect data-adashi-id="panel" width="120" height="80" fill="#fff"/></svg>"##;
        mockups::create_mockup(
            &mut db,
            project_row_id,
            CreateMockupInput {
                external_id: "mockup-home".into(),
                title: "Home".into(),
                attached_to_external_id: attachment.clone(),
                viewport_width: 120,
                viewport_height: 80,
                screen: "Home".into(),
                state: "Default".into(),
                fidelity: "static".into(),
                schema_version: Some(1),
                accepted_svg: accepted_svg.into(),
                operation_id: "mcp-mockup-fixture-2".to_string(),
                expected_accepted_version: 0,
                expected_working_version: 0,
            },
        )
        .unwrap();
        let task = tasks::create_task(
            &db,
            project_row_id,
            NewTask {
                title: "Implement home".into(),
                description: None,
                design_specification_links: Some(vec![
                    TaskDesignSpecificationLinkInput {
                        target_type: Some("element".into()),
                        design_external_id: attachment,
                    },
                    TaskDesignSpecificationLinkInput {
                        target_type: Some("mockup".into()),
                        design_external_id: "mockup-home".into(),
                    },
                ]),
            },
        )
        .unwrap();
        drop(db);

        let server = AdashiMcpServer::new(settings_path);
        let result = server
            .get_task(Parameters(TaskIdParams {
                project_name: project.id,
                task_id: task.id,
                include_design_scopes: true,
            }))
            .unwrap();
        let structured = result.structured_content.as_ref().unwrap();
        let specifications = structured["designSpecifications"].as_array().unwrap();

        assert_eq!(specifications.len(), 2);
        assert!(specifications[0].get("mockup").is_none());
        assert!(specifications[0]["scope"]["mockups"][0]
            .get("acceptedSvg")
            .is_none());
        assert_eq!(specifications[1]["mockup"]["acceptedSvg"], accepted_svg);
        assert_eq!(specifications[1]["mockupPreview"]["variant"], "accepted");
        assert_eq!(specifications[1]["mockupPreview"]["mimeType"], "image/png");
        assert_eq!(specifications[1]["mockupPreview"]["contentIndex"], 1);
        assert_eq!(result.content.len(), 2);
        assert!(matches!(result.content[1], ContentBlock::Image(_)));
        std::fs::remove_dir_all(root).unwrap();
    }
}
