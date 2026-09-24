use super::*;
pub(crate) mod conformance;
use adashi_storage_api::{
    qa::QaJobQuery,
    rules::NewRule,
    tasks::{NewTask, TaskState, UpdateTask},
    *,
};

pub(crate) fn fixture() -> (
    tempfile::TempDir,
    OpenRequest,
    StorageClient<Box<dyn StorageBackend>>,
) {
    let root = tempfile::tempdir().unwrap();
    let request = OpenRequest {
        location: root
            .path()
            .join(".adashi/adashi.sqlite3")
            .to_string_lossy()
            .into_owned(),
        registered_identity: ProjectIdentity {
            id: "complete-fixture".into(),
            name: "Complete fixture".into(),
        },
        computer_id: "computer-a".into(),
        checkout_path: root.path().to_string_lossy().into_owned(),
        mode: OpenMode::InitializeOrMigrate,
        cursor_scope: None,
    };
    let client = StorageClient::new(SqliteFactory.open(&request).unwrap());
    (root, request, client)
}
fn new_task() -> Change {
    Change::Task(TaskWrite::Create {
        input: NewTask {
            title: "Atomic task".into(),
            description: None,
            design_specification_links: None,
        },
    })
}
fn new_rule() -> Change {
    Change::Rule(RuleWrite::Create {
        input: NewRule {
            name: "Atomic rule".into(),
            enabled: true,
            intend: "general".into(),
            hook: "run.start".into(),
            prompt: "Keep context".into(),
        },
    })
}
fn mutation(id: &str, changes: Vec<Change>) -> Mutation {
    Mutation {
        operation_id: id.into(),
        changes,
    }
}

#[test]
fn full_adapter_reads_and_cross_domain_replay() {
    let (_root, _request, mut store) = fixture();
    let before = store.snapshot().unwrap().metadata().revision;
    let request = mutation("create", vec![new_task(), new_rule()]);
    let result = store.commit(request.clone()).unwrap();
    assert_eq!(result.revision, before + 1);
    assert_eq!(result.outcomes.len(), 2);
    let replay = store.commit(request).unwrap();
    assert_eq!(
        serde_json::to_value(result).unwrap(),
        serde_json::to_value(replay).unwrap()
    );
    assert!(matches!(
        store.commit(mutation("create", vec![new_task()])),
        Err(StorageError::OperationReused)
    ));
    let s = store.snapshot().unwrap();
    let model = s.design_inventory().unwrap();
    s.design_overview(None).unwrap();
    s.design_by_ids(&[]).unwrap();
    s.design_bindings(&BindingQuery {
        files: vec![],
        symbols: vec![],
    })
    .unwrap();
    s.design_documents(&[]).unwrap();
    s.design_search(&DesignSearchQuery {
        query: "fixture".into(),
        kinds: vec![],
        limit: 10,
    })
    .unwrap();
    if let Some(element) = model.elements.first() {
        s.design_scope(&ScopeQuery {
            element_id: element.value.external_id.clone(),
            children_depth: None,
            include_ancestors: true,
        })
        .unwrap();
    }
    assert_eq!(s.tasks(&[TaskState::Todo]).unwrap().len(), 1);
    let page = s
        .task_page(&TaskQuery {
            states: vec![TaskState::Todo],
            after_id: None,
            limit: 10,
        })
        .unwrap();
    s.task(page.tasks[0].id).unwrap();
    s.qa_jobs(&QaJobQuery::default()).unwrap();
    s.qa_job_summaries(&QaJobQuery::default()).unwrap();
    s.qa_runs(20).unwrap();
    s.qa_run_summaries(20).unwrap();
    s.memory().unwrap();
    s.retained_memory_notes().unwrap();
    s.rules().unwrap();
    s.fixed_prompts().unwrap();
    s.computer_checkouts().unwrap();
    s.legacy_content().unwrap();
    s.mockups(false).unwrap();
    s.live_intents().unwrap();
    s.health_waivers("missing").unwrap();
    s.resource_versions(&[]).unwrap();
    assert!(s.receipt("create").unwrap().is_some());
    let query = serde_json::from_value(serde_json::json!({"projectName":"fixture"})).unwrap();
    s.search(&query).unwrap();
}

#[test]
fn pinned_snapshot_survives_an_independent_writer_and_failure_is_atomic() {
    let (_root, mut request, mut first) = fixture();
    request.mode = OpenMode::ReadWrite;
    let mut second = StorageClient::new(SqliteFactory.open(&request).unwrap());
    let snapshot = first.snapshot().unwrap();
    let count = snapshot.tasks(&[TaskState::Todo]).unwrap().len();
    second.commit(mutation("other", vec![new_task()])).unwrap();
    assert_eq!(snapshot.tasks(&[TaskState::Todo]).unwrap().len(), count);
    drop(snapshot);
    assert_eq!(
        first
            .snapshot()
            .unwrap()
            .tasks(&[TaskState::Todo])
            .unwrap()
            .len(),
        count + 1
    );
    let before = first.snapshot().unwrap().metadata().revision;
    let bad = Change::Task(TaskWrite::Update {
        expected_version: 0,
        input: UpdateTask {
            task_id: 9999,
            title: Some("Missing".into()),
            description: None,
            state: None,
            design_specification_links: None,
        },
    });
    assert!(first
        .commit(mutation("rollback", vec![new_rule(), bad]))
        .is_err());
    let s = first.snapshot().unwrap();
    assert_eq!(s.metadata().revision, before);
    assert!(!s.rules().unwrap().iter().any(|r| r.name == "Atomic rule"));
    assert!(s.receipt("rollback").unwrap().is_none());
    drop(s);
    first.close().unwrap();
    first.close().unwrap();
    assert!(matches!(first.snapshot(), Err(StorageError::Closed)));
    second.snapshot().unwrap();
}
