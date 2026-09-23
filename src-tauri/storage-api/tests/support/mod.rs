//! Scripted test double: domain replies are fixtures; protocol transitions use
//! the real shared preparation, guard, reference and receipt machinery.
//! This is not a production in-memory adapter or a claim to test SQLite semantics.
use adashi_storage_api::{
    coordination::*, design::*, documents::*, fixed_hooks::*, memory::*, mockups::*, qa::*,
    rules::*, search::*, tasks::*, *,
};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct Frame {
    pub metadata: ProjectMetadata,
    pub answers: BTreeMap<String, Value>,
    pub versions: Vec<ResourceVersion>,
    pub documents: Vec<DesignDocument>,
    pub resources: BTreeSet<ResourceKey>,
}
impl Frame {
    fn reply<T: DeserializeOwned>(&self, method: &str, _query: Value) -> StorageResult<T> {
        serde_json::from_value(
            self.answers
                .get(method)
                .cloned()
                .ok_or_else(|| StorageError::Backend(format!("Missing test reply: {method}")))?,
        )
        .map_err(StorageError::backend)
    }
}
impl ReadSnapshot for Frame {
    fn metadata(&self) -> &ProjectMetadata {
        &self.metadata
    }
    fn computer_checkouts(&self) -> StorageResult<Vec<ComputerCheckout>> {
        self.reply("computer_checkouts", json!([]))
    }
    fn design_inventory(&self) -> StorageResult<DesignInventory> {
        self.reply("design_inventory", json!([]))
    }
    fn design_overview(&self, max_depth: Option<usize>) -> StorageResult<DesignOverviewResult> {
        self.reply("design_overview", json!([max_depth]))
    }
    fn design_scope(&self, query: &ScopeQuery) -> StorageResult<DesignScopeResult> {
        self.reply("design_scope", json!([query]))
    }
    fn design_by_ids(&self, ids: &[String]) -> StorageResult<DesignByIdsResult> {
        self.reply("design_by_ids", json!([ids]))
    }
    fn design_bindings(&self, query: &BindingQuery) -> StorageResult<DesignBindingsResult> {
        self.reply("design_bindings", json!([query]))
    }
    fn design_documents(&self, ids: &[String]) -> StorageResult<Vec<DesignDocument>> {
        Ok(self
            .documents
            .iter()
            .filter(|v| ids.contains(&v.document_id))
            .cloned()
            .collect())
    }
    fn design_search(&self, query: &DesignSearchQuery) -> StorageResult<DesignSearchResult> {
        self.reply("design_search", json!([query]))
    }
    fn mockups(&self, pending_only: bool) -> StorageResult<Vec<MockupSummary>> {
        self.reply("mockups", json!([pending_only]))
    }
    fn mockup(&self, external_id: &str) -> StorageResult<UiMockup> {
        self.reply("mockup", json!([external_id]))
    }
    fn tasks(&self, states: &[TaskState]) -> StorageResult<Vec<Task>> {
        self.reply("tasks", json!([states]))
    }
    fn task(&self, id: i64) -> StorageResult<Task> {
        self.reply("task", json!([id]))
    }
    fn task_page(&self, query: &TaskQuery) -> StorageResult<TaskPage> {
        self.reply("task_page", json!([query]))
    }
    fn qa_jobs(&self, query: &QaJobQuery) -> StorageResult<Vec<QaJob>> {
        self.reply("qa_jobs", json!([query]))
    }
    fn qa_job(&self, id: i64) -> StorageResult<QaJob> {
        self.reply("qa_job", json!([id]))
    }
    fn qa_job_summaries(&self, query: &QaJobQuery) -> StorageResult<Vec<QaJobSummary>> {
        self.reply("qa_job_summaries", json!([query]))
    }
    fn qa_runs(&self, limit: usize) -> StorageResult<Vec<QaRun>> {
        self.reply("qa_runs", json!([limit]))
    }
    fn qa_run(&self, id: i64) -> StorageResult<QaRun> {
        self.reply("qa_run", json!([id]))
    }
    fn qa_run_summaries(&self, limit: usize) -> StorageResult<Vec<QaRunSummary>> {
        self.reply("qa_run_summaries", json!([limit]))
    }
    fn memory(&self) -> StorageResult<ProjectMemory> {
        self.reply("memory", json!([]))
    }
    fn retained_memory_notes(&self) -> StorageResult<Vec<MemoryNote>> {
        self.reply("retained_memory_notes", json!([]))
    }
    fn rules(&self) -> StorageResult<Vec<Rule>> {
        self.reply("rules", json!([]))
    }
    fn fixed_prompts(&self) -> StorageResult<Vec<FixedHookPrompt>> {
        self.reply("fixed_prompts", json!([]))
    }
    fn resource_versions(&self, resources: &[ResourceKey]) -> StorageResult<Vec<ResourceVersion>> {
        Ok(self
            .versions
            .iter()
            .filter(|v| {
                resources
                    .iter()
                    .any(|r| r.kind == v.resource_kind && r.id == v.resource_id)
            })
            .cloned()
            .collect())
    }
    fn receipt(&self, operation_id: &str) -> StorageResult<Option<OperationReceipt>> {
        let value = self
            .answers
            .get("receipts")
            .and_then(|v| v.get(operation_id))
            .cloned()
            .unwrap_or(Value::Null);
        serde_json::from_value(value).map_err(StorageError::backend)
    }
    fn live_intents(&self) -> StorageResult<Vec<ResourceIntent>> {
        self.reply("live_intents", json!([]))
    }
    fn search(&self, query: &GrepParams) -> StorageResult<GrepResult> {
        self.reply("search", json!([query]))
    }
    fn legacy_content(&self) -> StorageResult<LegacyContent> {
        self.reply("legacy_content", json!([]))
    }
    fn health_waivers(&self, external_id: &str) -> StorageResult<Vec<health::HealthWaiver>> {
        self.reply("health_waivers", json!([external_id]))
    }
}
pub struct Step {
    pub request: Mutation,
    pub after: Frame,
    pub result: CommitResult,
    pub edges: Vec<MissingReference>,
    pub fail_before_commit: bool,
}
pub struct State {
    pub frame: Frame,
    pub steps: VecDeque<Step>,
    pub receipts: BTreeMap<String, OperationReceipt>,
    pub intents: Vec<ResourceIntent>,
}
pub struct FakeBackend {
    pub shared: Arc<Mutex<State>>,
    pub closed: bool,
}
impl FakeBackend {
    fn ensure_open(&self) -> StorageResult<()> {
        if self.closed {
            Err(StorageError::Closed)
        } else {
            Ok(())
        }
    }
}
impl StorageBackend for FakeBackend {
    fn snapshot(&mut self) -> StorageResult<Box<dyn ReadSnapshot + '_>> {
        self.ensure_open()?;
        let state = self.shared.lock().map_err(StorageError::backend)?;
        let mut frame = state.frame.clone();
        frame
            .answers
            .insert("receipts".into(), json!(state.receipts));
        frame
            .answers
            .insert("live_intents".into(), json!(state.intents));
        Ok(Box::new(frame))
    }
    fn commit(&mut self, prepared: PreparedMutation) -> StorageResult<CommitResult> {
        self.ensure_open()?;
        let mut state = self.shared.lock().map_err(StorageError::backend)?;
        if let Some(result) =
            prepared.replay(state.receipts.get(&prepared.mutation().operation_id))?
        {
            return Ok(result);
        }
        check_versions(&prepared.expected_versions(), &state.frame.versions)?;
        for change in &prepared.mutation().changes {
            if let Change::Design(
                DesignWrite::Save { read_tokens, .. } | DesignWrite::Describe { read_tokens, .. },
            ) = change
            {
                check_document_tokens(read_tokens, &state.frame.documents)?;
            }
        }
        let step = state
            .steps
            .front()
            .ok_or_else(|| StorageError::Backend("Unexpected test commit".into()))?;
        if prepared.fingerprint() != prepare_mutation(step.request.clone())?.fingerprint() {
            return Err(StorageError::Backend(
                "Commit did not match scripted request".into(),
            ));
        }
        check_references(&step.edges, &step.after.resources)?;
        if step.fail_before_commit {
            return Err(StorageError::Backend("Injected persistence failure".into()));
        }
        let step = state
            .steps
            .pop_front()
            .ok_or_else(|| StorageError::Backend("Missing test step".into()))?;
        let receipt = prepared.receipt(step.result.clone());
        state.frame = step.after;
        state
            .receipts
            .insert(prepared.mutation().operation_id.clone(), receipt);
        Ok(step.result)
    }
    fn poll_changes(&mut self, after: &ChangeCursor) -> StorageResult<ChangeNotification> {
        self.ensure_open()?;
        let cursor = self
            .shared
            .lock()
            .map_err(StorageError::backend)?
            .frame
            .metadata
            .cursor
            .clone();
        Ok(if &cursor == after {
            ChangeNotification::Unchanged { cursor }
        } else {
            ChangeNotification::Changed { cursor }
        })
    }
    fn publish_intents(&mut self, update: &IntentUpdate) -> StorageResult<Vec<ResourceIntent>> {
        self.ensure_open()?;
        validate_intent(update)?;
        let mut state = self.shared.lock().map_err(StorageError::backend)?;
        for key in &update.resources {
            state.intents.retain(|i| {
                !(i.agent_run_id == update.agent_run_id
                    && i.resource_kind == key.kind
                    && i.resource_id == key.id)
            });
            if update.ttl_seconds > 0 {
                state.intents.push(ResourceIntent {
                    agent_run_id: update.agent_run_id.clone(),
                    resource_kind: key.kind.clone(),
                    resource_id: key.id.clone(),
                    expires_at: "fixture lease".into(),
                });
            }
        }
        Ok(state.intents.clone())
    }
    fn close(&mut self) -> StorageResult<()> {
        self.closed = true;
        Ok(())
    }
}
