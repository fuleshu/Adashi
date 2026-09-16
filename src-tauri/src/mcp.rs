use crate::concurrency::{self, MutationGuard, ResourceExpectation, ResourceIntent};
use crate::design::{
    self, DesignChange, DesignOverviewResult, DesignSaveResult, DesignScopeResult,
    DesignSearchResult, ElementDescriptionUpdate,
};
use crate::design_health;
use crate::grep::{self, GrepParams};
use crate::memory::{self, AppendMemoryNote, MemoryNote, ProjectMemory};
use crate::mockups::{self, MockupSummary, UiMockup};
use crate::project::{open_project_database, resolve_project_from_settings};
use crate::qa::{
    self, NewQaJob, QaDesignLinkInput, QaJob, QaJobQuery, QaJobSummary, QaRun, QaRunSummary,
    UpdateQaJob,
};
use crate::rules::{self, NewRule, Rule, UpdateRule};
use crate::settings::{self, AppSettings, ProjectSettings};
use crate::state as project_state;
use crate::tasks::{
    self, FinishTask, NewTask, Task, TaskDesignSpecificationLink, TaskDesignSpecificationLinkInput,
    UpdateTask,
};
use rmcp::handler::server::tool::IntoCallToolResult;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::model::{CallToolResult, ContentBlock, ErrorData};
use rmcp::transport::stdio;
use rmcp::{serve_server, tool, tool_router};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
mod context;
mod errors;
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

    fn open_project(
        &self,
        project_name: Option<&str>,
    ) -> Result<(ProjectSettings, rusqlite::Connection), ErrorData> {
        let settings = self.load_settings()?;
        let project = resolve_project_from_settings(&settings, project_name)
            .map_err(|err| ErrorData::invalid_params(err, None))?;
        let db = open_project_database(&project).map_err(internal_error)?;
        Ok((project, db))
    }

    /// The router method's body, reachable from other modules' tests without widening the
    /// tool surface itself.
    #[cfg(test)]
    pub(crate) fn grep_result_for_tests(
        &self,
        params: &GrepParams,
    ) -> Result<grep::GrepResult, String> {
        let (_project, db) = self
            .open_project(Some(params.project_name.as_str()))
            .map_err(|error| error.to_string())?;
        let project_row_id = project_row_id(&db)?;
        grep::search(&db, project_row_id, params)
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
    operation: DesignOperation,
    /// Configured project name (case-insensitive) or project id.
    project_name: String,
    /// get_overview: how deep to expand the C4/UML tree.
    #[serde(default)]
    #[schemars(schema_with = "nonnegative_count_schema")]
    max_depth: Option<usize>,
    /// get_scope: root element id.
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
        let (project, db) = self.open_project(Some(params.project_name.as_str()))?;
        let rules = rules::load_rules(&db).map_err(tool_error)?;
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
        let (project, db) = self.open_project(Some(params.project_name.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        context::build(
            &db,
            project_row_id,
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
        let (project, mut db) = self.open_project(Some(params.project_name.as_str()))?;
        let tx = db.transaction().map_err(internal_error)?;
        let project_row_id = project_row_id(&tx).map_err(tool_error)?;
        // A default listing withholds closed work; state how much, so the omission is visible
        // rather than something the caller has to discover.
        let closed_hidden = if params.states.is_none() {
            tasks::count_closed_tasks(&tx, project_row_id).map_err(tool_error)?
        } else {
            0
        };
        let revision = project_state::load_project_revision(&tx, project_row_id)
            .map_err(tool_error)?
            .revision;
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
        let (mut tasks, filtered_total) =
            tasks::load_task_summaries(&tx, project_row_id, &state_filter, after_id, limit + 1)
                .map_err(tool_error)?;
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
        tx.commit().map_err(internal_error)?;
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
        let (project, db) = self.open_project(Some(params.project_name.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let revision =
            project_state::load_project_revision(&db, project_row_id).map_err(tool_error)?;
        let task = tasks::load_task(&db, project_row_id, params.task_id).map_err(tool_error)?;
        let mut design_specifications = load_task_design_specifications(
            &db,
            project_row_id,
            &task,
            params.include_design_scopes,
        )
        .map_err(tool_error)?;
        let mut previews = Vec::new();
        for specification in &mut design_specifications {
            let Some(mockup) = specification.mockup.as_ref() else {
                continue;
            };
            let png = mockups::preview_base64(&db, mockup, "accepted").map_err(tool_error)?;
            specification.mockup_preview = Some(TaskMockupPreview {
                variant: "accepted".to_string(),
                mime_type: "image/png".to_string(),
                content_index: previews.len() + 1,
            });
            previews.push(ContentBlock::image(png, "image/png"));
        }

        let design_scope_hint = if params.include_design_scopes {
            "Scopes are inlined for the links above because includeDesignScopes was set.".to_string()
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
        let (project, db) = self.open_project(Some(params.project_name.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let mut memory = memory::load_memory(&db, project_row_id).map_err(tool_error)?;
        let retained = memory::load_retained_notes(&db, project_row_id).map_err(tool_error)?;
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
        let revision =
            project_state::load_project_revision(&db, project_row_id).map_err(tool_error)?;

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
        let (project, mut db) = self.open_project(Some(params.project_name.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let operation_id = params.operation_id.trim().to_string();
        let rule = if let Some(replayed) =
            concurrency::load_operation::<Rule>(&db, project_row_id, &operation_id)
                .map_err(tool_error)?
        {
            replayed
        } else {
            let tx = db.transaction().map_err(internal_error)?;
            let rule_id = rules::create_rule(
                &tx,
                project_row_id,
                NewRule {
                    name: params.name,
                    enabled: params.enabled,
                    intend: params.intend,
                    hook: params.hook,
                    prompt: params.prompt,
                },
            )
            .map_err(tool_error)?;
            concurrency::bump_version(&tx, project_row_id, "rule", &rule_id.to_string())
                .map_err(tool_error)?;
            project_state::bump_project_revision(&tx, project_row_id).map_err(tool_error)?;
            let rule = rules::load_rule(&tx, rule_id).map_err(tool_error)?;
            concurrency::record_no_op(&tx, project_row_id, &operation_id, &rule)
                .map_err(tool_error)?;
            tx.commit().map_err(internal_error)?;
            rule
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
        let (project, mut db) = self.open_project(Some(params.project_name.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let guard = single_resource_guard(
            params.operation_id,
            "rule",
            params.rule_id.to_string(),
            params.expected_version,
        );
        let rule = if let Some(replayed) =
            concurrency::load_operation::<Rule>(&db, project_row_id, &guard.operation_id)
                .map_err(tool_error)?
        {
            replayed
        } else {
            let tx = db.transaction().map_err(internal_error)?;
            concurrency::validate_guard(&tx, project_row_id, &guard).map_err(tool_error)?;
            let before = rules::load_rule(&tx, params.rule_id).map_err(tool_error)?;
            rules::update_rule(
                &tx,
                UpdateRule {
                    id: params.rule_id,
                    name: params.name,
                    enabled: params.enabled,
                    intend: params.intend,
                    hook: params.hook,
                    prompt: params.prompt,
                },
            )
            .map_err(tool_error)?;
            let updated = rules::load_rule(&tx, params.rule_id).map_err(tool_error)?;
            if same_value(&before, &updated).map_err(tool_error)? {
                tx.rollback().map_err(internal_error)?;
                concurrency::record_no_op(&db, project_row_id, &guard.operation_id, &before)
                    .map_err(tool_error)?;
                before
            } else {
                concurrency::bump_version(&tx, project_row_id, "rule", &params.rule_id.to_string())
                    .map_err(tool_error)?;
                project_state::bump_project_revision(&tx, project_row_id).map_err(tool_error)?;
                let updated = rules::load_rule(&tx, params.rule_id).map_err(tool_error)?;
                concurrency::record_no_op(&tx, project_row_id, &guard.operation_id, &updated)
                    .map_err(tool_error)?;
                tx.commit().map_err(internal_error)?;
                updated
            }
        };
        let revision =
            project_state::load_project_revision(&db, project_row_id).map_err(tool_error)?;
        Ok(Json(UpdateRuleResult {
            project_id: project.id,
            updated_rule_id: rule.id,
            revision: revision.revision,
        }))
    }

    fn delete_rule(
        &self,
        Parameters(params): Parameters<DeleteRuleParams>,
    ) -> Result<Json<DeleteRuleResult>, ErrorData> {
        let (project, mut db) = self.open_project(Some(params.project_name.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let guard = single_resource_guard(
            params.operation_id,
            "rule",
            params.rule_id.to_string(),
            params.expected_version,
        );
        if concurrency::load_operation::<i64>(&db, project_row_id, &guard.operation_id)
            .map_err(tool_error)?
            .is_none()
        {
            let tx = db.transaction().map_err(internal_error)?;
            concurrency::validate_guard(&tx, project_row_id, &guard).map_err(tool_error)?;
            rules::delete_rule(&tx, params.rule_id).map_err(tool_error)?;
            concurrency::tombstone_version(
                &tx,
                project_row_id,
                "rule",
                &params.rule_id.to_string(),
            )
            .map_err(tool_error)?;
            project_state::bump_project_revision(&tx, project_row_id).map_err(tool_error)?;
            concurrency::record_no_op(&tx, project_row_id, &guard.operation_id, &params.rule_id)
                .map_err(tool_error)?;
            tx.commit().map_err(internal_error)?;
        }
        let revision =
            project_state::load_project_revision(&db, project_row_id).map_err(tool_error)?;
        Ok(Json(DeleteRuleResult {
            project_id: project.id,
            deleted_rule_id: params.rule_id,
            revision: revision.revision,
        }))
    }

    fn create_task(
        &self,
        Parameters(params): Parameters<CreateTaskParams>,
    ) -> Result<Json<TaskMutationResult>, ErrorData> {
        let (project, mut db) = self.open_project(Some(params.project_name.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let operation_id = params.operation_id.trim().to_string();
        let task = if let Some(replayed) =
            concurrency::load_operation::<Task>(&db, project_row_id, &operation_id)
                .map_err(tool_error)?
        {
            replayed
        } else {
            let tx = db.transaction().map_err(internal_error)?;
            let created = tasks::create_task(
                &tx,
                project_row_id,
                NewTask {
                    title: params.title,
                    description: params.description,
                    design_specification_links: params.design_specification_links,
                },
            )
            .map_err(tool_error)?;
            concurrency::bump_version(&tx, project_row_id, "task", &created.id.to_string())
                .map_err(tool_error)?;
            project_state::bump_project_revision(&tx, project_row_id).map_err(tool_error)?;
            let created = tasks::load_task(&tx, project_row_id, created.id).map_err(tool_error)?;
            concurrency::record_no_op(&tx, project_row_id, &operation_id, &created)
                .map_err(tool_error)?;
            tx.commit().map_err(internal_error)?;
            created
        };
        let revision =
            project_state::load_project_revision(&db, project_row_id).map_err(tool_error)?;
        let design_specifications =
            load_task_design_specifications(&db, project_row_id, &task, false)
                .map_err(tool_error)?;

        Ok(Json(TaskMutationResult {
            project_id: project.id,
            project_name: project.name,
            revision: revision.revision,
            task,
            design_specifications,
        }))
    }

    fn update_task(
        &self,
        Parameters(params): Parameters<UpdateTaskParams>,
    ) -> Result<Json<TaskMutationResult>, ErrorData> {
        let (project, mut db) = self.open_project(Some(params.project_name.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let guard = single_resource_guard(
            params.operation_id,
            "task",
            params.task_id.to_string(),
            params.expected_version,
        );
        let task = if let Some(replayed) =
            concurrency::load_operation::<Task>(&db, project_row_id, &guard.operation_id)
                .map_err(tool_error)?
        {
            replayed
        } else {
            let tx = db.transaction().map_err(internal_error)?;
            concurrency::validate_guard(&tx, project_row_id, &guard).map_err(tool_error)?;
            let before =
                tasks::load_task(&tx, project_row_id, params.task_id).map_err(tool_error)?;
            let updated = tasks::update_task(
                &tx,
                project_row_id,
                UpdateTask {
                    task_id: params.task_id,
                    title: params.title,
                    description: params.description,
                    state: params.state,
                    design_specification_links: params.design_specification_links,
                },
            )
            .map_err(tool_error)?;
            if same_task_content(&before, &updated) {
                tx.rollback().map_err(internal_error)?;
                concurrency::record_no_op(&db, project_row_id, &guard.operation_id, &before)
                    .map_err(tool_error)?;
                before
            } else {
                concurrency::bump_version(&tx, project_row_id, "task", &params.task_id.to_string())
                    .map_err(tool_error)?;
                project_state::bump_project_revision(&tx, project_row_id).map_err(tool_error)?;
                let updated =
                    tasks::load_task(&tx, project_row_id, params.task_id).map_err(tool_error)?;
                concurrency::record_no_op(&tx, project_row_id, &guard.operation_id, &updated)
                    .map_err(tool_error)?;
                tx.commit().map_err(internal_error)?;
                updated
            }
        };
        let revision =
            project_state::load_project_revision(&db, project_row_id).map_err(tool_error)?;
        let design_specifications =
            load_task_design_specifications(&db, project_row_id, &task, false)
                .map_err(tool_error)?;

        Ok(Json(TaskMutationResult {
            project_id: project.id,
            project_name: project.name,
            revision: revision.revision,
            task,
            design_specifications,
        }))
    }

    fn finish_task(
        &self,
        Parameters(params): Parameters<FinishTaskParams>,
    ) -> Result<Json<TaskMutationResult>, ErrorData> {
        let (project, mut db) = self.open_project(Some(params.project_name.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let guard = single_resource_guard(
            params.operation_id,
            "task",
            params.task_id.to_string(),
            params.expected_version,
        );
        let task = if let Some(replayed) =
            concurrency::load_operation::<Task>(&db, project_row_id, &guard.operation_id)
                .map_err(tool_error)?
        {
            replayed
        } else {
            let tx = db.transaction().map_err(internal_error)?;
            concurrency::validate_guard(&tx, project_row_id, &guard).map_err(tool_error)?;
            let before =
                tasks::load_task(&tx, project_row_id, params.task_id).map_err(tool_error)?;
            let finished = tasks::finish_task(
                &tx,
                project_row_id,
                FinishTask {
                    task_id: params.task_id,
                    completion_memo: params.completion_memo,
                    created_files: params.created_files.unwrap_or_default(),
                    changed_files: params.changed_files.unwrap_or_default(),
                },
            )
            .map_err(tool_error)?;
            if same_task_content(&before, &finished) {
                tx.rollback().map_err(internal_error)?;
                concurrency::record_no_op(&db, project_row_id, &guard.operation_id, &before)
                    .map_err(tool_error)?;
                before
            } else {
                concurrency::bump_version(&tx, project_row_id, "task", &params.task_id.to_string())
                    .map_err(tool_error)?;
                project_state::bump_project_revision(&tx, project_row_id).map_err(tool_error)?;
                let finished =
                    tasks::load_task(&tx, project_row_id, params.task_id).map_err(tool_error)?;
                concurrency::record_no_op(&tx, project_row_id, &guard.operation_id, &finished)
                    .map_err(tool_error)?;
                tx.commit().map_err(internal_error)?;
                finished
            }
        };
        let revision =
            project_state::load_project_revision(&db, project_row_id).map_err(tool_error)?;
        let design_specifications =
            load_task_design_specifications(&db, project_row_id, &task, false)
                .map_err(tool_error)?;

        Ok(Json(TaskMutationResult {
            project_id: project.id,
            project_name: project.name,
            revision: revision.revision,
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
        let (project, mut db) = self.open_project(Some(params.project_name.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let guard = single_resource_guard(
            params.operation_id,
            "task",
            params.task_id.to_string(),
            params.expected_version,
        );
        let task = if let Some(replayed) =
            concurrency::load_operation::<Task>(&db, project_row_id, &guard.operation_id)
                .map_err(tool_error)?
        {
            replayed
        } else {
            let tx = db.transaction().map_err(internal_error)?;
            concurrency::validate_guard(&tx, project_row_id, &guard).map_err(tool_error)?;
            let before =
                tasks::load_task(&tx, project_row_id, params.task_id).map_err(tool_error)?;
            let closed =
                tasks::close_task(&tx, project_row_id, params.task_id).map_err(tool_error)?;
            if same_task_content(&before, &closed) {
                tx.rollback().map_err(internal_error)?;
                concurrency::record_no_op(&db, project_row_id, &guard.operation_id, &before)
                    .map_err(tool_error)?;
                before
            } else {
                concurrency::bump_version(&tx, project_row_id, "task", &params.task_id.to_string())
                    .map_err(tool_error)?;
                project_state::bump_project_revision(&tx, project_row_id).map_err(tool_error)?;
                let closed =
                    tasks::load_task(&tx, project_row_id, params.task_id).map_err(tool_error)?;
                concurrency::record_no_op(&tx, project_row_id, &guard.operation_id, &closed)
                    .map_err(tool_error)?;
                tx.commit().map_err(internal_error)?;
                closed
            }
        };
        let revision =
            project_state::load_project_revision(&db, project_row_id).map_err(tool_error)?;
        let design_specifications =
            load_task_design_specifications(&db, project_row_id, &task, false)
                .map_err(tool_error)?;

        Ok(Json(TaskMutationResult {
            project_id: project.id,
            project_name: project.name,
            revision: revision.revision,
            task,
            design_specifications,
        }))
    }

    fn delete_task(
        &self,
        Parameters(params): Parameters<DeleteTaskParams>,
    ) -> Result<Json<DeleteTaskResult>, ErrorData> {
        let (project, mut db) = self.open_project(Some(params.project_name.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let guard = single_resource_guard(
            params.operation_id,
            "task",
            params.task_id.to_string(),
            params.expected_version,
        );
        if concurrency::load_operation::<i64>(&db, project_row_id, &guard.operation_id)
            .map_err(tool_error)?
            .is_none()
        {
            let tx = db.transaction().map_err(internal_error)?;
            concurrency::validate_guard(&tx, project_row_id, &guard).map_err(tool_error)?;
            let dependent_jobs: i64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM qa_job_task_links WHERE task_id=?1",
                    [params.task_id],
                    |row| row.get(0),
                )
                .map_err(internal_error)?;
            if dependent_jobs > 0 {
                return Err(tool_error(format!(
                    "Task {} is linked from {dependent_jobs} QA job(s); remove or version those dependencies before deletion",
                    params.task_id
                )));
            }
            tasks::delete_task(&tx, project_row_id, params.task_id).map_err(tool_error)?;
            concurrency::tombstone_version(
                &tx,
                project_row_id,
                "task",
                &params.task_id.to_string(),
            )
            .map_err(tool_error)?;
            project_state::bump_project_revision(&tx, project_row_id).map_err(tool_error)?;
            concurrency::record_no_op(&tx, project_row_id, &guard.operation_id, &params.task_id)
                .map_err(tool_error)?;
            tx.commit().map_err(internal_error)?;
        }
        let revision =
            project_state::load_project_revision(&db, project_row_id).map_err(tool_error)?;

        Ok(Json(DeleteTaskResult {
            project_id: project.id,
            project_name: project.name,
            revision: revision.revision,
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
        let (project, mut db) = self.open_project(Some(params.project_name.as_str()))?;
        let tx = db.transaction().map_err(internal_error)?;
        let project_row_id = project_row_id(&tx).map_err(tool_error)?;
        let revision = project_state::load_project_revision(&tx, project_row_id)
            .map_err(tool_error)?
            .revision;
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
        let (mut jobs, filtered_total) =
            qa::load_job_summaries(&tx, project_row_id, params.query.as_ref(), after, limit + 1)
                .map_err(tool_error)?;
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
        tx.commit().map_err(internal_error)?;

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
        let (project, db) = self.open_project(Some(params.project_name.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let revision =
            project_state::load_project_revision(&db, project_row_id).map_err(tool_error)?;
        let job = qa::load_job(&db, project_row_id, params.qa_job_id).map_err(tool_error)?;

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
        let (project, db) = self.open_project(Some(params.project_name.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let revision =
            project_state::load_project_revision(&db, project_row_id).map_err(tool_error)?;
        let run = qa::load_run(&db, project_row_id, params.qa_run_id).map_err(tool_error)?;

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
        let (project, mut db) = self.open_project(Some(params.project_name.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let operation_id = params.operation_id.trim().to_string();
        let job = if let Some(replayed) =
            concurrency::load_operation::<QaJob>(&db, project_row_id, &operation_id)
                .map_err(tool_error)?
        {
            replayed
        } else {
            let tx = db.transaction().map_err(internal_error)?;
            let created = qa::create_job(
                &tx,
                project_row_id,
                NewQaJob {
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
            )
            .map_err(tool_error)?;
            concurrency::bump_version(&tx, project_row_id, "qa.job", &created.id.to_string())
                .map_err(tool_error)?;
            project_state::bump_project_revision(&tx, project_row_id).map_err(tool_error)?;
            let created = qa::load_job(&tx, project_row_id, created.id).map_err(tool_error)?;
            concurrency::record_no_op(&tx, project_row_id, &operation_id, &created)
                .map_err(tool_error)?;
            tx.commit().map_err(internal_error)?;
            created
        };
        let revision =
            project_state::load_project_revision(&db, project_row_id).map_err(tool_error)?;
        Ok(Json(QaJobResult {
            project_id: project.id,
            project_name: project.name,
            revision: revision.revision,
            job,
        }))
    }

    fn update_qa_job(
        &self,
        Parameters(params): Parameters<UpdateQaJobParams>,
    ) -> Result<Json<QaJobResult>, ErrorData> {
        let (project, mut db) = self.open_project(Some(params.project_name.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let guard = single_resource_guard(
            params.operation_id,
            "qa.job",
            params.qa_job_id.to_string(),
            params.expected_version,
        );
        let job = if let Some(replayed) =
            concurrency::load_operation::<QaJob>(&db, project_row_id, &guard.operation_id)
                .map_err(tool_error)?
        {
            replayed
        } else {
            let tx = db.transaction().map_err(internal_error)?;
            concurrency::validate_guard(&tx, project_row_id, &guard).map_err(tool_error)?;
            let before = qa::load_job(&tx, project_row_id, params.qa_job_id).map_err(tool_error)?;
            let updated = qa::update_job(
                &tx,
                project_row_id,
                UpdateQaJob {
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
            )
            .map_err(tool_error)?;
            if same_qa_definition(&before, &updated) {
                tx.rollback().map_err(internal_error)?;
                concurrency::record_no_op(&db, project_row_id, &guard.operation_id, &before)
                    .map_err(tool_error)?;
                before
            } else {
                concurrency::bump_version(
                    &tx,
                    project_row_id,
                    "qa.job",
                    &params.qa_job_id.to_string(),
                )
                .map_err(tool_error)?;
                project_state::bump_project_revision(&tx, project_row_id).map_err(tool_error)?;
                let updated =
                    qa::load_job(&tx, project_row_id, params.qa_job_id).map_err(tool_error)?;
                concurrency::record_no_op(&tx, project_row_id, &guard.operation_id, &updated)
                    .map_err(tool_error)?;
                tx.commit().map_err(internal_error)?;
                updated
            }
        };
        let revision =
            project_state::load_project_revision(&db, project_row_id).map_err(tool_error)?;
        Ok(Json(QaJobResult {
            project_id: project.id,
            project_name: project.name,
            revision: revision.revision,
            job,
        }))
    }

    fn delete_qa_job(
        &self,
        Parameters(params): Parameters<DeleteQaJobParams>,
    ) -> Result<Json<DeleteQaJobResult>, ErrorData> {
        let (project, mut db) = self.open_project(Some(params.project_name.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let guard = single_resource_guard(
            params.operation_id,
            "qa.job",
            params.qa_job_id.to_string(),
            params.expected_version,
        );
        if concurrency::load_operation::<i64>(&db, project_row_id, &guard.operation_id)
            .map_err(tool_error)?
            .is_none()
        {
            let tx = db.transaction().map_err(internal_error)?;
            concurrency::validate_guard(&tx, project_row_id, &guard).map_err(tool_error)?;
            qa::delete_job(&tx, project_row_id, params.qa_job_id).map_err(tool_error)?;
            concurrency::tombstone_version(
                &tx,
                project_row_id,
                "qa.job",
                &params.qa_job_id.to_string(),
            )
            .map_err(tool_error)?;
            project_state::bump_project_revision(&tx, project_row_id).map_err(tool_error)?;
            concurrency::record_no_op(&tx, project_row_id, &guard.operation_id, &params.qa_job_id)
                .map_err(tool_error)?;
            tx.commit().map_err(internal_error)?;
        }
        let revision =
            project_state::load_project_revision(&db, project_row_id).map_err(tool_error)?;
        Ok(Json(DeleteQaJobResult {
            project_id: project.id,
            project_name: project.name,
            revision: revision.revision,
            deleted_qa_job_id: params.qa_job_id,
        }))
    }

    fn run_qa_jobs(
        &self,
        Parameters(params): Parameters<RunQaJobsParams>,
    ) -> Result<Json<QaRunResult>, ErrorData> {
        let (project, db) = self.open_project(Some(params.project_name.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let run = if let Some(replayed) =
            concurrency::load_operation::<QaRun>(&db, project_row_id, &params.operation_id)
                .map_err(tool_error)?
        {
            replayed
        } else {
            let run = qa::run_jobs(
                &db,
                project_row_id,
                &project.folder,
                params.query,
                params.trigger_source.as_deref().unwrap_or("mcp"),
            )
            .map_err(tool_error)?;
            project_state::bump_project_revision(&db, project_row_id).map_err(tool_error)?;
            concurrency::record_no_op(&db, project_row_id, &params.operation_id, &run)
                .map_err(tool_error)?;
            run
        };
        let revision =
            project_state::load_project_revision(&db, project_row_id).map_err(tool_error)?;
        Ok(Json(QaRunResult {
            project_id: project.id,
            project_name: project.name,
            revision: revision.revision,
            run,
        }))
    }

    fn list_qa_runs(
        &self,
        Parameters(params): Parameters<ListQaRunsParams>,
    ) -> Result<Json<QaRunListResult>, ErrorData> {
        let (project, db) = self.open_project(Some(params.project_name.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let revision =
            project_state::load_project_revision(&db, project_row_id).map_err(tool_error)?;
        let runs = qa::load_run_summaries(&db, project_row_id, params.limit).map_err(tool_error)?;

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
        let (project, mut db) = self.open_project(Some(params.project_name.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let (memory, revision) = memory::compact_memory_review(
            &mut db,
            project_row_id,
            params.expected_version,
            &params.operation_id,
            params.memory,
            &params.superseded_note_ids,
        )
        .map_err(tool_error)?;

        Ok(Json(MemoryResult {
            project_id: project.id,
            project_name: project.name,
            revision,
            memory,
        }))
    }

    fn update_memory_rule(
        &self,
        Parameters(params): Parameters<UpdateMemoryRuleParams>,
    ) -> Result<Json<MemoryResult>, ErrorData> {
        let (project, mut db) = self.open_project(Some(params.project_name.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let (memory, revision) = memory::update_memory_rule(
            &mut db,
            project_row_id,
            params.expected_version,
            &params.operation_id,
            params.rule,
        )
        .map_err(tool_error)?;

        Ok(Json(MemoryResult {
            project_id: project.id,
            project_name: project.name,
            revision,
            memory,
        }))
    }

    fn append_memory_note(
        &self,
        Parameters(params): Parameters<AppendMemoryNoteParams>,
    ) -> Result<Json<MemoryNoteResult>, ErrorData> {
        let (project, mut db) = self.open_project(Some(params.project_name.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let (note, revision) = memory::append_note(
            &mut db,
            project_row_id,
            AppendMemoryNote {
                note_id: params.note_id,
                operation_id: params.operation_id,
                run_id: params.run_id,
                task_id: params.task_id,
                body: params.body,
            },
        )
        .map_err(tool_error)?;
        Ok(Json(MemoryNoteResult {
            project_id: project.id,
            project_name: project.name,
            revision,
            note,
        }))
    }

    fn publish_resource_intent(
        &self,
        Parameters(params): Parameters<PublishIntentParams>,
    ) -> Result<Json<ResourceIntentResult>, ErrorData> {
        let (project, db) = self.open_project(Some(params.project_name.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let intent = concurrency::publish_intent(
            &db,
            project_row_id,
            &params.agent_run_id,
            &params.resource_kind,
            &params.resource_id,
            params.ttl_seconds,
        )
        .map_err(tool_error)?;
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
        let (project, db) = self.open_project(Some(params.project_name.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let intents = concurrency::load_live_intents(&db, project_row_id).map_err(tool_error)?;
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
        let (_project, db) = self.open_project(Some(params.project_name.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let overview =
            design::load_overview(&db, project_row_id, params.max_depth).map_err(tool_error)?;
        Ok(Json(overview))
    }

    fn design_get_scope(
        &self,
        Parameters(params): Parameters<DesignScopeParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let (_project, db) = self.open_project(Some(params.project_name.as_str()))?;
        let db = db
            .unchecked_transaction()
            .map_err(|error| tool_error(error.to_string()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let scope = design::load_scope(
            &db,
            project_row_id,
            &params.element_id,
            params.include_ancestors.unwrap_or(true),
            params.children_depth,
            params.include_source.unwrap_or(false),
        )
        .map_err(tool_error)?;
        Ok(Json(
            design::documents::with_documents(&db, project_row_id, scope).map_err(tool_error)?,
        ))
    }

    fn design_search(
        &self,
        Parameters(params): Parameters<DesignSearchParams>,
    ) -> Result<Json<DesignSearchResult>, ErrorData> {
        let (_project, db) = self.open_project(Some(params.project_name.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let result = design::search(
            &db,
            project_row_id,
            &params.query,
            &params.kinds.unwrap_or_default(),
            params.limit.unwrap_or(20),
        )
        .map_err(tool_error)?;
        Ok(Json(result))
    }

    fn design_get_by_ids(
        &self,
        Parameters(params): Parameters<DesignByIdsParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let (_project, db) = self.open_project(Some(params.project_name.as_str()))?;
        let db = db
            .unchecked_transaction()
            .map_err(|error| tool_error(error.to_string()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let result = design::load_by_ids(&db, project_row_id, &params.ids).map_err(tool_error)?;
        Ok(Json(
            design::documents::with_documents(&db, project_row_id, result).map_err(tool_error)?,
        ))
    }

    fn design_get_bindings(
        &self,
        Parameters(params): Parameters<DesignBindingsParams>,
    ) -> Result<Json<Value>, ErrorData> {
        let (_project, db) = self.open_project(Some(params.project_name.as_str()))?;
        let db = db
            .unchecked_transaction()
            .map_err(|error| tool_error(error.to_string()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let result = design::load_by_bindings(
            &db,
            project_row_id,
            &params.files.unwrap_or_default(),
            &params.symbols.unwrap_or_default(),
        )
        .map_err(tool_error)?;
        Ok(Json(
            design::documents::with_documents(&db, project_row_id, result).map_err(tool_error)?,
        ))
    }

    fn design_save(
        &self,
        Parameters(params): Parameters<DesignSaveParams>,
    ) -> Result<Json<DesignSaveResult>, ErrorData> {
        let (_project, mut db) = self.open_project(Some(params.project_name.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let result = design::save_changes(
            &mut db,
            project_row_id,
            &design::documents::DocumentGuard {
                operation_id: params.operation_id,
                read_tokens: params.read_tokens,
            },
            &params.change_intent,
            &params.changes,
        )
        .map_err(tool_error)?;
        Ok(Json(result))
    }

    fn design_set_element_descriptions(
        &self,
        Parameters(params): Parameters<SetElementDescriptionsParams>,
    ) -> Result<Json<DesignSaveResult>, ErrorData> {
        let (_project, mut db) = self.open_project(Some(params.project_name.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let result = design::set_element_descriptions(
            &mut db,
            project_row_id,
            &design::documents::DocumentGuard {
                operation_id: params.operation_id,
                read_tokens: params.read_tokens,
            },
            &params.updates,
        )
        .map_err(tool_error)?;
        Ok(Json(result))
    }

    /// Design-to-code correspondence. Reads the project's source files, so it reports what the
    /// model currently claims and whether the code still supports it.
    fn design_health_check(
        &self,
        Parameters(params): Parameters<ProjectParams>,
    ) -> Result<Json<design_health::DesignHealthResult>, ErrorData> {
        let (project, db) = self.open_project(Some(params.project_name.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        design_health::scan_and_record(&db, project_row_id, std::path::Path::new(&project.folder))
            .map(Json)
            .map_err(tool_error)
    }

    fn mockup_list_pending_revisions(
        &self,
        Parameters(params): Parameters<ProjectParams>,
    ) -> Result<Json<MockupPendingResult>, ErrorData> {
        let (project, db) = self.open_project(Some(params.project_name.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let revision = project_state::load_project_revision(&db, project_row_id)
            .map_err(tool_error)?
            .revision;
        let mockups = mockups::load_pending(&db, project_row_id).map_err(tool_error)?;
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
        let (project, db) = self.open_project(Some(params.project_name.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let revision = project_state::load_project_revision(&db, project_row_id)
            .map_err(tool_error)?
            .revision;
        let mockup = mockups::load_mockup(&db, project_row_id, params.external_id.trim())
            .map_err(tool_error)?;
        let preview_variant = if mockup.working_svg.is_some() {
            "working"
        } else {
            "accepted"
        };
        let png = mockups::preview_base64(&db, &mockup, preview_variant).map_err(tool_error)?;
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
        name = "adashi_design",
        description = "Formal C4/UML design, binding, and UI-mockup API. Required fields per operation: save = projectName, operationId, changeIntent, changes; set_element_descriptions = projectName, operationId, readTokens, updates; get_scope = projectName, elementId; get_by_ids/get_documents = projectName, ids; search = projectName, query; get_overview/get_bindings/health/mockup_list_pending_revisions = projectName; mockup_get_revision_context = projectName, externalId. Every changes item needs op, e.g. upsert_element. Before editing existing content, use get_by_ids/get_scope/get_bindings: documents contains the complete editable document, documentId and opaque readToken. Pass copied {documentId,readToken} pairs as readTokens for every changed or deleted existing document, including cascading deletions. Creates need no token. Adashi handles dependency checks internally; no guard/readSet/writeSet. On out_of_date nothing is saved: merge your intended edits into the returned currentDocument and retry with its readToken and a new operationId. Never only replace the token on an old payload. get_documents accepts documentId values for exact full reads. Overview/search are navigation, not writable snapshots. Errors include the complete operation schema.",
        annotations(read_only_hint = true, destructive_hint = false)
    )]
    fn design(
        &self,
        Parameters(params): Parameters<DesignParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match params.operation {
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
                let (_project, db) = self.open_project(Some(params.project_name.as_str()))?;
                let tx = db
                    .unchecked_transaction()
                    .map_err(|error| tool_error(error.to_string()))?;
                let project_row_id = project_row_id(&tx).map_err(tool_error)?;
                let documents = design::documents::load_documents(&tx, project_row_id, &ids)
                    .map_err(tool_error)?;
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
        description = "Cross-domain search over project content: design (C4 elements, relationships, diagrams, mockups, bindings), tasks, and project memory. One match per line as grep output, with a drillable locator prefix: design:<externalId> opens the design get_scope operation, task:<id> opens the tasks get operation, memory:<noteId> opens the memory get operation with its noteId filter. The pattern is tolerant (case-insensitive, whitespace-separated terms are AND, quoted phrases are exact substrings, key:value clauses filter on in/file/type/state/limit and an unrecognised key is searched as text). An empty pattern returns the top-layer overview with counts. QA and rules are project tooling and are not searched.",
        annotations(read_only_hint = true, destructive_hint = false)
    )]
    fn grep(
        &self,
        Parameters(params): Parameters<GrepParams>,
    ) -> Result<Json<grep::GrepResult>, ErrorData> {
        let (_project, db) = self.open_project(Some(params.project_name.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        grep::search(&db, project_row_id, &params)
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        name = "adashi_tasks",
        description = "Project task API. Required fields per operation: create = projectName, operationId, title; list = projectName; update = projectName, operationId, expectedVersion, taskId; finish = projectName, operationId, expectedVersion, taskId, completionMemo; close = projectName, operationId, expectedVersion, taskId; delete = projectName, operationId, expectedVersion, taskId; get = projectName, taskId. expectedVersion is the task version from get/list, never the project revision. Use a unique operationId per mutation and reuse it only for an identical retry. Errors return the full selected operation schema.",
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
        description = "QA job definitions and execution API. Required fields per operation: create_job = projectName, operationId, name, command; update_job = projectName, operationId, expectedVersion, qaJobId; delete_job = projectName, operationId, expectedVersion, qaJobId; list_jobs = projectName (query, limit, cursor optional); run_jobs = projectName, operationId, query (e.g. {jobIds:[1]}; {} selects all enabled jobs); list_runs = projectName (limit optional); get_job = projectName, qaJobId; get_run = projectName, qaRunId. Lists return bounded metadata without console output; get_job/get_run return full evidence. expectedVersion is the job version. Use a unique operationId per mutation and reuse it only for an identical retry. Errors return the full selected operation schema.",
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
        description = "Project memory API. Select an `operation`: get (summary, protocol and notes), append (add a handover note), update (coordinator summary replacement), update_rule (memory protocol rule). Required fields per operation, all required unless marked optional: get = projectName (query, noteId, runId, taskId, includeSuperseded optional); append = projectName, noteId, operationId, body (runId, taskId optional), where runId is the run id that produced the note, e.g. run.<topic>-<yyyymmdd>, and defaults to operationId because retained notes are retrieved by this exact value; update = projectName, operationId, expectedVersion, memory (supersededNoteIds optional); update_rule = projectName, operationId, expectedVersion, rule.",
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
        description = "Lifecycle rule prompts and injection API. Select an `operation`: list, create, update, delete, get_rule_injections. Required fields per operation, all required unless marked optional: list = projectName; get_rule_injections = projectName, intend, hook (memoryContext optional); create = projectName, operationId, name, enabled, intend, hook, prompt; update = projectName, operationId, expectedVersion, ruleId, name, enabled, intend, hook, prompt; delete = projectName, operationId, expectedVersion, ruleId.",
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
        description = "Advisory resource-intent coordination API. Required fields per operation: publish = projectName, agentRunId, resourceKind, resourceId, ttlSeconds; list = projectName. Publishing creates or renews an expiring advisory intent; it does not grant write authority.",
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

fn single_resource_guard(
    operation_id: String,
    resource_kind: &str,
    resource_id: String,
    expected_version: i64,
) -> MutationGuard {
    MutationGuard {
        operation_id,
        read_set: Vec::new(),
        write_set: vec![ResourceExpectation {
            resource_kind: resource_kind.to_string(),
            resource_id,
            expected_version,
        }],
    }
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

fn same_value<T: Serialize>(left: &T, right: &T) -> Result<bool, String> {
    Ok(
        serde_json::to_value(left).map_err(|error| error.to_string())?
            == serde_json::to_value(right).map_err(|error| error.to_string())?,
    )
}

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
fn load_task_design_specifications(
    db: &rusqlite::Connection,
    project_row_id: i64,
    task: &Task,
    include_scopes: bool,
) -> Result<Vec<TaskDesignSpecificationBranch>, String> {
    task.design_specification_links
        .iter()
        .map(|link| {
            if !include_scopes {
                return Ok(TaskDesignSpecificationBranch {
                    link: link.clone(),
                    scope: None,
                    mockup: None,
                    mockup_preview: None,
                    note: None,
                });
            }

            let mockup = if link.target_type == "mockup" {
                Some(mockups::load_mockup(
                    db,
                    project_row_id,
                    link.design_external_id.as_str(),
                )?)
            } else {
                None
            };
            let root_id = match link.target_type.as_str() {
                "element" => Some(link.design_external_id.clone()),
                "uml" => db
                    .query_row(
                        "SELECT attached_to_external_id
                         FROM diagrams
                         WHERE key = ?1
                         LIMIT 1",
                        rusqlite::params![link.design_external_id],
                        |row| row.get::<_, Option<String>>(0),
                    )
                    .map_err(|err| err.to_string())?,
                "relationship" => None,
                "mockup" => match mockup.as_ref() {
                    Some(mockup) => Some(mockup.manifest.attached_to_external_id.clone()),
                    None => db
                        .query_row(
                            "SELECT attached_to_external_id FROM ui_mockups WHERE external_id=?1 LIMIT 1",
                            rusqlite::params![link.design_external_id],
                            |row| row.get::<_, String>(0),
                        )
                        .optional()
                        .map_err(|err| err.to_string())?,
                },
                _ => None,
            };

            let scope = match root_id {
                Some(root_id) => Some(design::load_scope(
                    db,
                    project_row_id,
                    &root_id,
                    true,
                    Some(2),
                    false,
                )?),
                None => None,
            };
            let note = if scope.is_some() {
                None
            } else {
                Some("This link does not resolve to an element-rooted design branch.".to_string())
            };

            Ok(TaskDesignSpecificationBranch {
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

    /// A capability tool publishes one flat argument schema, so a caller can only discover an
    /// operation's required fields from the tool description. Those requirements are asserted
    /// per operation, because a silently dropped field name turns every append into a failed
    /// call that the caller cannot self-correct.
    #[test]
    fn capability_descriptions_declare_each_operation_s_required_fields() {
        let tools = AdashiMcpServer::tool_router().list_all();
        let cases: [(&str, &[(&str, &[&str])]); 2] = [
            (
                "adashi_memory",
                &[
                    ("get", &[]),
                    ("append", &["projectName", "noteId", "operationId", "body"]),
                    (
                        "update",
                        &["projectName", "operationId", "expectedVersion", "memory"],
                    ),
                    (
                        "update_rule",
                        &["projectName", "operationId", "expectedVersion", "rule"],
                    ),
                ],
            ),
            (
                "adashi_rules",
                &[
                    ("list", &[]),
                    (
                        "create",
                        &[
                            "projectName",
                            "operationId",
                            "name",
                            "enabled",
                            "intend",
                            "hook",
                            "prompt",
                        ],
                    ),
                    (
                        "update",
                        &[
                            "projectName",
                            "operationId",
                            "expectedVersion",
                            "ruleId",
                            "name",
                            "enabled",
                            "intend",
                            "hook",
                            "prompt",
                        ],
                    ),
                    (
                        "delete",
                        &["projectName", "operationId", "expectedVersion", "ruleId"],
                    ),
                    ("get_rule_injections", &["projectName", "intend", "hook"]),
                ],
            ),
        ];

        for (tool, operations) in cases {
            let description = &tools
                .iter()
                .find(|tool_definition| tool_definition.name == tool)
                .unwrap_or_else(|| panic!("{tool} must be advertised"))
                .description;
            let description = description
                .as_deref()
                .unwrap_or_else(|| panic!("{tool} must have a description"));
            assert!(
                !description.contains("Required fields are listed per operation in the schema"),
                "{tool} must write out its per-operation requirements instead of pointing at the schema"
            );
            for (operation, fields) in operations {
                let prefix = format!("{operation} = ");
                let start = description
                    .find(&prefix)
                    .unwrap_or_else(|| panic!("{tool} description must name its `{operation}` operation's required fields"));
                let tail = &description[start + prefix.len()..];
                let listed = tail.split(';').next().unwrap_or(tail);
                for field in *fields {
                    assert!(
                        listed.contains(field),
                        "{tool} `{operation}` must list required field `{field}`; listed: {listed}"
                    );
                }
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
        assert!(message.contains("'body'"), "error must name the field: {message}");
        assert!(message.contains("'append'"), "error must name the operation: {message}");
        assert!(message.contains("1000 characters"), "error must state the real limit: {message}");

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
        let (_project, db) = server.open_project(Some("append-run-id-test")).unwrap();
        let notes = memory::load_retained_notes(&db, project_row_id(&db).unwrap()).unwrap();
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
        assert_eq!(conflict.data.as_ref().unwrap()["code"], json!("resource.conflict"));
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
        assert!(jobs["jobs"][0].get("latestRun").unwrap().get("output").is_none());
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
        assert_eq!(explicit.closed_hidden, 0, "an explicit filter hides nothing");

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
            lean_json["designScopesIncluded"].is_null() || lean_json.get("designScopeHint").is_some(),
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
    fn memory_get_resolves_an_exact_note_id() {        let suffix = SystemTime::now()
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
        assert_eq!(addressed.memory.notes[0].body, "body of note-2 sharing the word revision");

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
