use super::*;
use super::{design, fixed_hooks, health as design_health, memory, mockups, qa, rules, tasks};
use crate::grep;
use adashi_storage_api::{self as api, *};
use api::{
    coordination::*, design::*, documents::*, fixed_hooks::*, health::*, memory::*, mockups::*,
    qa::*, rules::*, search::*, tasks::*,
};

/// The read transaction is pinned by the metadata query before this is returned.
/// WAL allows independent writer handles to commit without changing this view.
pub(super) struct Snapshot<'a> {
    pub(super) tx: rusqlite::Transaction<'a>,
    pub(super) metadata: ProjectMetadata,
}

pub(crate) fn metadata(db: &Connection, project_id: i64) -> StorageResult<ProjectMetadata> {
    let (id, name) = db
        .query_row(
            "SELECT slug,name FROM projects WHERE id=?1",
            [project_id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .map_err(StorageError::backend)?;
    let state = state::load_project_revision(db, project_id).map_err(StorageError::backend)?;
    let cursor = if db
        .table_exists(Some("temp"), "adashi_text_context")
        .map_err(StorageError::backend)?
    {
        db.query_row("SELECT cursor FROM temp.adashi_text_context", [], |r| {
            r.get::<_, String>(0)
        })
        .map_err(StorageError::backend)?
    } else {
        format!("sqlite:{id}:{}", state.revision)
    };
    Ok(ProjectMetadata {
        cursor: ChangeCursor::from_token(cursor),
        identity: ProjectIdentity { id, name },
        record_id: project_id,
        schema_version: db
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(StorageError::backend)?,
        revision: state.revision,
        updated_at: state.updated_at,
    })
}

pub(crate) fn receipt(
    db: &Connection,
    project_id: i64,
    operation_id: &str,
) -> StorageResult<Option<OperationReceipt>> {
    let payload = concurrency::load_operation::<serde_json::Value>(db, project_id, operation_id)
        .map_err(StorageError::backend)?;
    Ok(payload.map(|payload| {
        serde_json::from_value(payload.clone()).unwrap_or(OperationReceipt::Legacy { payload })
    }))
}

pub(crate) fn all_versions(
    db: &Connection,
    project_id: i64,
) -> StorageResult<Vec<ResourceVersion>> {
    let mut stmt = db.prepare("SELECT resource_kind,resource_id,version FROM resource_versions WHERE project_id=?1 ORDER BY resource_kind,resource_id").map_err(StorageError::backend)?;
    let rows = stmt
        .query_map([project_id], |r| {
            Ok(ResourceVersion {
                resource_kind: r.get(0)?,
                resource_id: r.get(1)?,
                version: r.get(2)?,
            })
        })
        .map_err(StorageError::backend)?;
    rows.collect::<rusqlite::Result<_>>()
        .map_err(StorageError::backend)
}

impl ReadSnapshot for Snapshot<'_> {
    fn metadata(&self) -> &ProjectMetadata {
        &self.metadata
    }
    fn computer_checkouts(&self) -> StorageResult<Vec<ComputerCheckout>> {
        let mut stmt = self.tx.prepare("SELECT computer_id,repository_path FROM project_computers WHERE project_id=?1 ORDER BY computer_id").map_err(StorageError::backend)?;
        let rows = stmt
            .query_map([self.metadata.record_id], |r| {
                Ok(ComputerCheckout {
                    computer_id: r.get(0)?,
                    repository_path: r.get(1)?,
                })
            })
            .map_err(StorageError::backend)?;
        rows.collect::<rusqlite::Result<_>>()
            .map_err(StorageError::backend)
    }
    fn design_inventory(&self) -> StorageResult<DesignInventory> {
        let workspace = self.tx.query_row("SELECT id,name,description,structurizr_dsl,structurizr_json FROM design_workspaces WHERE project_id=?1", [self.metadata.record_id], |r| Ok(Workspace { id:r.get(0)?, name:r.get(1)?, description:r.get(2)?, structurizr_dsl:r.get(3)?, structurizr_json:r.get(4)? })).map_err(StorageError::backend)?;
        let content = design::load_content(&self.tx).map_err(StorageError::backend)?;
        let identify = |table: &str, field: &str, key: &str| -> StorageResult<i64> {
            self.tx
                .query_row(
                    &format!("SELECT id FROM {table} WHERE workspace_id=?1 AND {field}=?2"),
                    params![workspace.id, key],
                    |r| r.get(0),
                )
                .map_err(StorageError::backend)
        };
        let elements = content
            .elements
            .into_iter()
            .map(|value| {
                Ok(Identified {
                    id: identify("c4_elements", "external_id", &value.external_id)?,
                    value,
                })
            })
            .collect::<StorageResult<_>>()?;
        let relationships = content
            .relationships
            .into_iter()
            .map(|value| {
                Ok(Identified {
                    id: identify("c4_relationships", "external_id", &value.external_id)?,
                    value,
                })
            })
            .collect::<StorageResult<_>>()?;
        let diagrams = content
            .diagrams
            .into_iter()
            .map(|value| {
                Ok(Identified {
                    id: identify("diagrams", "key", &value.key)?,
                    value,
                })
            })
            .collect::<StorageResult<_>>()?;
        Ok(DesignInventory {
            workspace,
            elements,
            relationships,
            diagrams,
            bindings: content.bindings,
        })
    }
    fn design_overview(&self, depth: Option<usize>) -> StorageResult<DesignOverviewResult> {
        design::load_overview(&self.tx, self.metadata.record_id, depth)
            .map_err(StorageError::backend)
    }
    fn design_scope(&self, q: &ScopeQuery) -> StorageResult<DesignScopeResult> {
        design::load_scope(
            &self.tx,
            self.metadata.record_id,
            &q.element_id,
            q.include_ancestors,
            q.children_depth,
            true,
        )
        .map_err(StorageError::backend)
    }
    fn design_by_ids(&self, ids: &[String]) -> StorageResult<DesignByIdsResult> {
        design::load_by_ids(&self.tx, self.metadata.record_id, ids).map_err(StorageError::backend)
    }
    fn design_bindings(&self, q: &BindingQuery) -> StorageResult<DesignBindingsResult> {
        design::load_by_bindings(&self.tx, self.metadata.record_id, &q.files, &q.symbols)
            .map_err(StorageError::backend)
    }
    fn design_documents(&self, ids: &[String]) -> StorageResult<Vec<DesignDocument>> {
        design::documents::load_documents(&self.tx, self.metadata.record_id, ids)
            .map_err(StorageError::backend)
    }
    fn design_search(&self, q: &DesignSearchQuery) -> StorageResult<DesignSearchResult> {
        design::search(
            &self.tx,
            self.metadata.record_id,
            &q.query,
            &q.kinds,
            q.limit,
        )
        .map_err(StorageError::backend)
    }
    fn mockups(&self, pending: bool) -> StorageResult<Vec<MockupSummary>> {
        if pending {
            mockups::load_pending(&self.tx, self.metadata.record_id)
        } else {
            mockups::load_summaries(&self.tx, self.metadata.record_id)
        }
        .map_err(StorageError::backend)
    }
    fn mockup(&self, id: &str) -> StorageResult<UiMockup> {
        mockups::load_mockup(&self.tx, self.metadata.record_id, id).map_err(StorageError::backend)
    }
    fn tasks(&self, states: &[TaskState]) -> StorageResult<Vec<Task>> {
        tasks::load_tasks(&self.tx, self.metadata.record_id, states).map_err(StorageError::backend)
    }
    fn task(&self, id: i64) -> StorageResult<Task> {
        tasks::load_task(&self.tx, self.metadata.record_id, id).map_err(StorageError::backend)
    }
    fn task_page(&self, q: &TaskQuery) -> StorageResult<TaskPage> {
        let limit = u32::try_from(q.limit)
            .map_err(|_| StorageError::Validation("Task page limit is too large".into()))?;
        if limit == 0 {
            return Err(StorageError::Validation(
                "Task page limit must be positive".into(),
            ));
        }
        let (mut tasks, total) = tasks::load_task_summaries(
            &self.tx,
            self.metadata.record_id,
            &q.states,
            q.after_id.unwrap_or(0),
            limit.saturating_add(1),
        )
        .map_err(StorageError::backend)?;
        let more = tasks.len() > q.limit;
        tasks.truncate(q.limit);
        let next_after_id = if more {
            tasks.last().map(|t| t.id)
        } else {
            None
        };
        Ok(TaskPage {
            tasks,
            total,
            next_after_id,
            closed_count: tasks::count_closed_tasks(&self.tx, self.metadata.record_id)
                .map_err(StorageError::backend)?,
        })
    }
    fn qa_jobs(&self, q: &QaJobQuery) -> StorageResult<Vec<QaJob>> {
        qa::load_jobs(&self.tx, self.metadata.record_id, Some(q)).map_err(StorageError::backend)
    }
    fn qa_job(&self, id: i64) -> StorageResult<QaJob> {
        qa::load_job(&self.tx, self.metadata.record_id, id).map_err(StorageError::backend)
    }
    fn qa_job_summaries(&self, q: &QaJobQuery) -> StorageResult<Vec<QaJobSummary>> {
        qa::load_job_summaries(&self.tx, self.metadata.record_id, Some(q), None, i64::MAX)
            .map(|v| v.0)
            .map_err(StorageError::backend)
    }
    fn qa_runs(&self, limit: usize) -> StorageResult<Vec<QaRun>> {
        qa::load_runs(
            &self.tx,
            self.metadata.record_id,
            Some(limit.min(100) as i64),
        )
        .map_err(StorageError::backend)
    }
    fn qa_run(&self, id: i64) -> StorageResult<QaRun> {
        qa::load_run(&self.tx, self.metadata.record_id, id).map_err(StorageError::backend)
    }
    fn qa_run_summaries(&self, limit: usize) -> StorageResult<Vec<QaRunSummary>> {
        qa::load_run_summaries(
            &self.tx,
            self.metadata.record_id,
            Some(limit.min(100) as i64),
        )
        .map_err(StorageError::backend)
    }
    fn memory(&self) -> StorageResult<ProjectMemory> {
        memory::load_memory(&self.tx, self.metadata.record_id).map_err(StorageError::backend)
    }
    fn retained_memory_notes(&self) -> StorageResult<Vec<MemoryNote>> {
        memory::load_retained_notes(&self.tx, self.metadata.record_id)
            .map_err(StorageError::backend)
    }
    fn rules(&self) -> StorageResult<Vec<Rule>> {
        rules::load_rules(&self.tx).map_err(StorageError::backend)
    }
    fn fixed_prompts(&self) -> StorageResult<Vec<FixedHookPrompt>> {
        fixed_hooks::load_fixed_hook_prompts(&self.tx, self.metadata.record_id)
            .map_err(StorageError::backend)
    }
    fn resource_versions(&self, resources: &[ResourceKey]) -> StorageResult<Vec<ResourceVersion>> {
        resources
            .iter()
            .map(|key| {
                Ok(ResourceVersion {
                    resource_kind: key.kind.clone(),
                    resource_id: key.id.clone(),
                    version: concurrency::load_version(
                        &self.tx,
                        self.metadata.record_id,
                        &key.kind,
                        &key.id,
                    )
                    .map_err(StorageError::backend)?,
                })
            })
            .collect()
    }
    fn receipt(&self, id: &str) -> StorageResult<Option<OperationReceipt>> {
        receipt(&self.tx, self.metadata.record_id, id)
    }
    fn live_intents(&self) -> StorageResult<Vec<ResourceIntent>> {
        concurrency::load_live_intents(&self.tx, self.metadata.record_id)
            .map_err(StorageError::backend)
    }
    fn search(&self, q: &GrepParams) -> StorageResult<GrepResult> {
        grep::search(self, q).map_err(StorageError::Validation)
    }
    fn legacy_content(&self) -> StorageResult<LegacyContent> {
        legacy_content(&self.tx, self.metadata.record_id)
    }
    fn health_waivers(&self, id: &str) -> StorageResult<Vec<HealthWaiver>> {
        design_health::load_waivers(&self.tx, self.metadata.record_id, id)
            .map_err(StorageError::backend)
    }
}

pub(super) fn legacy_content(db: &Connection, project_id: i64) -> StorageResult<LegacyContent> {
    fn read<T>(
        db: &Connection,
        sql: &str,
        project_id: i64,
        map: impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
    ) -> StorageResult<Vec<T>> {
        let mut stmt = db.prepare(sql).map_err(StorageError::backend)?;
        let rows = stmt
            .query_map([project_id], map)
            .map_err(StorageError::backend)?;
        rows.collect::<rusqlite::Result<_>>()
            .map_err(StorageError::backend)
    }
    Ok(LegacyContent {
        guidelines:read(db,"SELECT id,title,body,severity,created_at FROM coding_guidelines WHERE project_id=?1 ORDER BY id",project_id,|r| Ok(Guideline{id:r.get(0)?,title:r.get(1)?,body:r.get(2)?,severity:r.get(3)?,created_at:r.get(4)?}))?,
        post_task_commands:read(db,"SELECT id,label,command,trigger,created_at FROM post_task_commands WHERE project_id=?1 ORDER BY id",project_id,|r| Ok(PostTaskCommand{id:r.get(0)?,label:r.get(1)?,command:r.get(2)?,trigger:r.get(3)?,created_at:r.get(4)?}))?,
        qa_checks:read(db,"SELECT id,label,command,required,created_at FROM qa_checks WHERE project_id=?1 ORDER BY id",project_id,|r| Ok(QaCheck{id:r.get(0)?,label:r.get(1)?,command:r.get(2)?,required:r.get(3)?,created_at:r.get(4)?}))?,
        task_qa_entries:read(db,"SELECT q.id,q.task_id,q.label,q.status,q.body,q.created_at FROM task_qa_entries q JOIN agent_tasks t ON t.id=q.task_id WHERE t.project_id=?1 ORDER BY q.id",project_id,|r| Ok(TaskQaEntry{id:r.get(0)?,task_id:r.get(1)?,label:r.get(2)?,status:r.get(3)?,body:r.get(4)?,created_at:r.get(5)?}))?,
    })
}
