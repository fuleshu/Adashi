use super::*;
use crate::storage::{api::*, ProjectStore};
use serde_json::json;

fn commit(store: &mut dyn ProjectStorage, id: &str, changes: Vec<Change>) -> CommitResult {
    store
        .commit(Mutation {
            operation_id: id.into(),
            changes,
        })
        .unwrap()
}
fn reserve(store: &mut dyn ProjectStorage, id: &str) -> qa::QaRun {
    let job = store.snapshot().unwrap().qa_job(1).unwrap();
    let result = commit(
        store,
        id,
        vec![Change::Qa(QaWrite::StartRun {
            query: qa::QaJobQuery::default(),
            trigger_source: "migration-test".into(),
            jobs: vec![QaExecutionPlan {
                job_id: job.id,
                expected_version: job.version,
                command_snapshot: crate::qa_runner::command_snapshot(&job).unwrap(),
            }],
        })],
    );
    let ChangeOutcome::QaRun(run) = result.outcomes.into_iter().next().unwrap() else {
        panic!("run");
    };
    run
}

fn fixture() -> (tempfile::TempDir, ProjectRegistration) {
    let (root, request, mut store) = sqlite::tests::fixture();
    sqlite::tests::conformance::populate(&mut store);
    let m = store.snapshot().unwrap().mockup("screen").unwrap();
    commit(
        &mut store,
        "request",
        vec![Change::Mockup(MockupWrite::RequestRevision(
            mockups::MockupMutationInput {
                external_id: m.external_id,
                operation_id: "request".into(),
                expected_accepted_version: m.accepted_version,
                expected_working_version: m.working_version,
            },
        ))],
    );
    let m = store.snapshot().unwrap().mockup("screen").unwrap();
    commit(
        &mut store,
        "proposal",
        vec![Change::Mockup(MockupWrite::Propose(
            mockups::ProposeMockupInput {
                external_id: m.external_id,
                base_revision: m.accepted_revision,
                proposed_svg: m.accepted_svg.replace("blue", "red"),
                proposed_manifest: m.manifest,
                operation_id: "proposal".into(),
                expected_accepted_version: m.accepted_version,
                expected_working_version: m.working_version,
            },
        ))],
    );
    let memory = store.snapshot().unwrap().memory().unwrap();
    commit(
        &mut store,
        "compact",
        vec![Change::Memory(MemoryWrite::Compact {
            expected_version: memory.memory_version,
            summary: "Reviewed migration history".into(),
            superseded_note_ids: vec!["handover".into()],
        })],
    );
    let run = reserve(&mut store, "history");
    let job = run.job_runs[0].id;
    commit(
        &mut store,
        "claim-history",
        vec![Change::Qa(QaWrite::ClaimJob {
            job_run_id: job,
            expected_version: 1,
        })],
    );
    commit(
        &mut store,
        "evidence-history",
        vec![Change::Qa(QaWrite::CompleteJob {
            job_run_id: job,
            expected_version: 2,
            evidence: QaEvidence {
                outcome: QaJobOutcome::Passed,
                exit_code: Some(0),
                duration_ms: 7,
                output: "Historical evidence\nsecond line".into(),
            },
        })],
    );
    commit(
        &mut store,
        "complete-history",
        vec![Change::Qa(QaWrite::CompleteRun {
            run_id: run.id,
            expected_version: 1,
        })],
    );
    store.close().unwrap();
    let project = ProjectRegistration {
        id: request.registered_identity.id,
        name: request.registered_identity.name,
        folder: request.checkout_path,
    };
    (root, project)
}
#[test]
fn complete_project_round_trips_and_old_clients_cannot_write() {
    let (root, project) = fixture();
    let mut old = ProjectStore::open(&project).unwrap();
    let original = adapter("sqlite")
        .unwrap()
        .capture(&project, false)
        .unwrap()
        .image()
        .clone();
    let old_cursor = old.snapshot().unwrap().metadata().cursor.clone();
    let receipt =
        serde_json::to_value(old.snapshot().unwrap().receipt("proposal").unwrap()).unwrap();
    let plan = preview(&project, "text").unwrap();
    let report = convert(&project, "text", &plan.plan_token, false).unwrap();
    assert!(Path::new(&report.backup_folder)
        .join("source/.adashi/adashi.sqlite3")
        .exists());
    assert!(old
        .commit(Mutation {
            operation_id: "stale-client".into(),
            changes: vec![]
        })
        .unwrap_err()
        .to_string()
        .contains("reopen"));
    drop(old);
    let text_image = adapter("text")
        .unwrap()
        .capture(&project, false)
        .unwrap()
        .image()
        .clone();
    assert_eq!(
        original.semantic_fingerprint().unwrap(),
        text_image.semantic_fingerprint().unwrap()
    );
    let mut fresh = ProjectStore::open(&project).unwrap();
    assert_eq!(
        serde_json::to_value(fresh.snapshot().unwrap().receipt("proposal").unwrap()).unwrap(),
        receipt
    );
    assert!(fresh
        .snapshot()
        .unwrap()
        .mockup("screen")
        .unwrap()
        .proposal
        .is_some());
    assert!(fresh
        .snapshot()
        .unwrap()
        .mockup("screen")
        .unwrap()
        .working_svg
        .is_some());
    assert_eq!(
        fresh.snapshot().unwrap().qa_runs(10).unwrap()[0].job_runs[0].output,
        "Historical evidence\nsecond line"
    );
    assert_ne!(old_cursor, fresh.snapshot().unwrap().metadata().cursor);
    let version = fresh.snapshot().unwrap().task(1).unwrap().version;
    drop(fresh);
    let plan = preview(&project, "sqlite").unwrap();
    assert!(plan.destination_populated);
    assert!(convert(&project, "sqlite", &plan.plan_token, false).is_err());
    convert(&project, "sqlite", &plan.plan_token, true).unwrap();
    let mut fresh = ProjectStore::open(&project).unwrap();
    assert_eq!(
        serde_json::to_value(fresh.snapshot().unwrap().receipt("proposal").unwrap()).unwrap(),
        receipt
    );
    assert!(matches!(
        fresh.commit(Mutation {
            operation_id: "dirty-text-editor".into(),
            changes: vec![Change::Task(TaskWrite::Update {
                expected_version: version,
                input: serde_json::from_value(
                    serde_json::json!({"taskId":1,"title":"stale overwrite"})
                )
                .unwrap()
            })]
        }),
        Err(StorageError::Conflict(_))
    ));
    drop(fresh);
    let sql_image = adapter("sqlite")
        .unwrap()
        .capture(&project, false)
        .unwrap()
        .image()
        .clone();
    assert_eq!(
        text_image.semantic_fingerprint().unwrap(),
        sql_image.semantic_fingerprint().unwrap()
    );
    let plan = preview(&project, "text").unwrap();
    convert(&project, "text", &plan.plan_token, true).unwrap();
    let again = adapter("text")
        .unwrap()
        .capture(&project, false)
        .unwrap()
        .image()
        .clone();
    assert_eq!(
        text_image.files.keys().collect::<Vec<_>>(),
        again.files.keys().collect::<Vec<_>>()
    );
    assert!(fs::read_to_string(root.path().join(".gitignore"))
        .unwrap()
        .contains("!/.adashi/text/**"));
}

#[test]
fn live_source_edits_running_qa_and_project_selection_are_checked() {
    let (_root, project) = fixture();
    let (_other_root, other) = fixture();
    let plan = preview(&project, "text").unwrap();
    let mut store = ProjectStore::open(&project).unwrap();
    let task = store.snapshot().unwrap().task(1).unwrap();
    commit(
        &mut store,
        "peer-change",
        vec![Change::Task(TaskWrite::Update {
            expected_version: task.version,
            input: serde_json::from_value(json!({"taskId":1,"title":"Peer update"})).unwrap(),
        })],
    );
    assert!(convert(&project, "text", &plan.plan_token, false).is_err());
    let plan = preview(&project, "text").unwrap();
    convert(&project, "text", &plan.plan_token, false).unwrap();
    drop(store);
    assert_eq!(status(&other).unwrap().backend, "sqlite");
    let mut current = ProjectStore::open(&project).unwrap();
    assert_eq!(
        current.snapshot().unwrap().task(1).unwrap().title,
        "Peer update"
    );
    reserve(&mut current, "still-running");
    drop(current);
    assert!(preview(&project, "sqlite")
        .err()
        .unwrap()
        .to_string()
        .contains("running QA"));
    assert_eq!(status(&project).unwrap().backend, "text");
}

#[test]
fn canonical_text_edit_is_detected_outside_the_pinned_view() {
    let (_root, project) = fixture();
    let plan = preview(&project, "text").unwrap();
    convert(&project, "text", &plan.plan_token, false).unwrap();
    let source = adapter("text").unwrap().capture(&project, true).unwrap();
    let name = source
        .image()
        .files
        .keys()
        .find(|p| p.starts_with("records/rules/"))
        .unwrap();
    let path = Path::new(&project.folder).join(".adashi/text").join(name);
    let mut bytes = fs::read(&path).unwrap();
    bytes.push(b'\n');
    fs::write(path, bytes).unwrap();
    assert!(source.ensure_unchanged().is_err());
}
#[test]
fn stale_preview_and_unknown_data_do_not_activate() {
    let (root, project) = fixture();
    let plan = preview(&project, "text").unwrap();
    fs::write(root.path().join(".gitignore"), "changed\n").unwrap();
    assert!(convert(&project, "text", &plan.plan_token, false)
        .unwrap_err()
        .to_string()
        .contains("after preview"));
    assert_eq!(status(&project).unwrap().backend, "sqlite");
    let db = rusqlite::Connection::open(crate::settings::project_database_path(&project)).unwrap();
    db.execute("CREATE TABLE unknown_authoritative_data (id INTEGER)", [])
        .unwrap();
    assert!(preview(&project, "text").is_err());
    assert!(!root.path().join(".adashi/storage.json").exists());
}
#[test]
fn interrupted_activation_rolls_back_or_forward_without_losing_external_edits() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let after = BTreeMap::from([
        (journal::DESCRIPTOR.into(), Some(b"new selection".to_vec())),
        (".gitignore".into(), Some(b"new ignore".to_vec())),
    ]);
    fs::write(root.join(".gitignore"), "original").unwrap();
    journal::interrupt(root, &root.join("backup"), after.clone(), false);
    journal::recover(root).unwrap();
    assert_eq!(fs::read(root.join(".gitignore")).unwrap(), b"original");
    assert!(!root.join(journal::DESCRIPTOR).exists());
    journal::interrupt(root, &root.join("backup"), after.clone(), true);
    journal::recover(root).unwrap();
    assert_eq!(fs::read(root.join(".gitignore")).unwrap(), b"new ignore");
    fs::remove_file(root.join(journal::DESCRIPTOR)).unwrap();
    journal::interrupt(root, &root.join("backup"), after, false);
    fs::write(root.join(".gitignore"), "external edit").unwrap();
    assert!(journal::recover(root)
        .unwrap_err()
        .to_string()
        .contains("external edit"));
    assert_eq!(fs::read(root.join(".gitignore")).unwrap(), b"external edit");
    assert!(root.join(".adashi/local/storage-migration.json").exists());
}

#[test]
fn reopening_an_interrupted_conversion_restores_a_usable_source() {
    let (root, project) = fixture();
    let descriptor = StorageDescriptor {
        schema_version: 1,
        backend: BackendSelection::Text {},
        generation: Some(uuid::Uuid::new_v4().to_string()),
    };
    let after = BTreeMap::from([
        (
            journal::DESCRIPTOR.into(),
            Some(serde_json::to_vec(&descriptor).unwrap()),
        ),
        (
            ".adashi/text/format.json".into(),
            Some(b"unfinished staged publication".to_vec()),
        ),
    ]);
    journal::interrupt(root.path(), &root.path().join("backup"), after, false);
    let mut source = ProjectStore::open(&project).unwrap();
    assert_eq!(
        source.snapshot().unwrap().task(1).unwrap().title,
        "Linked task"
    );
    assert_eq!(status(&project).unwrap().backend, "sqlite");
    assert!(!root.path().join(".adashi/text/format.json").exists());
}

#[test]
fn external_revert_to_a_valid_before_image_cannot_activate_partial_data() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    fs::write(root.join(".gitignore"), "original").unwrap();
    let activation = journal::Activation::prepare(
        root,
        BTreeMap::from([
            (journal::DESCRIPTOR.into(), Some(b"new selection".to_vec())),
            (".gitignore".into(), Some(b"converted".to_vec())),
        ]),
    )
    .unwrap();
    activation.save(root, &root.join("backup")).unwrap();
    assert!(activation
        .publish(root, || {
            fs::write(root.join(".gitignore"), "original").unwrap();
            Ok(())
        })
        .unwrap_err()
        .to_string()
        .contains("not activated"));
    journal::recover(root).unwrap();
    assert!(!root.join(journal::DESCRIPTOR).exists());
    assert_eq!(fs::read(root.join(".gitignore")).unwrap(), b"original");
}

#[test]
fn activated_selection_never_seeds_missing_clone_data() {
    for backend in [BackendSelection::Sqlite {}, BackendSelection::Text {}] {
        let root = tempfile::tempdir().unwrap();
        let project = ProjectRegistration {
            id: "missing".into(),
            name: "Missing clone data".into(),
            folder: root.path().to_string_lossy().into_owned(),
        };
        let descriptor = StorageDescriptor {
            schema_version: 1,
            backend,
            generation: Some(uuid::Uuid::new_v4().to_string()),
        };
        text::journal::atomic_write(
            &root.path().join(journal::DESCRIPTOR),
            &serde_json::to_vec(&descriptor).unwrap(),
        )
        .unwrap();
        assert!(ProjectStore::open(&project)
            .err()
            .unwrap()
            .to_string()
            .contains("storage is missing"));
        assert!(!root.path().join(".adashi/adashi.sqlite3").exists());
        assert!(!root.path().join(".adashi/text/format.json").exists());
    }
}

#[test]
fn deleted_sqlite_high_water_ids_survive_round_trip() {
    let (_root, project) = fixture();
    let db = rusqlite::Connection::open(crate::settings::project_database_path(&project)).unwrap();
    db.execute("INSERT INTO rules(id,project_id,name,enabled,intend,hook,prompt) SELECT 70000,project_id,'Deleted high ID',enabled,intend,hook,prompt FROM rules LIMIT 1",[]).unwrap();
    db.execute("DELETE FROM rules WHERE id=70000", []).unwrap();
    drop(db);
    let plan = preview(&project, "text").unwrap();
    convert(&project, "text", &plan.plan_token, false).unwrap();
    let plan = preview(&project, "sqlite").unwrap();
    convert(&project, "sqlite", &plan.plan_token, true).unwrap();
    let mut store = ProjectStore::open(&project).unwrap();
    let result = commit(
        &mut store,
        "new-after-deletion",
        vec![Change::Rule(RuleWrite::Create {
            input: crate::rules::NewRule {
                name: "After deletion".into(),
                enabled: true,
                intend: "general".into(),
                hook: "run.start".into(),
                prompt: "Preserve aliases".into(),
            },
        })],
    );
    let ChangeOutcome::Rule(Some(rule)) = &result.outcomes[0] else {
        panic!("rule");
    };
    assert!(rule.id > 70000);
    drop(store);
    let plan = preview(&project, "text").unwrap();
    convert(&project, "text", &plan.plan_token, true).unwrap();
}
