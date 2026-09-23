use crate::{
    coordination::*, design::*, documents::*, fixed_hooks::*, memory::*, mockups::*, qa::*,
    rules::*, search::*, tasks::*, *,
};
use serde::{Deserialize, Serialize};

/// Lifecycle work is explicit. Inspect/read must never initialize, migrate or
/// change a computer mapping. Existing canonical identity wins over local aliases.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenRequest {
    pub location: String,
    pub registered_identity: ProjectIdentity,
    pub computer_id: String,
    pub checkout_path: String,
    pub mode: OpenMode,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OpenMode {
    ReadOnly,
    ReadWrite,
    InitializeOrMigrate,
}

/// The selecting factory resolves the descriptor/credential profile outside this
/// crate. A backend-specific factory owns format/schema migration and seeding.
pub trait StorageFactory {
    fn open(&self, request: &OpenRequest) -> StorageResult<Box<dyn StorageBackend>>;
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMetadata {
    pub identity: ProjectIdentity,
    pub record_id: i64,
    pub schema_version: i64,
    pub revision: i64,
    pub updated_at: String,
    pub cursor: ChangeCursor,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerCheckout {
    pub computer_id: String,
    pub repository_path: String,
}

/// Integer identities remain available to the existing desktop transport.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Identified<T> {
    pub id: i64,
    pub value: T,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub structurizr_dsl: String,
    pub structurizr_json: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesignInventory {
    pub workspace: Workspace,
    pub elements: Vec<Identified<DesignElementRecord>>,
    pub relationships: Vec<Identified<DesignRelationshipRecord>>,
    pub diagrams: Vec<Identified<DesignDiagramRecord>>,
    pub bindings: Vec<DesignBindingRecord>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScopeQuery {
    pub element_id: String,
    pub children_depth: Option<usize>,
    pub include_ancestors: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BindingQuery {
    pub files: Vec<String>,
    pub symbols: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesignSearchQuery {
    pub query: String,
    pub kinds: Vec<String>,
    pub limit: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskQuery {
    /// Explicit, including [] for none. Transports apply their own default.
    pub states: Vec<TaskState>,
    pub after_id: Option<i64>,
    pub limit: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskPage {
    pub tasks: Vec<TaskSummary>,
    pub next_after_id: Option<i64>,
    /// Total matching tasks before pagination, separate from closed_count.
    pub total: i64,
    pub closed_count: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Guideline {
    pub id: i64,
    pub title: String,
    pub body: String,
    pub severity: String,
    pub created_at: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostTaskCommand {
    pub id: i64,
    pub label: String,
    pub command: String,
    pub trigger: String,
    pub created_at: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QaCheck {
    pub id: i64,
    pub label: String,
    pub command: String,
    pub required: bool,
    pub created_at: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskQaEntry {
    pub id: i64,
    pub task_id: i64,
    pub label: String,
    pub status: String,
    pub body: String,
    pub created_at: String,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyContent {
    pub guidelines: Vec<Guideline>,
    pub post_task_commands: Vec<PostTaskCommand>,
    pub qa_checks: Vec<QaCheck>,
    pub task_qa_entries: Vec<TaskQaEntry>,
}

/// Owned by a single project handle. All methods observe ONE committed instant,
/// including content, resource versions, canonical tokens, receipts and cursor.
/// Drop releases the read transaction/pin; reads never modify persisted content.
/// No method has an "unsupported" default: a full adapter provides every domain.
pub trait ReadSnapshot {
    fn metadata(&self) -> &ProjectMetadata;
    fn computer_checkouts(&self) -> StorageResult<Vec<ComputerCheckout>>;
    fn design_inventory(&self) -> StorageResult<DesignInventory>;
    fn design_overview(&self, max_depth: Option<usize>) -> StorageResult<DesignOverviewResult>;
    fn design_scope(&self, query: &ScopeQuery) -> StorageResult<DesignScopeResult>;
    fn design_by_ids(&self, ids: &[String]) -> StorageResult<DesignByIdsResult>;
    fn design_bindings(&self, query: &BindingQuery) -> StorageResult<DesignBindingsResult>;
    fn design_documents(&self, ids: &[String]) -> StorageResult<Vec<DesignDocument>>;
    fn design_search(&self, query: &DesignSearchQuery) -> StorageResult<DesignSearchResult>;
    fn mockups(&self, pending_only: bool) -> StorageResult<Vec<MockupSummary>>;
    fn mockup(&self, external_id: &str) -> StorageResult<UiMockup>;
    fn tasks(&self, states: &[TaskState]) -> StorageResult<Vec<Task>>;
    fn task(&self, id: i64) -> StorageResult<Task>;
    fn task_page(&self, query: &TaskQuery) -> StorageResult<TaskPage>;
    fn qa_jobs(&self, query: &QaJobQuery) -> StorageResult<Vec<QaJob>>;
    fn qa_job(&self, id: i64) -> StorageResult<QaJob>;
    fn qa_job_summaries(&self, query: &QaJobQuery) -> StorageResult<Vec<QaJobSummary>>;
    fn qa_runs(&self, limit: usize) -> StorageResult<Vec<QaRun>>;
    fn qa_run(&self, id: i64) -> StorageResult<QaRun>;
    fn qa_run_summaries(&self, limit: usize) -> StorageResult<Vec<QaRunSummary>>;
    fn memory(&self) -> StorageResult<ProjectMemory>;
    /// Includes resolved notes with their provenance; retention remains bounded.
    fn retained_memory_notes(&self) -> StorageResult<Vec<MemoryNote>>;
    fn rules(&self) -> StorageResult<Vec<Rule>>;
    fn fixed_prompts(&self) -> StorageResult<Vec<FixedHookPrompt>>;
    fn resource_versions(&self, resources: &[ResourceKey]) -> StorageResult<Vec<ResourceVersion>>;
    fn receipt(&self, operation_id: &str) -> StorageResult<Option<OperationReceipt>>;
    fn live_intents(&self) -> StorageResult<Vec<ResourceIntent>>;
    fn search(&self, query: &GrepParams) -> StorageResult<GrepResult>;
    fn legacy_content(&self) -> StorageResult<LegacyContent>;
    fn health_waivers(&self, external_id: &str) -> StorageResult<Vec<health::HealthWaiver>>;
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceKey {
    pub kind: String,
    pub id: String,
}

/// A notification invalidates caches. Cursors are opaque, project/backend scoped,
/// and never a write precondition. Polling may coalesce multiple commits.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ChangeNotification {
    Unchanged {
        cursor: ChangeCursor,
    },
    Changed {
        cursor: ChangeCursor,
    },
    /// Unknown/expired/foreign cursor: caller must retrieve a fresh snapshot.
    Reset {
        cursor: ChangeCursor,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IntentUpdate {
    pub agent_run_id: String,
    pub resources: Vec<ResourceKey>,
    /// Zero releases these intents. Intents are advisory, never write locks.
    pub ttl_seconds: u32,
}

/// COMPLETE backend implementation contract. Commit must do receipt lookup,
/// guards, reference/cascade validation, ALL domain writes, version/cursor
/// changes and receipt persistence in one atomic transaction. Look up the
/// receipt BEFORE guards (an identical retry may have obsolete guards).
///
/// Resource conflicts abort the whole batch; disjoint writes may commit.
/// Failed commits publish nothing. No-ops retain versions/cursor but store a
/// receipt. Return CommitUncertain only when durability cannot be determined.
/// Backend failures must not expose credentials or SQL/driver details.
pub trait StorageBackend {
    fn snapshot(&mut self) -> StorageResult<Box<dyn ReadSnapshot + '_>>;
    fn commit(&mut self, mutation: PreparedMutation) -> StorageResult<CommitResult>;
    fn poll_changes(&mut self, after: &ChangeCursor) -> StorageResult<ChangeNotification>;
    fn publish_intents(&mut self, update: &IntentUpdate) -> StorageResult<Vec<ResourceIntent>>;
    /// Releases backend resources. Idempotent; later operations return Closed.
    fn close(&mut self) -> StorageResult<()>;
}

/// Client-facing API shared by desktop and MCP. ProjectStore selects a complete
/// backend and delegates through StorageClient.
pub trait ProjectStorage {
    fn snapshot(&mut self) -> StorageResult<Box<dyn ReadSnapshot + '_>>;
    fn commit(&mut self, mutation: Mutation) -> StorageResult<CommitResult>;
    fn poll_changes(&mut self, after: &ChangeCursor) -> StorageResult<ChangeNotification>;
    fn publish_intents(&mut self, update: &IntentUpdate) -> StorageResult<Vec<ResourceIntent>>;
    fn close(&mut self) -> StorageResult<()>;
}

/// A complete facade can only be constructed with a complete backend.
pub struct StorageClient<B: StorageBackend> {
    backend: B,
}
impl<B: StorageBackend> StorageClient<B> {
    pub fn new(backend: B) -> Self {
        Self { backend }
    }
}
impl<B: StorageBackend> ProjectStorage for StorageClient<B> {
    fn snapshot(&mut self) -> StorageResult<Box<dyn ReadSnapshot + '_>> {
        self.backend.snapshot()
    }
    fn commit(&mut self, mutation: Mutation) -> StorageResult<CommitResult> {
        self.backend.commit(prepare_mutation(mutation)?)
    }
    fn poll_changes(&mut self, after: &ChangeCursor) -> StorageResult<ChangeNotification> {
        self.backend.poll_changes(after)
    }
    fn publish_intents(&mut self, update: &IntentUpdate) -> StorageResult<Vec<ResourceIntent>> {
        validate_intent(update)?;
        self.backend.publish_intents(update)
    }
    fn close(&mut self) -> StorageResult<()> {
        self.backend.close()
    }
}

impl StorageBackend for Box<dyn StorageBackend> {
    fn snapshot(&mut self) -> StorageResult<Box<dyn ReadSnapshot + '_>> {
        self.as_mut().snapshot()
    }
    fn commit(&mut self, mutation: PreparedMutation) -> StorageResult<CommitResult> {
        self.as_mut().commit(mutation)
    }
    fn poll_changes(&mut self, after: &ChangeCursor) -> StorageResult<ChangeNotification> {
        self.as_mut().poll_changes(after)
    }
    fn publish_intents(&mut self, update: &IntentUpdate) -> StorageResult<Vec<ResourceIntent>> {
        self.as_mut().publish_intents(update)
    }
    fn close(&mut self) -> StorageResult<()> {
        self.as_mut().close()
    }
}
