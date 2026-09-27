//! Real adapter acceptance tests; fixture construction also uses the public API.
use super::*;
use adashi_storage_api::{design::*, documents::*, memory::*, mockups::*, qa::*};
use adashi_storage_api::{health, tasks};
use serde_json::{json, Value};

fn input<T: serde::de::DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).unwrap()
}
fn commit(store: &mut dyn ProjectStorage, id: &str, changes: Vec<Change>) -> CommitResult {
    store.commit(mutation(id, changes)).unwrap()
}
fn token(store: &mut dyn ProjectStorage, id: &str) -> DocumentReadToken {
    let doc = store
        .snapshot()
        .unwrap()
        .design_documents(&[id.into()])
        .unwrap()
        .remove(0);
    DocumentReadToken {
        document_id: doc.document_id,
        read_token: doc.read_token,
    }
}
fn design(changes: Value, tokens: Vec<DocumentReadToken>) -> Change {
    Change::Design(DesignWrite::Save {
        change_intent: "Adapter conformance".into(),
        changes: input(changes),
        read_tokens: tokens,
    })
}
fn version(store: &mut dyn ProjectStorage, kind: &str, id: &str) -> i64 {
    store
        .snapshot()
        .unwrap()
        .resource_versions(&[ResourceKey {
            kind: kind.into(),
            id: id.into(),
        }])
        .unwrap()[0]
        .version
}
fn svg(color: &str) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="60"><rect data-adashi-id="box" width="100" height="60" fill="{color}"/></svg>"#
    )
}
fn mockup_guard(store: &mut dyn ProjectStorage, operation: &str) -> MockupMutationInput {
    let m = store.snapshot().unwrap().mockup("screen").unwrap();
    MockupMutationInput {
        external_id: m.external_id,
        operation_id: operation.into(),
        expected_accepted_version: m.accepted_version,
        expected_working_version: m.working_version,
    }
}
pub(crate) fn populate(store: &mut dyn ProjectStorage) {
    commit(
        store,
        "model",
        vec![design(
            json!([
                {"op":"upsert_element","externalId":"app","parentExternalId":"1","elementType":"Container","name":"Application"},
                {"op":"upsert_relationship","externalId":"uses","sourceExternalId":"1","destinationExternalId":"app","description":"Uses"},
                {"op":"upsert_binding","designExternalId":"app","targetType":"file","target":"app.rs"},
                {"op":"upsert_uml","key":"flow","title":"Flow","language":"mermaid","diagramType":"flow","attachedToExternalId":"app","source":"flowchart TD\n a[Start] --> b[End]"}
            ]),
            vec![],
        )],
    );
    commit(
        store,
        "task",
        vec![
            Change::Task(TaskWrite::Create {
                input: input(
                    json!({"title":"Linked task","designSpecificationLinks":[{"targetType":"element","designExternalId":"app"}]}),
                ),
            }),
            new_rule(),
        ],
    );
    let task_id = store.snapshot().unwrap().tasks(&tasks::ALL_TASK_STATES).unwrap()[0].id;
    commit(
        store,
        "content",
        vec![
            Change::Qa(QaWrite::CreateJob {
                input: input(
                    json!({"name":"Check","command":"echo checked","tags":["adapter"],"taskIds":[task_id],"designSpecificationLinks":[{"targetType":"element","designExternalId":"app"}]}),
                ),
            }),
            Change::Memory(MemoryWrite::Append {
                note: AppendMemoryNote {
                    note_id: "handover".into(),
                    operation_id: "content".into(),
                    run_id: "run-a".into(),
                    task_id: Some(task_id),
                    body: "Retain this decision".into(),
                },
            }),
            Change::Mockup(MockupWrite::Create(input(
                json!({"externalId":"screen","title":"Screen","attachedToExternalId":"app","viewportWidth":100,"viewportHeight":60,"screen":"main","state":"initial","fidelity":"wireframe","acceptedSvg":svg("blue"),"operationId":"content","expectedAcceptedVersion":0,"expectedWorkingVersion":0}),
            ))),
        ],
    );
    let memory = store.snapshot().unwrap().memory().unwrap();
    let prompt = store.snapshot().unwrap().fixed_prompts().unwrap().remove(0);
    let legacy = version(store, "legacy", "content");
    commit(
        store,
        "settings",
        vec![
            Change::Memory(MemoryWrite::Protocol {
                expected_version: memory.protocol_version,
                rule: "Preserve context".into(),
            }),
            Change::FixedPrompt(FixedPromptWrite {
                key: prompt.key,
                expected_version: prompt.version,
                prompt: "Project guidance".into(),
            }),
            Change::Computer {
                expected_version: 0,
                input: ComputerCheckout {
                    computer_id: "computer-b".into(),
                    repository_path: "/peer/checkout".into(),
                },
            },
            Change::HealthWaiver {
                external_id: "app".into(),
                state: health::ElementHealth::Unmapped,
                reason: "Reviewed fixture".into(),
                task_id: Some(task_id),
            },
            Change::Legacy {
                expected_version: legacy,
                content: input(
                    json!({"guidelines":[{"id":41,"title":"Legacy guide","body":"Keep this","severity":"info","createdAt":"2020-01-01"}],"postTaskCommands":[],"qaChecks":[],"taskQaEntries":[]}),
                ),
            },
        ],
    );
    let m = store.snapshot().unwrap().mockup("screen").unwrap();
    commit(
        store,
        "draft",
        vec![Change::Mockup(MockupWrite::SaveDraft(SaveDraftInput {
            external_id: m.external_id,
            working_svg: svg("green"),
            base_revision: m.accepted_revision,
            operation_id: "draft".into(),
            expected_accepted_version: m.accepted_version,
            expected_working_version: m.working_version,
            edit_operations: vec![MockupEditOperation {
                sequence: 0,
                kind: "color".into(),
                target_element_id: Some("box".into()),
                payload_json: "{}".into(),
            }],
            annotations: vec![MockupAnnotation {
                external_id: "note".into(),
                svg_path: "M 1 1 L 2 2".into(),
                optional_text: "Review".into(),
                sort_order: 0,
            }],
        }))],
    );
}
pub(crate) fn contents(store: &mut dyn ProjectStorage) -> Value {
    let s = store.snapshot().unwrap();
    json!({"identity":s.metadata().identity,"design":s.design_inventory().unwrap(),"tasks":s.tasks(&tasks::ALL_TASK_STATES).unwrap(),"jobs":s.qa_jobs(&QaJobQuery::default()).unwrap(),"runs":s.qa_runs(100).unwrap(),"memory":s.memory().unwrap(),"retainedNotes":s.retained_memory_notes().unwrap(),"rules":s.rules().unwrap(),"prompts":s.fixed_prompts().unwrap(),"mockup":s.mockup("screen").unwrap(),"waivers":s.health_waivers("app").unwrap(),"legacy":s.legacy_content().unwrap(),"computers":s.computer_checkouts().unwrap()})
}

#[test]
fn all_domains_survive_reopen_and_mockup_memory_task_lifecycles() {
    let (_root, mut request, mut store) = fixture();
    populate(&mut store);
    let before = contents(&mut store);
    store.close().unwrap();
    request.mode = OpenMode::ReadWrite;
    let mut store = StorageClient::new(SqliteFactory.open(&request).unwrap());
    assert_eq!(contents(&mut store), before);
    let memory = store.snapshot().unwrap().memory().unwrap();
    commit(
        &mut store,
        "compact",
        vec![Change::Memory(MemoryWrite::Compact {
            expected_version: memory.memory_version,
            summary: "Reviewed summary".into(),
            superseded_note_ids: vec!["handover".into()],
        })],
    );
    let note = store
        .snapshot()
        .unwrap()
        .retained_memory_notes()
        .unwrap()
        .remove(0);
    assert_eq!(note.body, "Retain this decision");
    assert!(note.superseded_by_version.is_some());
    let guard = mockup_guard(&mut store, "request-revision");
    commit(
        &mut store,
        "request-revision",
        vec![Change::Mockup(MockupWrite::RequestRevision(guard))],
    );
    let m = store.snapshot().unwrap().mockup("screen").unwrap();
    commit(
        &mut store,
        "propose",
        vec![Change::Mockup(MockupWrite::Propose(ProposeMockupInput {
            external_id: m.external_id,
            base_revision: m.accepted_revision,
            proposed_svg: svg("red"),
            proposed_manifest: m.manifest,
            operation_id: "propose".into(),
            expected_accepted_version: m.accepted_version,
            expected_working_version: m.working_version,
        }))],
    );
    let proposed = contents(&mut store);
    store.close().unwrap();
    let mut store = StorageClient::new(SqliteFactory.open(&request).unwrap());
    assert_eq!(contents(&mut store), proposed);
    let guard = mockup_guard(&mut store, "accept");
    let accepted = commit(
        &mut store,
        "accept",
        vec![Change::Mockup(MockupWrite::AcceptProposal(guard.clone()))],
    );
    let retry = commit(
        &mut store,
        "accept",
        vec![Change::Mockup(MockupWrite::AcceptProposal(guard))],
    );
    assert_eq!(
        serde_json::to_value(accepted).unwrap(),
        serde_json::to_value(retry).unwrap()
    );
    let m = store.snapshot().unwrap().mockup("screen").unwrap();
    assert!(m.accepted_svg.contains("red"));
    assert!(m.proposal.is_none());
    commit(
        &mut store,
        "active",
        vec![Change::Task(TaskWrite::Update {
            expected_version: 1,
            input: input(json!({"taskId":1,"state":"active"})),
        })],
    );
    commit(
        &mut store,
        "finish",
        vec![Change::Task(TaskWrite::Finish {
            expected_version: 2,
            input: input(
                json!({"taskId":1,"completionMemo":"Verified","createdFiles":["new.rs"],"changedFiles":["app.rs"]}),
            ),
        })],
    );
    commit(
        &mut store,
        "close",
        vec![Change::Task(TaskWrite::Close {
            id: 1,
            expected_version: 3,
        })],
    );
    let task = store.snapshot().unwrap().task(1).unwrap();
    assert_eq!(task.state, "closed");
    assert_eq!(task.created_files, vec!["new.rs"]);
}

#[test]
fn stale_guards_final_references_and_domain_errors_rollback_whole_batch() {
    let (_root, _request, mut store) = fixture();
    populate(&mut store);
    let old = token(&mut store, "element:app");
    let update = Change::Design(DesignWrite::Describe {
        updates: vec![ElementDescriptionUpdate {
            external_id: "app".into(),
            description: "Concurrent edit".into(),
        }],
        read_tokens: vec![old.clone()],
    });
    commit(&mut store, "edit", vec![update]);
    let before = contents(&mut store);
    let revision = store.snapshot().unwrap().metadata().revision;
    for (id, change) in [
        (
            "stale-token",
            Change::Design(DesignWrite::Describe {
                updates: vec![ElementDescriptionUpdate {
                    external_id: "app".into(),
                    description: "Stale".into(),
                }],
                read_tokens: vec![old],
            }),
        ),
        (
            "stale-version",
            Change::Task(TaskWrite::Update {
                expected_version: 999,
                input: input(json!({"taskId":1,"title":"Stale"})),
            }),
        ),
        (
            "bad-model",
            design(
                json!([{"op":"upsert_element","externalId":"invalid","elementType":"Component","parentExternalId":"missing","name":"Invalid"}]),
                vec![],
            ),
        ),
    ] {
        assert!(
            store
                .commit(mutation(id, vec![new_rule(), change]))
                .is_err(),
            "{id}"
        );
        assert_eq!(contents(&mut store), before);
        assert!(store.snapshot().unwrap().receipt(id).unwrap().is_none());
        assert_eq!(store.snapshot().unwrap().metadata().revision, revision);
    }
    // Deleting a linked mockup cannot leave a retained task pointing at it.
    commit(
        &mut store,
        "link-screen",
        vec![Change::Task(TaskWrite::Update {
            expected_version: 1,
            input: input(
                json!({"taskId":1,"designSpecificationLinks":[{"targetType":"mockup","designExternalId":"screen"}]}),
            ),
        })],
    );
    let guard = mockup_guard(&mut store, "delete-linked");
    let before = contents(&mut store);
    assert!(matches!(
        store.commit(mutation(
            "delete-linked",
            vec![new_rule(), Change::Mockup(MockupWrite::Delete(guard))]
        )),
        Err(StorageError::References(_))
    ));
    assert_eq!(contents(&mut store), before);
    // A no-op preserves timestamps, versions and cursor, but persists a replay receipt.
    let task = store.snapshot().unwrap().task(1).unwrap();
    let no_op = commit(
        &mut store,
        "no-op",
        vec![Change::Task(TaskWrite::Update {
            expected_version: task.version,
            input: input(json!({"taskId":1,"title":task.title})),
        })],
    );
    assert!(!no_op.changed);
    assert_eq!(contents(&mut store), before);
    assert!(store
        .snapshot()
        .unwrap()
        .receipt("no-op")
        .unwrap()
        .is_some());
}

#[test]
fn upgrade_preserves_content_receipts_and_read_only_is_byte_stable() {
    let (_root, mut request, mut store) = fixture();
    populate(&mut store);
    let before = contents(&mut store);
    store.close().unwrap();
    {
        let db = Connection::open(&request.location).unwrap();
        db.pragma_update(None, "user_version", 13).unwrap();
        db.execute("DELETE FROM resource_versions WHERE resource_kind IN ('qa.run','qa.job-run','computer','legacy','memory.note')",[]).unwrap();
        db.execute("INSERT INTO mutation_operations(project_id,operation_id,result_json) VALUES(1,'legacy-receipt','{\"oldResult\":true}')",[]).unwrap();
    }
    request.mode = OpenMode::ReadOnly;
    let bytes = fs::read(&request.location).unwrap();
    assert!(matches!(
        SqliteFactory.open(&request),
        Err(StorageError::Unavailable(_))
    ));
    assert_eq!(fs::read(&request.location).unwrap(), bytes);
    request.mode = OpenMode::InitializeOrMigrate;
    let mut store = StorageClient::new(SqliteFactory.open(&request).unwrap());
    assert_eq!(contents(&mut store), before);
    assert!(matches!(
        store.snapshot().unwrap().receipt("legacy-receipt").unwrap(),
        Some(OperationReceipt::Legacy { .. })
    ));
    assert!(matches!(
        store.commit(mutation("legacy-receipt", vec![new_rule()])),
        Err(StorageError::OperationReused)
    ));
    let revision = store.snapshot().unwrap().metadata().revision;
    store.close().unwrap();
    let bytes = fs::read(&request.location).unwrap();
    let modified = fs::metadata(&request.location).unwrap().modified().unwrap();
    request.mode = OpenMode::ReadOnly;
    request.computer_id = "unregistered-reader".into();
    request.checkout_path = "/never-register-on-read".into();
    for _ in 0..3 {
        let mut reader = StorageClient::new(SqliteFactory.open(&request).unwrap());
        assert_eq!(contents(&mut reader), before);
        assert_eq!(reader.snapshot().unwrap().metadata().revision, revision);
        assert!(matches!(
            reader.commit(mutation("forbidden", vec![new_rule()])),
            Err(StorageError::AccessDenied)
        ));
        reader.close().unwrap();
        assert_eq!(fs::read(&request.location).unwrap(), bytes);
        assert_eq!(
            fs::metadata(&request.location).unwrap().modified().unwrap(),
            modified
        );
    }
}

fn reserve(store: &mut dyn ProjectStorage, id: &str) -> QaRun {
    let job = store.snapshot().unwrap().qa_job(1).unwrap();
    let result = commit(
        store,
        id,
        vec![Change::Qa(QaWrite::StartRun {
            query: QaJobQuery::default(),
            trigger_source: "test".into(),
            jobs: vec![QaExecutionPlan {
                job_id: 1,
                expected_version: job.version,
                command_snapshot: crate::qa_runner::command_snapshot(&job).unwrap(),
            }],
        })],
    );
    let ChangeOutcome::QaRun(run) = result.outcomes.into_iter().next().unwrap() else {
        panic!("run")
    };
    run
}
#[test]
fn qa_claims_are_exclusive_and_retention_preserves_active_runs() {
    let (_root, mut request, mut store) = fixture();
    populate(&mut store);
    let held = reserve(&mut store, "held");
    let held_job = held.job_runs[0].id;
    commit(
        &mut store,
        "claim",
        vec![Change::Qa(QaWrite::ClaimJob {
            job_run_id: held_job,
            expected_version: 1,
        })],
    );
    request.mode = OpenMode::ReadWrite;
    let mut peer = StorageClient::new(SqliteFactory.open(&request).unwrap());
    assert!(matches!(
        peer.commit(mutation(
            "second-claim",
            vec![Change::Qa(QaWrite::ClaimJob {
                job_run_id: held_job,
                expected_version: 1
            })]
        )),
        Err(StorageError::Conflict(_))
    ));
    for i in 0..4 {
        let run = reserve(&mut peer, &format!("run-{i}"));
        let job = run.job_runs[0].id;
        commit(
            &mut peer,
            &format!("claim-{i}"),
            vec![Change::Qa(QaWrite::ClaimJob {
                job_run_id: job,
                expected_version: 1,
            })],
        );
        commit(
            &mut peer,
            &format!("evidence-{i}"),
            vec![Change::Qa(QaWrite::CompleteJob {
                job_run_id: job,
                expected_version: 2,
                evidence: QaEvidence {
                    outcome: QaJobOutcome::Passed,
                    exit_code: Some(0),
                    duration_ms: 3,
                    output: format!("Evidence {i}"),
                },
            })],
        );
        commit(
            &mut peer,
            &format!("complete-{i}"),
            vec![Change::Qa(QaWrite::CompleteRun {
                run_id: run.id,
                expected_version: 1,
            })],
        );
    }
    let run = store.snapshot().unwrap().qa_run(held.id).unwrap();
    assert_eq!(run.job_runs[0].id, held_job);
    assert_eq!(run.status, "running");
    let history = store.snapshot().unwrap().qa_job(1).unwrap().run_history;
    assert_eq!(history.len(), 2);
    assert!(history.iter().all(|r| r.output.starts_with("Evidence")));
    assert!(store
        .snapshot()
        .unwrap()
        .qa_run_summaries(20)
        .unwrap()
        .iter()
        .any(|r| r.id == held.id));
}
