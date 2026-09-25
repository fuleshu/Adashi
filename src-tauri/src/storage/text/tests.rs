use super::*;
use api::{Change, ChangeOutcome, Mutation, ProjectStorage, RuleWrite, StorageClient};

mod id_parity;
mod task_numbers;
mod upgrade;

fn request(root: &Path) -> api::OpenRequest {
    api::OpenRequest {
        location: root.join(".adashi/text").to_string_lossy().into_owned(),
        registered_identity: api::ProjectIdentity {
            id: "fixture".into(),
            name: "Fixture".into(),
        },
        computer_id: "test-computer".into(),
        checkout_path: root.to_string_lossy().into_owned(),
        mode: api::OpenMode::InitializeOrMigrate,
        cursor_scope: None,
    }
}
fn store(request: &api::OpenRequest) -> StorageClient<Box<dyn StorageBackend>> {
    StorageClient::new(TextFactory.open(request).unwrap())
}
fn new_rule(name: &str) -> api::rules::NewRule {
    api::rules::NewRule {
        name: name.into(),
        enabled: true,
        intend: "implementation".into(),
        hook: "task.start".into(),
        prompt: format!("{name}\nsecond line\n"),
    }
}
fn create(store: &mut dyn ProjectStorage, op: &str, name: &str) -> api::rules::Rule {
    let result = store
        .commit(Mutation {
            operation_id: op.into(),
            changes: vec![Change::Rule(RuleWrite::Create {
                input: new_rule(name),
            })],
        })
        .unwrap();
    let ChangeOutcome::Rule(Some(rule)) = result.outcomes.into_iter().next().unwrap() else {
        panic!()
    };
    rule
}
fn update(rule: &api::rules::Rule, op: &str, prompt: &str) -> Mutation {
    let mut input = new_rule(&rule.name);
    input.prompt = prompt.into();
    Mutation {
        operation_id: op.into(),
        changes: vec![Change::Rule(RuleWrite::Update {
            id: rule.id,
            expected_version: rule.version,
            input,
        })],
    }
}

#[test]
fn removed_natural_child_key_can_be_added_again_with_same_identity() {
    let dir = tempfile::tempdir().unwrap();
    let req = request(dir.path());
    let mut store = store(&req);
    let created = store
        .commit(Mutation {
            operation_id: "tagged".into(),
            changes: vec![Change::Qa(api::QaWrite::CreateJob {
                input: serde_json::from_value(
                    serde_json::json!({"name":"Tags","command":"echo ok","tags":["keep"]}),
                )
                .unwrap(),
            })],
        })
        .unwrap();
    let api::ChangeOutcome::QaJob(Some(job)) = &created.outcomes[0] else {
        panic!("job");
    };
    let id = job.id;
    let root = Path::new(&req.location);
    let original = journal::inventory(root).unwrap();
    let identity = original
        .keys()
        .find(|p| p.starts_with("records/qa_job_tags/"))
        .unwrap()
        .clone();
    for (operation, tags) in [("remove", vec![]), ("restore", vec!["keep"])] {
        let version = store.snapshot().unwrap().qa_job(id).unwrap().version;
        store
            .commit(Mutation {
                operation_id: operation.into(),
                changes: vec![Change::Qa(api::QaWrite::UpdateJob {
                    expected_version: version,
                    input: serde_json::from_value(serde_json::json!({"qaJobId":id,"tags":tags}))
                        .unwrap(),
                })],
            })
            .unwrap();
    }
    assert_eq!(
        store.snapshot().unwrap().qa_job(id).unwrap().tags,
        vec!["keep"]
    );
    let files = journal::inventory(root).unwrap();
    let record: codec::Record = codec::parse(&files[&identity], &identity).unwrap();
    assert!(!record.deleted);
}

#[test]
fn text_roundtrip_guards_replay_and_write_free_reads() {
    let dir = tempfile::tempdir().unwrap();
    let request = request(dir.path());
    let mut a = store(&request);
    let rule = create(&mut a, "rule-one", "First");
    assert_eq!(rule.id, 1);
    assert!(rule.id <= MAX_SAFE);
    let before = journal::inventory(Path::new(&request.location)).unwrap();
    let mut b = store(&request);
    for _ in 0..3 {
        assert_eq!(
            b.snapshot().unwrap().rules().unwrap()[0].prompt,
            rule.prompt
        );
    }
    assert_eq!(
        before,
        journal::inventory(Path::new(&request.location)).unwrap()
    );
    let pinned = a.snapshot().unwrap();
    let mutation = update(&rule, "edit", "Changed");
    let result = b.commit(mutation.clone()).unwrap();
    assert_eq!(pinned.rules().unwrap()[0].prompt, rule.prompt);
    drop(pinned);
    assert_eq!(
        serde_json::to_value(result).unwrap(),
        serde_json::to_value(b.commit(mutation).unwrap()).unwrap()
    );
    assert!(matches!(
        a.commit(update(&rule, "stale", "lost")),
        Err(StorageError::Conflict(_))
    ));
    assert_eq!(a.snapshot().unwrap().rules().unwrap()[0].prompt, "Changed");
}

#[test]
fn duplicate_fields_and_conflict_markers_are_rejected() {
    assert!(codec::parse::<Value>(br#"{"a":1,"a":2}"#, "test").is_err());
    assert!(codec::parse::<Value>(b"<<<<<<< branch\n{}", "test").is_err());
    let source = Value::String("a\r\nb\n".into());
    assert_eq!(
        source,
        codec::decode(&codec::encode(&source), "test").unwrap()
    );
}

#[test]
fn no_op_keeps_cursor_and_local_state_does_not_leak_into_a_clone() {
    let dir = tempfile::tempdir().unwrap();
    let req = request(dir.path());
    let mut a = store(&req);
    let rule = create(&mut a, "first", "First");
    let before = a.snapshot().unwrap().metadata().cursor.clone();
    let files_before = portable(&journal::inventory(Path::new(&req.location)).unwrap());
    let result = a.commit(update(&rule, "no-op", &rule.prompt)).unwrap();
    assert!(!result.changed);
    assert_eq!(result.cursor, before);
    assert_eq!(
        portable(&journal::inventory(Path::new(&req.location)).unwrap()),
        files_before
    );
    assert!(a.snapshot().unwrap().receipt("no-op").unwrap().is_some());
    let local_change = Mutation {
        operation_id: "computer".into(),
        changes: vec![Change::Computer {
            expected_version: 0,
            input: api::ComputerCheckout {
                computer_id: "peer".into(),
                repository_path: "/private/checkouts/peer".into(),
            },
        }],
    };
    let saved = a.commit(local_change.clone()).unwrap();
    let mut b = store(&req);
    assert_eq!(
        serde_json::to_value(saved).unwrap(),
        serde_json::to_value(b.commit(local_change.clone()).unwrap()).unwrap()
    );
    a.publish_intents(&api::IntentUpdate {
        agent_run_id: "local-agent".into(),
        resources: vec![api::ResourceKey {
            kind: "rule".into(),
            id: rule.id.to_string(),
        }],
        ttl_seconds: 60,
    })
    .unwrap();
    assert_eq!(b.snapshot().unwrap().live_intents().unwrap().len(), 1);
    let canonical = portable(&journal::inventory(Path::new(&req.location)).unwrap());
    assert!(!canonical
        .keys()
        .any(|p| p.starts_with("records/mutation_operations/")));
    assert!(!canonical
        .values()
        .any(|b| String::from_utf8_lossy(b).contains("/private/checkouts/peer")));
    let clone = tempfile::tempdir().unwrap();
    let clone_request = request(clone.path());
    write_files(Path::new(&clone_request.location), &canonical);
    let mut peer = store(&clone_request);
    assert!(peer.snapshot().unwrap().live_intents().unwrap().is_empty());
    assert!(peer.snapshot().unwrap().receipt("first").unwrap().is_none());
    assert!(peer.snapshot().unwrap().receipt("no-op").unwrap().is_none());
    assert!(peer
        .snapshot()
        .unwrap()
        .receipt("computer")
        .unwrap()
        .is_none());
    // Request IDs belong to the checkout. A clone has no request history.
    peer.commit(local_change).unwrap();
}

fn write_files(root: &Path, files: &Files) {
    for (p, b) in files.iter().filter(|(p, _)| !p.starts_with("$local/")) {
        journal::atomic_write(&root.join(p), b).unwrap();
    }
    if let Some(b) = files.get(local::PATH) {
        journal::atomic_write(&root.parent().unwrap().join("local/state.json"), b).unwrap();
    }
}
fn portable(files: &Files) -> Files {
    files
        .iter()
        .filter(|(p, _)| !p.starts_with("$local/"))
        .map(|(p, b)| (p.clone(), b.clone()))
        .collect()
}

#[test]
fn independent_clone_edits_merge_without_common_files() {
    let a_dir = tempfile::tempdir().unwrap();
    let b_dir = tempfile::tempdir().unwrap();
    let ar = request(a_dir.path());
    let br = request(b_dir.path());
    let mut a = store(&ar);
    let first = create(&mut a, "first", "First");
    let second = create(&mut a, "second", "Second");
    let base = portable(&journal::inventory(Path::new(&ar.location)).unwrap());
    write_files(Path::new(&br.location), &base);
    let mut b = store(&br);
    let cursor = a.snapshot().unwrap().metadata().cursor.clone();
    a.commit(update(&first, "a-edit", "A edited first"))
        .unwrap();
    b.commit(update(&second, "b-edit", "B edited second"))
        .unwrap();
    let af = portable(&journal::inventory(Path::new(&ar.location)).unwrap());
    let bf = portable(&journal::inventory(Path::new(&br.location)).unwrap());
    for (p, bytes) in bf.iter().filter(|(p, v)| base.get(*p) != Some(*v)) {
        assert_eq!(af.get(p), base.get(p), "both branches touched {p}");
        journal::atomic_write(&Path::new(&ar.location).join(p), bytes).unwrap();
    }
    assert!(matches!(
        a.poll_changes(&cursor).unwrap(),
        api::ChangeNotification::Reset { .. }
    ));
    let rules = a.snapshot().unwrap().rules().unwrap();
    assert_eq!(rules.len(), 2);
    assert!(rules.iter().any(|r| r.prompt == "A edited first"));
    assert!(rules.iter().any(|r| r.prompt == "B edited second"));
}

#[test]
fn external_content_with_unchanged_counter_rejects_old_guard() {
    let dir = tempfile::tempdir().unwrap();
    let req = request(dir.path());
    let mut s = store(&req);
    let rule = create(&mut s, "first", "First");
    let files = journal::inventory(Path::new(&req.location)).unwrap();
    let (path, bytes) = files
        .iter()
        .find(|(p, _)| p.starts_with("records/rules/"))
        .unwrap();
    let mut record: codec::Record = codec::parse(bytes, path).unwrap();
    record
        .data
        .insert("prompt".into(), "Branch content changed".into());
    journal::atomic_write(
        &Path::new(&req.location).join(path),
        &codec::bytes(&record).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        s.commit(update(&rule, "stale", "lost")),
        Err(StorageError::Conflict(_))
    ));
    assert_eq!(
        s.snapshot().unwrap().rules().unwrap()[0].prompt,
        "Branch content changed"
    );
}

#[test]
fn full_sqlite_fixture_preserves_every_authoritative_record_in_text() {
    let (_dir, source_request, mut source) = sqlite::tests::fixture();
    sqlite::tests::conformance::populate(&mut source);
    let request = api::OpenRequest {
        location: Path::new(&source_request.location)
            .parent()
            .unwrap()
            .join("text")
            .to_string_lossy()
            .into_owned(),
        ..source_request.clone()
    };
    let sql = sqlite::SqliteStorage::open_request(&source_request).unwrap();
    let db = sql.connection().unwrap();
    let tables = engine::tables(db).unwrap();
    let rows = engine::dump(db, &tables).unwrap();
    let project = db
        .query_row("SELECT id FROM projects", [], |r| r.get(0))
        .unwrap();
    let mut local = local::LocalState::default();
    local.capture(db, project).unwrap();
    let mut files =
        engine::export(&rows, &tables, &BTreeMap::new(), &engine::Rows::new(), true).unwrap();
    local.add_to(&mut files).unwrap();
    write_files(Path::new(&request.location), &files);
    let mut text = store(&request);
    let mut expected = sqlite::tests::conformance::contents(&mut source);
    let mut actual = sqlite::tests::conformance::contents(&mut text);
    fn strip(v: &mut Value) {
        match v {
            Value::Object(o) => {
                for k in [
                    "version",
                    "acceptedVersion",
                    "workingVersion",
                    "memoryVersion",
                    "protocolVersion",
                    "structurizrDsl",
                    "structurizrJson",
                ] {
                    o.remove(k);
                }
                for v in o.values_mut() {
                    strip(v);
                }
            }
            Value::Array(a) => {
                for v in a {
                    strip(v)
                }
            }
            _ => {}
        }
    }
    strip(&mut expected);
    strip(&mut actual);
    assert_eq!(expected, actual);
    let task = text.snapshot().unwrap().task(1).unwrap();
    text.commit(Mutation {
        operation_id: "text-task-edit".into(),
        changes: vec![Change::Task(api::TaskWrite::Update {
            expected_version: task.version,
            input: serde_json::from_value(serde_json::json!({"taskId":1,"title":"Text edited"}))
                .unwrap(),
        })],
    })
    .unwrap();
    let memory = text.snapshot().unwrap().memory().unwrap();
    text.commit(Mutation {
        operation_id: "text-protocol".into(),
        changes: vec![Change::Memory(api::MemoryWrite::Protocol {
            expected_version: memory.protocol_version,
            rule: "Text rule".into(),
        })],
    })
    .unwrap();
    let m = text.snapshot().unwrap().mockup("screen").unwrap();
    text.commit(Mutation {
        operation_id: "text-discard".into(),
        changes: vec![Change::Mockup(api::MockupWrite::DiscardDraft(
            api::mockups::MockupMutationInput {
                external_id: m.external_id,
                operation_id: "text-discard".into(),
                expected_accepted_version: m.accepted_version,
                expected_working_version: m.working_version,
            },
        ))],
    })
    .unwrap();
    let s = text.snapshot().unwrap();
    assert_eq!(s.task(1).unwrap().title, "Text edited");
    assert_eq!(s.memory().unwrap().rule, "Text rule");
    assert!(s.mockup("screen").unwrap().working_svg.is_none());
}

#[test]
fn prepared_publication_recovers_before_reading_and_preserves_external_edits() {
    let dir = tempfile::tempdir().unwrap();
    let req = request(dir.path());
    let root = Path::new(&req.location);
    let local = root.parent().unwrap().join("local");
    let mut s = store(&req);
    let rule = create(&mut s, "first", "First");
    let before = journal::inventory(root).unwrap();
    s.commit(update(&rule, "edit", "Recovered")).unwrap();
    let after = journal::inventory(root).unwrap();
    for p in after
        .keys()
        .filter(|p| !p.starts_with("$local/") && !before.contains_key(*p))
    {
        fs::remove_file(root.join(p)).unwrap();
    }
    write_files(root, &before);
    journal::stage_for_test(&local, &before, &after).unwrap();
    let (p, b) = after
        .iter()
        .find(|(p, b)| p.starts_with("records/rules/") && before.get(*p) != Some(*b))
        .unwrap();
    journal::atomic_write(&root.join(p), b).unwrap();
    assert_eq!(
        s.snapshot().unwrap().rules().unwrap()[0].prompt,
        "Recovered"
    );
    assert_eq!(journal::inventory(root).unwrap(), after);
    journal::stage_for_test(&local, &before, &after).unwrap();
    journal::atomic_write(&root.join(p), b"external edit").unwrap();
    assert!(s
        .snapshot()
        .err()
        .unwrap()
        .to_string()
        .contains("external edit"));
    assert_eq!(fs::read(root.join(p)).unwrap(), b"external edit");
    assert!(local.join("text-transaction.json").exists());
}

#[test]
fn malformed_merged_reference_and_future_schema_prevent_all_writes() {
    let dir = tempfile::tempdir().unwrap();
    let req = request(dir.path());
    let root = Path::new(&req.location);
    let mut s = store(&req);
    let rule = create(&mut s, "first", "First");
    let files = journal::inventory(root).unwrap();
    let (p, b) = files
        .iter()
        .find(|(p, _)| p.starts_with("records/rules/"))
        .unwrap();
    let mut record: codec::Record = codec::parse(b, p).unwrap();
    record.data.insert(
        "project_id".into(),
        serde_json::json!({"ref":uuid::Uuid::new_v4().to_string()}),
    );
    journal::atomic_write(&root.join(p), &codec::bytes(&record).unwrap()).unwrap();
    let invalid = journal::inventory(root).unwrap();
    assert!(s.commit(update(&rule, "broken", "lost")).is_err());
    assert_eq!(journal::inventory(root).unwrap(), invalid);
    write_files(root, &files);
    journal::atomic_write(
        &root.join("format.json"),
        br#"{"schemaVersion":99,"relationalSchema":14}"#,
    )
    .unwrap();
    assert!(s.snapshot().is_err());
}
