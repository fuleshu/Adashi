mod support;
use adashi_storage_api::{coordination::*, documents::*, qa::*, rules::*, search::*, tasks::*, *};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex};
use support::*;

fn frame() -> Frame {
    let task = json!({"id":12,"version":1,"number":7,"title":"Implement storage","description":"Typed API",
        "state":"active","designSpecificationLinks":[],"createdAt":"created","updatedAt":"updated",
        "completedAt":null,"confirmedAt":null,"completionMemo":"","createdFiles":[],"changedFiles":[],
        "confirmationCommitId":null});
    let rule = json!({"id":3,"version":1,"name":"Review","enabled":true,"intend":"implementation","hook":"task.end","prompt":"Review"});
    let manifest = json!({"schemaVersion":1,"key":"screen","attachedToExternalId":"core","viewportWidth":800,"viewportHeight":600,"screen":"main","state":"","fidelity":""});
    let mockup = json!({"id":4,"externalId":"screen","title":"Screen","manifest":manifest,
        "acceptedSvg":"<svg/>","acceptedRevision":1,"acceptedVersion":1,"workingVersion":1,
        "workingSvg":null,"baseRevision":null,"status":"accepted","editOperations":[],"annotations":[],
        "proposal":null,"createdAt":"created","updatedAt":"updated"});
    let job = json!({"id":5,"version":1,"number":1,"name":"Tests","description":"","command":"cargo test",
        "workingDirectory":"","shell":"powershell","timeoutSeconds":120,"enabled":true,"createdBy":"user",
        "createdAt":"created","updatedAt":"updated","derivedState":"not_run","designSpecificationLinks":[],
        "taskLinks":[],"tags":[],"latestRun":null,"runHistory":[]});
    let run = json!({"id":9,"triggerSource":"user","querySnapshot":"{}","status":"running","startedAt":"started","finishedAt":null,"summary":"","jobRuns":[]});
    let memory = json!({"limits":{"totalChars":12000,"summaryChars":4000,"noteChars":1000,"notes":20},
        "rule":"","memory":"Shared identity","memoryVersion":1,"protocolVersion":1,"notes":[],"updatedAt":"updated"});
    let mut answers = BTreeMap::new();
    for method in [
        "computer_checkouts",
        "mockups",
        "qa_job_summaries",
        "qa_run_summaries",
        "retained_memory_notes",
        "fixed_prompts",
        "live_intents",
        "health_waivers",
    ] {
        answers.insert(method.into(), json!([]));
    }
    for (key, value) in [
        ("task", task.clone()),
        ("tasks", json!([task])),
        ("rules", json!([rule])),
        ("mockup", mockup),
        ("qa_job", job.clone()),
        ("qa_jobs", json!([job])),
        ("qa_run", run.clone()),
        ("qa_runs", json!([run])),
        ("memory", memory),
        ("receipt", Value::Null),
        (
            "legacy_content",
            json!({"guidelines":[],"postTaskCommands":[],"qaChecks":[],"taskQaEntries":[]}),
        ),
        (
            "task_page",
            json!({"tasks":[{"id":12,"number":7,"title":"Implement storage","titleTruncated":false,"state":"active","version":1}],"nextAfterId":null,"total":1,"closedCount":0}),
        ),
        (
            "search",
            json!({"output":"task:12 Implement storage","total":1,"counts":{"design":0,"tasks":1,"memory":0},"shown":1,"truncated":false,"outputBytes":25,"patternTerms":1}),
        ),
        (
            "design_inventory",
            json!({"workspace":{"id":1,"name":"Architecture","description":"","structurizrDsl":"","structurizrJson":"{}"},"elements":[],"relationships":[],"diagrams":[],"bindings":[]}),
        ),
        (
            "design_overview",
            json!({"revision":1,"workspaceName":"Architecture","workspaceDescription":"","structurizrDsl":"","umlArtifactTypes":[],"elements":[],"relationships":[],"diagrams":[],"bindings":[],"mockups":[]}),
        ),
        (
            "design_scope",
            json!({"revision":1,"rootExternalId":"core","umlArtifactTypes":[],"ancestors":[],"elements":[],"relationships":[],"diagrams":[],"bindings":[],"mockups":[],"structurizrDsl":null}),
        ),
        (
            "design_by_ids",
            json!({"revision":1,"umlArtifactTypes":[],"elements":[],"relationships":[],"diagrams":[],"bindings":[],"mockups":[]}),
        ),
        (
            "design_bindings",
            json!({"revision":1,"umlArtifactTypes":[],"elements":[],"relationships":[],"diagrams":[],"bindings":[],"mockups":[]}),
        ),
        ("design_search", json!({"revision":1,"hits":[]})),
    ] {
        answers.insert(key.into(), value);
    }
    Frame {
        metadata: ProjectMetadata {
            identity: ProjectIdentity {
                id: "p".into(),
                name: "Project".into(),
            },
            record_id: 1,
            schema_version: 13,
            revision: 1,
            updated_at: "updated".into(),
            cursor: ChangeCursor::from_token("fake:1"),
        },
        answers,
        versions: vec![
            ResourceVersion {
                resource_kind: "task".into(),
                resource_id: "12".into(),
                version: 1,
            },
            ResourceVersion {
                resource_kind: "rule".into(),
                resource_id: "3".into(),
                version: 1,
            },
        ],
        documents: vec![],
        resources: BTreeSet::new(),
    }
}
fn setup() -> (StorageClient<FakeBackend>, Arc<Mutex<State>>) {
    let shared = Arc::new(Mutex::new(State {
        frame: frame(),
        steps: VecDeque::new(),
        receipts: BTreeMap::new(),
        intents: vec![],
    }));
    (client(shared.clone()), shared)
}
fn client(shared: Arc<Mutex<State>>) -> StorageClient<FakeBackend> {
    StorageClient::new(FakeBackend {
        shared,
        closed: false,
    })
}
fn mutation() -> Mutation {
    Mutation {
        operation_id: "batch".into(),
        changes: vec![
            Change::Task(TaskWrite::Update {
                expected_version: 1,
                input: UpdateTask {
                    task_id: 12,
                    title: Some("Updated task".into()),
                    description: None,
                    state: None,
                    design_specification_links: None,
                },
            }),
            Change::Rule(RuleWrite::Update {
                id: 3,
                expected_version: 1,
                input: NewRule {
                    name: "Review".into(),
                    enabled: true,
                    intend: "implementation".into(),
                    hook: "task.end".into(),
                    prompt: "Updated prompt".into(),
                },
            }),
        ],
    }
}
fn step(request: Mutation) -> Step {
    let mut after = frame();
    after.metadata.revision = 2;
    after.metadata.cursor = ChangeCursor::from_token("fake:2");
    after.answers.get_mut("task").unwrap()["title"] = json!("Updated task");
    after.answers.get_mut("task").unwrap()["version"] = json!(2);
    after.answers.get_mut("rules").unwrap()[0]["prompt"] = json!("Updated prompt");
    after.answers.get_mut("rules").unwrap()[0]["version"] = json!(2);
    for v in &mut after.versions {
        v.version = 2;
    }
    let result = CommitResult {
        cursor: after.metadata.cursor.clone(),
        revision: 2,
        changed: true,
        outcomes: vec![
            ChangeOutcome::Task(Some(
                serde_json::from_value(after.answers["task"].clone()).unwrap(),
            )),
            ChangeOutcome::Rule(Some(
                serde_json::from_value(after.answers["rules"][0].clone()).unwrap(),
            )),
        ],
        versions: after.versions.clone(),
        read_tokens: vec![],
    };
    Step {
        request,
        after,
        result,
        edges: vec![],
        fail_before_commit: false,
    }
}

/// Every required read method is implemented by the fake and callable through
/// the same object-safe facade used by desktop and MCP, with no driver dependency.
#[test]
fn all_domains_are_typed_and_consumable_without_sqlite() {
    let (mut client, _) = setup();
    let storage: &mut dyn ProjectStorage = &mut client;
    let s = storage.snapshot().unwrap();
    assert_eq!(s.metadata().identity.id, "p");
    s.computer_checkouts().unwrap();
    s.design_inventory().unwrap();
    s.design_overview(Some(2)).unwrap();
    s.design_scope(&ScopeQuery {
        element_id: "core".into(),
        children_depth: None,
        include_ancestors: true,
    })
    .unwrap();
    s.design_by_ids(&["core".into()]).unwrap();
    s.design_bindings(&BindingQuery {
        files: vec!["lib.rs".into()],
        symbols: vec![],
    })
    .unwrap();
    s.design_documents(&[]).unwrap();
    s.design_search(&DesignSearchQuery {
        query: "core".into(),
        kinds: vec![],
        limit: 25,
    })
    .unwrap();
    s.mockups(false).unwrap();
    assert_eq!(s.mockup("screen").unwrap().accepted_revision, 1);
    assert_eq!(s.tasks(&[TaskState::Active]).unwrap()[0].id, 12);
    assert_eq!(s.task(12).unwrap().title, "Implement storage");
    s.task_page(&TaskQuery {
        states: vec![TaskState::Active],
        after_id: None,
        limit: 25,
    })
    .unwrap();
    s.qa_jobs(&QaJobQuery::default()).unwrap();
    s.qa_job(5).unwrap();
    s.qa_job_summaries(&QaJobQuery::default()).unwrap();
    s.qa_runs(20).unwrap();
    s.qa_run(9).unwrap();
    s.qa_run_summaries(20).unwrap();
    s.memory().unwrap();
    s.retained_memory_notes().unwrap();
    s.rules().unwrap();
    s.fixed_prompts().unwrap();
    s.resource_versions(&[ResourceKey {
        kind: "task".into(),
        id: "12".into(),
    }])
    .unwrap();
    assert!(s.receipt("unknown").unwrap().is_none());
    s.live_intents().unwrap();
    s.search(&GrepParams {
        project_name: "p".into(),
        pattern: Some("storage".into()),
        r#in: None,
        file: None,
        r#type: None,
        state: None,
        limit: None,
    })
    .unwrap();
    s.legacy_content().unwrap();
    s.health_waivers("core").unwrap();
}

#[test]
fn atomic_cross_domain_commit_snapshot_isolation_and_replay() {
    let (mut reader, shared) = setup();
    shared.lock().unwrap().steps.push_back(step(mutation()));
    let mut writer = client(shared.clone());
    let before = reader.snapshot().unwrap();
    let result = writer.commit(mutation()).unwrap();
    assert!(result.changed);
    assert_eq!(before.task(12).unwrap().title, "Implement storage");
    assert_eq!(before.rules().unwrap()[0].prompt, "Review");
    let after = writer.snapshot().unwrap();
    assert_eq!(after.task(12).unwrap().title, "Updated task");
    assert_eq!(after.rules().unwrap()[0].prompt, "Updated prompt");
    assert_eq!(after.metadata().cursor, result.cursor);
    assert!(matches!(
        after.receipt("batch").unwrap(),
        Some(OperationReceipt::V1 { .. })
    ));
    drop(after);
    // Obsolete expectedVersion values are allowed ONLY for an identical replay.
    let replay = writer.commit(mutation()).unwrap();
    assert_eq!(
        serde_json::to_value(result).unwrap(),
        serde_json::to_value(replay).unwrap()
    );
    let mut reused = mutation();
    reused.changes.pop();
    assert!(matches!(
        writer.commit(reused),
        Err(StorageError::OperationReused)
    ));
}
#[test]
fn stale_resource_aborts_every_domain_and_returns_exact_version() {
    let (mut store, shared) = setup();
    shared.lock().unwrap().frame.versions[0].version = 2;
    match store.commit(mutation()).unwrap_err() {
        StorageError::Conflict(conflicts) => {
            assert_eq!(conflicts.len(), 1);
            assert_eq!(conflicts[0].current_version, 2);
            assert_eq!(conflicts[0].resource_id, "12");
        }
        other => panic!("{other}"),
    }
    assert_eq!(
        store.snapshot().unwrap().rules().unwrap()[0].prompt,
        "Review"
    );
    assert!(shared.lock().unwrap().receipts.is_empty());
}
#[test]
fn failure_rolls_back_content_receipt_and_cursor() {
    let (mut store, shared) = setup();
    let mut planned = step(mutation());
    planned.fail_before_commit = true;
    shared.lock().unwrap().steps.push_back(planned);
    assert!(store.commit(mutation()).is_err());
    let s = store.snapshot().unwrap();
    assert_eq!(s.metadata().revision, 1);
    assert_eq!(s.task(12).unwrap().title, "Implement storage");
    assert_eq!(s.rules().unwrap()[0].prompt, "Review");
    assert!(shared.lock().unwrap().receipts.is_empty());
}
#[test]
fn no_op_is_receipted_without_a_notification() {
    let (mut store, shared) = setup();
    let mut request = mutation();
    if let Change::Task(TaskWrite::Update { input, .. }) = &mut request.changes[0] {
        input.title = Some("Implement storage".into());
    }
    if let Change::Rule(RuleWrite::Update { input, .. }) = &mut request.changes[1] {
        input.prompt = "Review".into();
    }
    let original = frame();
    let mut planned = step(request.clone());
    planned.after = original.clone();
    planned.result.cursor = original.metadata.cursor.clone();
    planned.result.revision = 1;
    planned.result.changed = false;
    planned.result.versions = original.versions.clone();
    planned.result.outcomes = vec![
        ChangeOutcome::Task(Some(
            serde_json::from_value(original.answers["task"].clone()).unwrap(),
        )),
        ChangeOutcome::Rule(Some(
            serde_json::from_value(original.answers["rules"][0].clone()).unwrap(),
        )),
    ];
    shared.lock().unwrap().steps.push_back(planned);
    assert!(!store.commit(request.clone()).unwrap().changed);
    assert!(!store.commit(request).unwrap().changed);
    assert!(matches!(
        store.poll_changes(&original.metadata.cursor).unwrap(),
        ChangeNotification::Unchanged { .. }
    ));
    assert_eq!(shared.lock().unwrap().receipts.len(), 1);
}
fn document(id: &str, content: Value) -> DesignDocument {
    DesignDocument {
        document_id: id.into(),
        read_token: document_token(id, &content).unwrap(),
        document: content,
    }
}
#[test]
fn content_tokens_cover_cascades_equal_versions_and_deletions() {
    let old = document("element:core", json!({"name":"old","tags":""}));
    let current = document("element:core", json!({"tags":"","name":"new"}));
    let token = DocumentReadToken {
        document_id: old.document_id.clone(),
        read_token: old.read_token,
    };
    let error = check_document_tokens(&[token], std::slice::from_ref(&current)).unwrap_err();
    match error {
        StorageError::Documents(conflicts) => {
            assert_eq!(conflicts[0].code, DocumentConflictCode::OutOfDate);
            assert_eq!(conflicts[0].current_document["name"], "new");
            assert_eq!(conflicts[0].read_token, current.read_token);
        }
        other => panic!("{other}"),
    }
    let cascade = document("binding:core|file|lib.rs", json!({"target":"lib.rs"}));
    assert!(matches!(
        check_document_tokens(&[], &[cascade]),
        Err(StorageError::Documents(_))
    ));
    let absent = document("element:core", Value::Null);
    check_document_tokens(&[], std::slice::from_ref(&absent)).unwrap();
    let stale = DocumentReadToken {
        document_id: current.document_id.clone(),
        read_token: current.read_token,
    };
    assert!(matches!(
        check_document_tokens(&[stale], &[absent]),
        Err(StorageError::Documents(_))
    ));
    assert_eq!(
        document_token("x", &json!({"a":1,"b":{"z":2,"y":3}})).unwrap(),
        document_token("x", &json!({"b":{"y":3,"z":2},"a":1})).unwrap()
    );
}
#[test]
fn dangling_references_abort_the_batch() {
    let (mut store, shared) = setup();
    let mut planned = step(mutation());
    let edge = MissingReference {
        source: ResourceKey {
            kind: "task".into(),
            id: "12".into(),
        },
        target: ResourceKey {
            kind: "design.element".into(),
            id: "deleted".into(),
        },
    };
    planned.edges.push(edge.clone());
    shared.lock().unwrap().steps.push_back(planned);
    assert!(matches!(
        store.commit(mutation()),
        Err(StorageError::References(_))
    ));
    assert_eq!(store.snapshot().unwrap().metadata().revision, 1);
    check_references(
        std::slice::from_ref(&edge),
        &BTreeSet::from([edge.target.clone()]),
    )
    .unwrap();
}
#[test]
fn legacy_receipts_never_claim_identical_replay() {
    let prepared = prepare_mutation(mutation()).unwrap();
    assert!(matches!(
        prepared.replay(Some(&OperationReceipt::Legacy {
            payload: json!({"ok":true})
        })),
        Err(StorageError::OperationReused)
    ));
}
#[test]
fn intents_are_advisory_and_handles_close_independently() {
    let (mut store, shared) = setup();
    let mut other = client(shared.clone());
    let key = ResourceKey {
        kind: "task".into(),
        id: "12".into(),
    };
    let mut intent = IntentUpdate {
        agent_run_id: "agent-a".into(),
        resources: vec![key],
        ttl_seconds: 60,
    };
    store.publish_intents(&intent).unwrap();
    intent.agent_run_id = "agent-b".into();
    assert_eq!(other.publish_intents(&intent).unwrap().len(), 2);
    intent.ttl_seconds = 0;
    assert_eq!(other.publish_intents(&intent).unwrap().len(), 1);
    shared.lock().unwrap().steps.push_back(step(mutation()));
    other.commit(mutation()).unwrap(); // the remaining intent does not lock the task
    store.close().unwrap();
    store.close().unwrap();
    assert!(matches!(store.snapshot(), Err(StorageError::Closed)));
    assert_eq!(other.snapshot().unwrap().metadata().revision, 2);
}
#[test]
fn malformed_batches_are_rejected_before_reaching_a_backend() {
    let (mut store, _) = setup();
    let mut request = mutation();
    request.operation_id.clear();
    assert!(matches!(
        store.commit(request),
        Err(StorageError::Validation(_))
    ));
    let mut request = mutation();
    request.changes.push(request.changes[0].clone());
    assert!(matches!(
        store.commit(request),
        Err(StorageError::Validation(_))
    ));
}
