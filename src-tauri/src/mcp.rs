use crate::concurrency::{self, MutationGuard, ResourceExpectation, ResourceIntent};
use crate::design::{
    self, DesignBindingsResult, DesignByIdsResult, DesignChange, DesignOverviewResult,
    DesignSaveResult, DesignScopeResult, DesignSearchResult, ElementDescriptionUpdate,
};
use crate::memory::{self, AppendMemoryNote, MemoryNote, ProjectMemory};
use crate::mockups::{self, MockupSummary, UiMockup};
use crate::project::{open_project_database, resolve_project_from_settings};
use crate::qa::{self, NewQaJob, QaDesignLinkInput, QaJob, QaJobQuery, QaRun, UpdateQaJob};
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
use serde_json::json;
mod context;
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
        project_id: Option<&str>,
    ) -> Result<(ProjectSettings, rusqlite::Connection), ErrorData> {
        let settings = self.load_settings()?;
        let project = resolve_project_from_settings(&settings, project_id)
            .map_err(|err| ErrorData::invalid_params(err, None))?;
        let db = open_project_database(&project).map_err(internal_error)?;
        Ok((project, db))
    }
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProjectParams {
    project_id: String,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetMemoryParams {
    project_id: String,
    /// Literal case-insensitive substring in retained note bodies.
    query: Option<String>,
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
    project_id: String,
    intend: String,
    hook: String,
    /// Summary by default; protocolOnly skips the summary for operational work.
    #[serde(default)]
    memory_context: MemoryContext,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateRuleParams {
    project_id: String,
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
    project_id: String,
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
    project_id: String,
    operation_id: String,
    expected_version: i64,
    rule_id: i64,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateTaskParams {
    project_id: String,
    operation_id: String,
    title: String,
    description: Option<String>,
    design_specification_links: Option<Vec<TaskDesignSpecificationLinkInput>>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateTaskParams {
    project_id: String,
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
    project_id: String,
    /// Omitted/null selects all states; [] selects none. Values are exact.
    states: Option<Vec<tasks::TaskState>>,
    /// Default 25, range 1..=100.
    limit: Option<u32>,
    /// Opaque continuation; keep the same states.
    cursor: Option<String>,
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
    project_id: String,
    task_id: i64,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DeleteTaskParams {
    project_id: String,
    task_id: i64,
    operation_id: String,
    expected_version: i64,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FinishTaskParams {
    project_id: String,
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
    project_id: String,
    query: Option<QaJobQuery>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QaJobIdParams {
    project_id: String,
    qa_job_id: i64,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DeleteQaJobParams {
    project_id: String,
    qa_job_id: i64,
    operation_id: String,
    expected_version: i64,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateQaJobParams {
    project_id: String,
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
    project_id: String,
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
    project_id: String,
    operation_id: String,
    query: QaJobQuery,
    trigger_source: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListQaRunsParams {
    project_id: String,
    limit: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateMemoryParams {
    project_id: String,
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
    project_id: String,
    operation_id: String,
    expected_version: i64,
    rule: String,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AppendMemoryNoteParams {
    project_id: String,
    note_id: String,
    operation_id: String,
    run_id: String,
    task_id: Option<i64>,
    body: String,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PublishIntentParams {
    project_id: String,
    agent_run_id: String,
    resource_kind: String,
    resource_id: String,
    ttl_seconds: i64,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DesignOverviewParams {
    project_id: String,
    #[schemars(schema_with = "nonnegative_count_schema")]
    max_depth: Option<usize>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DesignScopeParams {
    project_id: String,
    element_id: String,
    include_ancestors: Option<bool>,
    #[schemars(schema_with = "nonnegative_count_schema")]
    children_depth: Option<usize>,
    include_source: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DesignSearchParams {
    project_id: String,
    query: String,
    kinds: Option<Vec<String>>,
    #[schemars(schema_with = "nonnegative_count_schema")]
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DesignByIdsParams {
    project_id: String,
    ids: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DesignBindingsParams {
    project_id: String,
    files: Option<Vec<String>>,
    symbols: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DesignSaveParams {
    project_id: String,
    guard: MutationGuard,
    change_intent: String,
    changes: Vec<DesignChange>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SetElementDescriptionsParams {
    project_id: String,
    guard: MutationGuard,
    updates: Vec<ElementDescriptionUpdate>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MockupContextParams {
    project_id: String,
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
    design_specifications: Vec<TaskDesignSpecificationBranch>,
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
    project_id: String,
    project_name: String,
    revision: i64,
    jobs: Vec<QaJob>,
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
    runs: Vec<QaRun>,
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

/// Capability menu for the consolidated design/mockup tool. The model reads these
/// values straight from the JSON schema, so no memorization is required.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
enum DesignOperation {
    Save,
    GetScope,
    GetByIds,
    Search,
    GetOverview,
    GetBindings,
    SetElementDescriptions,
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
    project_id: String,
    /// get_overview: how deep to expand the C4/UML tree.
    #[schemars(schema_with = "nonnegative_count_schema")]
    max_depth: Option<usize>,
    /// get_scope: root element id.
    element_id: Option<String>,
    /// get_scope: include ancestors.
    include_ancestors: Option<bool>,
    /// get_scope: child expansion depth.
    #[schemars(schema_with = "nonnegative_count_schema")]
    children_depth: Option<usize>,
    /// get_scope: include canonical source.
    include_source: Option<bool>,
    /// search: text query.
    query: Option<String>,
    /// search: kinds to restrict results to.
    kinds: Option<Vec<String>>,
    /// search: result limit.
    #[schemars(schema_with = "nonnegative_count_schema")]
    limit: Option<usize>,
    /// get_by_ids: stored design ids.
    ids: Option<Vec<String>>,
    /// get_bindings: files to resolve bindings for.
    files: Option<Vec<String>>,
    /// get_bindings: symbols to resolve bindings for.
    symbols: Option<Vec<String>>,
    /// save: mutation guard.
    guard: Option<MutationGuard>,
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
    project_id: String,
    /// create/update/finish/delete: idempotency key.
    operation_id: Option<String>,
    /// create (required) / update: task title.
    title: Option<String>,
    /// create/update: task description.
    description: Option<String>,
    /// create/update: ordered design specification links.
    design_specification_links: Option<Vec<TaskDesignSpecificationLinkInput>>,
    /// update: new state.
    state: Option<String>,
    /// update/finish/delete: expected task version.
    expected_version: Option<i64>,
    /// update/finish/delete/get: task id.
    task_id: Option<i64>,
    /// finish: completion memo.
    completion_memo: Option<String>,
    /// finish: files created.
    created_files: Option<Vec<String>>,
    /// finish: files changed.
    changed_files: Option<Vec<String>>,
    /// list: states filter (omitted=all, empty=none).
    states: Option<Vec<tasks::TaskState>>,
    /// list: page size (default 25, 1..=100).
    limit: Option<u32>,
    /// list: opaque continuation cursor.
    cursor: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QaParams {
    operation: QaOperation,
    project_id: String,
    /// list_jobs / run_jobs: selection filters.
    query: Option<QaJobQuery>,
    /// get_job/update_job/delete_job: job id.
    qa_job_id: Option<i64>,
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
    /// list_runs: max runs returned.
    limit: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MemoryParams {
    operation: MemoryOperation,
    project_id: String,
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
    project_id: String,
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
    project_id: String,
    /// publish: agent run id.
    agent_run_id: Option<String>,
    /// publish: resource kind.
    resource_kind: Option<String>,
    /// publish: resource id.
    resource_id: Option<String>,
    /// publish: time-to-live in seconds.
    ttl_seconds: Option<i64>,
}

#[tool_router(server_handler)]
impl AdashiMcpServer {
    fn list_rules(
        &self,
        Parameters(params): Parameters<ProjectParams>,
    ) -> Result<Json<RuleListResult>, ErrorData> {
        let (project, db) = self.open_project(Some(params.project_id.as_str()))?;
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
        let (project, db) = self.open_project(Some(params.project_id.as_str()))?;
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
        let (project, mut db) = self.open_project(Some(params.project_id.as_str()))?;
        let tx = db.transaction().map_err(internal_error)?;
        let project_row_id = project_row_id(&tx).map_err(tool_error)?;
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
                || cursor.states != params.states
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
        let (mut tasks, filtered_total) = tasks::load_task_summaries(
            &tx,
            project_row_id,
            params.states.as_deref(),
            after_id,
            limit + 1,
        )
        .map_err(tool_error)?;
        let has_more = tasks.len() > limit as usize;
        tasks.truncate(limit as usize);
        let next_cursor = if has_more {
            let cursor = TaskCursor {
                contract_version: 2,
                project_id: project.id.clone(),
                revision,
                states: params.states,
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
            has_more,
            next_cursor,
        }))
    }

    fn get_task(
        &self,
        Parameters(params): Parameters<TaskIdParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let (project, db) = self.open_project(Some(params.project_id.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let revision =
            project_state::load_project_revision(&db, project_row_id).map_err(tool_error)?;
        let task = tasks::load_task(&db, project_row_id, params.task_id).map_err(tool_error)?;
        let mut design_specifications =
            load_task_design_specifications(&db, project_row_id, &task, true)
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

        let payload = TaskReadResult {
            project_id: project.id,
            project_name: project.name,
            revision: revision.revision,
            task,
            design_specifications,
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
        let (project, db) = self.open_project(Some(params.project_id.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let mut memory = memory::load_memory(&db, project_row_id).map_err(tool_error)?;
        let retained = memory::load_retained_notes(&db, project_row_id).map_err(tool_error)?;
        let retained_notes = retained.len() as u32;
        let query = params.query.as_deref().map(str::to_lowercase);
        memory.notes = retained
            .into_iter()
            .filter(|note| {
                (params.include_superseded || note.superseded_by_version.is_none())
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
        let (project, mut db) = self.open_project(Some(params.project_id.as_str()))?;
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
        let (project, mut db) = self.open_project(Some(params.project_id.as_str()))?;
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
        let (project, mut db) = self.open_project(Some(params.project_id.as_str()))?;
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
        let (project, mut db) = self.open_project(Some(params.project_id.as_str()))?;
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
        let (project, mut db) = self.open_project(Some(params.project_id.as_str()))?;
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
        let (project, mut db) = self.open_project(Some(params.project_id.as_str()))?;
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

    fn delete_task(
        &self,
        Parameters(params): Parameters<DeleteTaskParams>,
    ) -> Result<Json<DeleteTaskResult>, ErrorData> {
        let (project, mut db) = self.open_project(Some(params.project_id.as_str()))?;
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
        let (project, db) = self.open_project(Some(params.project_id.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let revision =
            project_state::load_project_revision(&db, project_row_id).map_err(tool_error)?;
        let jobs = qa::load_jobs(&db, project_row_id, params.query.as_ref()).map_err(tool_error)?;

        Ok(Json(QaJobListResult {
            project_id: project.id,
            project_name: project.name,
            revision: revision.revision,
            jobs,
        }))
    }

    fn get_qa_job(
        &self,
        Parameters(params): Parameters<QaJobIdParams>,
    ) -> Result<Json<QaJobResult>, ErrorData> {
        let (project, db) = self.open_project(Some(params.project_id.as_str()))?;
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

    fn create_qa_job(
        &self,
        Parameters(params): Parameters<CreateQaJobParams>,
    ) -> Result<Json<QaJobResult>, ErrorData> {
        let (project, mut db) = self.open_project(Some(params.project_id.as_str()))?;
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
        let (project, mut db) = self.open_project(Some(params.project_id.as_str()))?;
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
        let (project, mut db) = self.open_project(Some(params.project_id.as_str()))?;
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
        let (project, db) = self.open_project(Some(params.project_id.as_str()))?;
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
        let (project, db) = self.open_project(Some(params.project_id.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let revision =
            project_state::load_project_revision(&db, project_row_id).map_err(tool_error)?;
        let runs = qa::load_runs(&db, project_row_id, params.limit).map_err(tool_error)?;

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
        let (project, mut db) = self.open_project(Some(params.project_id.as_str()))?;
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
        let (project, mut db) = self.open_project(Some(params.project_id.as_str()))?;
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
        let (project, mut db) = self.open_project(Some(params.project_id.as_str()))?;
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
        let (project, db) = self.open_project(Some(params.project_id.as_str()))?;
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
        let (project, db) = self.open_project(Some(params.project_id.as_str()))?;
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
        let (_project, db) = self.open_project(Some(params.project_id.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let overview =
            design::load_overview(&db, project_row_id, params.max_depth).map_err(tool_error)?;
        Ok(Json(overview))
    }

    fn design_get_scope(
        &self,
        Parameters(params): Parameters<DesignScopeParams>,
    ) -> Result<Json<DesignScopeResult>, ErrorData> {
        let (_project, db) = self.open_project(Some(params.project_id.as_str()))?;
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
        Ok(Json(scope))
    }

    fn design_search(
        &self,
        Parameters(params): Parameters<DesignSearchParams>,
    ) -> Result<Json<DesignSearchResult>, ErrorData> {
        let (_project, db) = self.open_project(Some(params.project_id.as_str()))?;
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
    ) -> Result<Json<DesignByIdsResult>, ErrorData> {
        let (_project, db) = self.open_project(Some(params.project_id.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let result = design::load_by_ids(&db, project_row_id, &params.ids).map_err(tool_error)?;
        Ok(Json(result))
    }

    fn design_get_bindings(
        &self,
        Parameters(params): Parameters<DesignBindingsParams>,
    ) -> Result<Json<DesignBindingsResult>, ErrorData> {
        let (_project, db) = self.open_project(Some(params.project_id.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let result = design::load_by_bindings(
            &db,
            project_row_id,
            &params.files.unwrap_or_default(),
            &params.symbols.unwrap_or_default(),
        )
        .map_err(tool_error)?;
        Ok(Json(result))
    }

    fn design_save(
        &self,
        Parameters(params): Parameters<DesignSaveParams>,
    ) -> Result<Json<DesignSaveResult>, ErrorData> {
        let (_project, mut db) = self.open_project(Some(params.project_id.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let result = design::save_changes(
            &mut db,
            project_row_id,
            &params.guard,
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
        let (_project, mut db) = self.open_project(Some(params.project_id.as_str()))?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let result = design::set_element_descriptions(
            &mut db,
            project_row_id,
            &params.guard,
            &params.updates,
        )
        .map_err(tool_error)?;
        Ok(Json(result))
    }

    fn mockup_list_pending_revisions(
        &self,
        Parameters(params): Parameters<ProjectParams>,
    ) -> Result<Json<MockupPendingResult>, ErrorData> {
        let (project, db) = self.open_project(Some(params.project_id.as_str()))?;
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
        let (project, db) = self.open_project(Some(params.project_id.as_str()))?;
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
        description = "Formal C4/UML design, binding, and UI-mockup API. Select an `operation`: save (transactional changeset), get_scope (read a C4 branch), get_by_ids (read explicit ids), search (text search), get_overview (top-down overview), get_bindings (file/symbol bindings), set_element_descriptions (set C4 element descriptions), mockup_list_pending_revisions, mockup_get_revision_context. Required fields are listed per operation in the schema.",
        annotations(read_only_hint = true, destructive_hint = false)
    )]
    fn design(
        &self,
        Parameters(params): Parameters<DesignParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match params.operation {
            DesignOperation::Save => {
                let guard = required(params.guard, "guard")?;
                let change_intent = required(params.change_intent, "changeIntent")?;
                let changes = required(params.changes, "changes")?;
                self.design_save(Parameters(DesignSaveParams {
                    project_id: params.project_id,
                    guard,
                    change_intent,
                    changes,
                }))?
                .into_call_tool_result()
            }
            DesignOperation::GetScope => {
                let element_id = required(params.element_id, "elementId")?;
                self.design_get_scope(Parameters(DesignScopeParams {
                    project_id: params.project_id,
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
                    project_id: params.project_id,
                    ids,
                }))?
                .into_call_tool_result()
            }
            DesignOperation::Search => {
                let query = required(params.query, "query")?;
                self.design_search(Parameters(DesignSearchParams {
                    project_id: params.project_id,
                    query,
                    kinds: params.kinds,
                    limit: params.limit,
                }))?
                .into_call_tool_result()
            }
            DesignOperation::GetOverview => self
                .design_get_overview(Parameters(DesignOverviewParams {
                    project_id: params.project_id,
                    max_depth: params.max_depth,
                }))?
                .into_call_tool_result(),
            DesignOperation::GetBindings => self
                .design_get_bindings(Parameters(DesignBindingsParams {
                    project_id: params.project_id,
                    files: params.files,
                    symbols: params.symbols,
                }))?
                .into_call_tool_result(),
            DesignOperation::SetElementDescriptions => {
                let guard = required(params.guard, "guard")?;
                let updates = required(params.updates, "updates")?;
                self.design_set_element_descriptions(Parameters(SetElementDescriptionsParams {
                    project_id: params.project_id,
                    guard,
                    updates,
                }))?
                .into_call_tool_result()
            }
            DesignOperation::MockupListPendingRevisions => self
                .mockup_list_pending_revisions(Parameters(ProjectParams {
                    project_id: params.project_id,
                }))?
                .into_call_tool_result(),
            DesignOperation::MockupGetRevisionContext => {
                let external_id = required(params.external_id, "externalId")?;
                self.mockup_get_revision_context(Parameters(MockupContextParams {
                    project_id: params.project_id,
                    external_id,
                }))?
                .into_call_tool_result()
            }
        }
    }

    #[tool(
        name = "adashi_tasks",
        description = "Project task API. Select an `operation`: create, list, update, finish, delete, get. Required fields are listed per operation in the schema.",
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
                    project_id: params.project_id,
                    operation_id,
                    title,
                    description: params.description,
                    design_specification_links: params.design_specification_links,
                }))?
                .into_call_tool_result()
            }
            TasksOperation::List => self
                .list_tasks(Parameters(ListTasksParams {
                    project_id: params.project_id,
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
                    project_id: params.project_id,
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
                    project_id: params.project_id,
                    operation_id,
                    expected_version,
                    task_id,
                    completion_memo,
                    created_files: params.created_files,
                    changed_files: params.changed_files,
                }))?
                .into_call_tool_result()
            }
            TasksOperation::Delete => {
                let operation_id = required(params.operation_id, "operationId")?;
                let expected_version = required(params.expected_version, "expectedVersion")?;
                let task_id = required(params.task_id, "taskId")?;
                self.delete_task(Parameters(DeleteTaskParams {
                    project_id: params.project_id,
                    task_id,
                    operation_id,
                    expected_version,
                }))?
                .into_call_tool_result()
            }
            TasksOperation::Get => {
                let task_id = required(params.task_id, "taskId")?;
                self.get_task(Parameters(TaskIdParams {
                    project_id: params.project_id,
                    task_id,
                }))?
                .into_call_tool_result()
            }
        }
    }

    #[tool(
        name = "adashi_qa",
        description = "QA job definitions and execution API. Select an `operation`: create_job, update_job, delete_job, list_jobs, run_jobs, list_runs, get_job. Required fields are listed per operation in the schema.",
        annotations(read_only_hint = false, destructive_hint = true)
    )]
    fn qa(&self, Parameters(params): Parameters<QaParams>) -> Result<CallToolResult, ErrorData> {
        match params.operation {
            QaOperation::ListJobs => self
                .list_qa_jobs(Parameters(ListQaJobsParams {
                    project_id: params.project_id,
                    query: params.query,
                }))?
                .into_call_tool_result(),
            QaOperation::GetJob => {
                let qa_job_id = required(params.qa_job_id, "qaJobId")?;
                self.get_qa_job(Parameters(QaJobIdParams {
                    project_id: params.project_id,
                    qa_job_id,
                }))?
                .into_call_tool_result()
            }
            QaOperation::CreateJob => {
                let operation_id = required(params.operation_id, "operationId")?;
                let name = required(params.name, "name")?;
                let command = required(params.command, "command")?;
                self.create_qa_job(Parameters(CreateQaJobParams {
                    project_id: params.project_id,
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
                    project_id: params.project_id,
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
                    project_id: params.project_id,
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
                    project_id: params.project_id,
                    operation_id,
                    query,
                    trigger_source: params.trigger_source,
                }))?
                .into_call_tool_result()
            }
            QaOperation::ListRuns => self
                .list_qa_runs(Parameters(ListQaRunsParams {
                    project_id: params.project_id,
                    limit: params.limit,
                }))?
                .into_call_tool_result(),
        }
    }

    #[tool(
        name = "adashi_memory",
        description = "Project memory API. Select an `operation`: get (summary, protocol and notes), append (add a handover note), update (coordinator summary replacement), update_rule (memory protocol rule). Required fields are listed per operation in the schema.",
        annotations(read_only_hint = true, destructive_hint = false)
    )]
    fn memory(
        &self,
        Parameters(params): Parameters<MemoryParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match params.operation {
            MemoryOperation::Get => self
                .get_memory(Parameters(GetMemoryParams {
                    project_id: params.project_id,
                    query: params.query,
                    run_id: params.run_id,
                    task_id: params.task_id,
                    include_superseded: params.include_superseded,
                }))?
                .into_call_tool_result(),
            MemoryOperation::Append => {
                let note_id = required(params.note_id, "noteId")?;
                let operation_id = required(params.operation_id, "operationId")?;
                let run_id = required(params.run_id, "runId")?;
                let body = required(params.body, "body")?;
                self.append_memory_note(Parameters(AppendMemoryNoteParams {
                    project_id: params.project_id,
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
                    project_id: params.project_id,
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
                    project_id: params.project_id,
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
        description = "Lifecycle rule prompts and injection API. Select an `operation`: list, create, update, delete, get_rule_injections. Required fields are listed per operation in the schema.",
        annotations(read_only_hint = true, destructive_hint = false)
    )]
    fn rules(
        &self,
        Parameters(params): Parameters<RulesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match params.operation {
            RulesOperation::List => self
                .list_rules(Parameters(ProjectParams {
                    project_id: params.project_id,
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
                    project_id: params.project_id,
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
                    project_id: params.project_id,
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
                    project_id: params.project_id,
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
                    project_id: params.project_id,
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
        description = "Advisory resource-intent coordination API. Select an `operation`: publish (create/renew an expiring intent) or list (live intents after pruning). Required fields are listed per operation in the schema.",
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
                    project_id: params.project_id,
                    agent_run_id,
                    resource_kind,
                    resource_id,
                    ttl_seconds,
                }))?
                .into_call_tool_result()
            }
            IntentsOperation::List => self
                .list_resource_intents(Parameters(ProjectParams {
                    project_id: params.project_id,
                }))?
                .into_call_tool_result(),
        }
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

fn load_task_design_specifications(
    db: &rusqlite::Connection,
    project_row_id: i64,
    task: &Task,
    include_linked_mockup_content: bool,
) -> Result<Vec<TaskDesignSpecificationBranch>, String> {
    task.design_specification_links
        .iter()
        .map(|link| {
            let mockup = if include_linked_mockup_content && link.target_type == "mockup" {
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

fn required<T>(value: Option<T>, field: &str) -> Result<T, ErrorData> {
    value.ok_or_else(|| missing_field(field))
}

fn tool_error(message: String) -> ErrorData {
    let value = serde_json::from_str::<serde_json::Value>(&message)
        .unwrap_or_else(|_| json!({ "message": message }));
    ErrorData::invalid_params("Adashi MCP request failed", Some(value))
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
                    "search",
                    "get_overview",
                    "get_bindings",
                    "set_element_descriptions",
                    "mockup_list_pending_revisions",
                    "mockup_get_revision_context",
                ],
            ),
            (
                serde_json::to_value(rmcp::schemars::schema_for!(TasksParams)).unwrap(),
                vec!["create", "list", "update", "finish", "delete", "get"],
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

    #[test]
    fn element_description_contract_is_closed_narrow_and_compact() {
        let schema =
            serde_json::to_value(rmcp::schemars::schema_for!(SetElementDescriptionsParams))
                .unwrap();
        assert_eq!(schema["additionalProperties"], json!(false));
        for field in ["projectId", "guard", "updates"] {
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
                project_id: project.id,
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
                project_id: project.id,
                task_id: task.id,
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
