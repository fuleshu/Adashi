use super::*;

fn task(store: &mut dyn ProjectStorage, operation: &str) -> api::tasks::Task {
    let result = store
        .commit(Mutation {
            operation_id: operation.into(),
            changes: vec![Change::Task(api::TaskWrite::Create {
                input: serde_json::from_value(serde_json::json!({"title": operation})).unwrap(),
            })],
        })
        .unwrap();
    let ChangeOutcome::Task(Some(task)) = result.outcomes.into_iter().next().unwrap() else {
        panic!("expected task");
    };
    task
}

#[test]
fn task_numbers_are_derived_and_ids_survive_clients_retries_and_deletion() {
    let dir = tempfile::tempdir().unwrap();
    let req = request(dir.path());
    let mut a = store(&req);
    let mut b = store(&req);
    let first = task(&mut a, "first-task");
    let second = task(&mut b, "second-task");
    assert_eq!((first.number, second.number), (1, 2));
    assert_ne!(first.id, second.id);
    assert!(first.id > 1 && second.id <= MAX_SAFE);
    let before = journal::inventory(Path::new(&req.location)).unwrap();
    assert_eq!(task(&mut a, "first-task").id, first.id);
    assert_eq!(
        journal::inventory(Path::new(&req.location)).unwrap(),
        before
    );
    for r in engine::parse_records(&before)
        .unwrap()
        .values()
        .filter(|r| r.collection == "agent_tasks" && !r.deleted)
    {
        assert!(!r.data.contains_key("number"));
    }
    a.commit(Mutation {
        operation_id: "delete-first".into(),
        changes: vec![Change::Task(api::TaskWrite::Delete {
            id: first.id,
            expected_version: first.version,
        })],
    })
    .unwrap();
    let snapshot = b.snapshot().unwrap();
    let current = snapshot.task_by_number(1).unwrap();
    assert_eq!(
        (current.id, current.number, current.version),
        (second.id, 1, second.version)
    );
    assert!(snapshot.task_by_number(2).is_err());
    drop(snapshot);
    let third = task(&mut a, "third");
    assert_ne!(third.id, first.id);
    assert_eq!(third.number, 2);
}

/// External Git timestamps deliberately put a larger permanent ID first.
fn stamp(req: &api::OpenRequest, id: i64, date: &str) {
    let root = Path::new(&req.location);
    let records = engine::parse_records(&journal::inventory(root).unwrap()).unwrap();
    let mut record = records
        .values()
        .find(|r| {
            r.collection == "agent_tasks" && r.data.get("id").and_then(Value::as_i64) == Some(id)
        })
        .unwrap()
        .clone();
    record.data.insert("created_at".into(), date.into());
    journal::atomic_write(
        &root.join(format!("records/agent_tasks/{}.json", record.identity)),
        &codec::bytes(&record).unwrap(),
    )
    .unwrap();
}

#[test]
fn independent_tasks_merge_and_numbers_follow_creation_before_filters_and_pages() {
    let a_dir = tempfile::tempdir().unwrap();
    let b_dir = tempfile::tempdir().unwrap();
    let ar = request(a_dir.path());
    let br = request(b_dir.path());
    let mut a = store(&ar);
    let first = task(&mut a, "base");
    stamp(&ar, first.id, "2026-09-01 00:00:00");
    let base = portable(&journal::inventory(Path::new(&ar.location)).unwrap());
    write_files(Path::new(&br.location), &base);
    let mut b = store(&br);
    let left = task(&mut a, "left");
    let right = task(&mut b, "right");
    assert_ne!(left.id, right.id);
    assert_eq!(left.number, right.number);
    stamp(&ar, left.id, "2026-09-03 00:00:00");
    stamp(&br, right.id, "2026-09-02 00:00:00");
    let bf = portable(&journal::inventory(Path::new(&br.location)).unwrap());
    for (path, bytes) in bf.iter().filter(|(p, v)| base.get(*p) != Some(*v)) {
        journal::atomic_write(&Path::new(&ar.location).join(path), bytes).unwrap();
    }
    let before = portable(&journal::inventory(Path::new(&ar.location)).unwrap());
    let snapshot = a.snapshot().unwrap();
    let all = snapshot.tasks(&api::tasks::ALL_TASK_STATES).unwrap();
    assert_eq!(
        all.iter().map(|t| (t.id, t.number)).collect::<Vec<_>>(),
        vec![(first.id, 1), (right.id, 2), (left.id, 3)]
    );
    let mut query = api::TaskQuery {
        states: vec![api::tasks::TaskState::Todo],
        after_id: None,
        limit: 1,
    };
    for expected in &all {
        let page = snapshot.task_page(&query).unwrap();
        assert_eq!(
            (page.tasks[0].id, page.tasks[0].number),
            (expected.id, expected.number)
        );
        assert_eq!(
            snapshot.task_by_number(expected.number).unwrap().id,
            expected.id
        );
        query.after_id = Some(expected.id);
    }
    assert!(snapshot.task_page(&query).unwrap().tasks.is_empty());
    drop(snapshot);
    assert_eq!(
        portable(&journal::inventory(Path::new(&ar.location)).unwrap()),
        before
    );
    let first = a.snapshot().unwrap().task(first.id).unwrap();
    a.commit(Mutation {
        operation_id: "activate-first".into(),
        changes: vec![Change::Task(api::TaskWrite::Update {
            expected_version: first.version,
            input: serde_json::from_value(serde_json::json!({"taskId":first.id,"state":"active"}))
                .unwrap(),
        })],
    })
    .unwrap();
    let snapshot = a.snapshot().unwrap();
    let filtered = snapshot.tasks(&[api::tasks::TaskState::Todo]).unwrap();
    assert_eq!(
        filtered.iter().map(|t| t.number).collect::<Vec<_>>(),
        vec![2, 3]
    );
}

#[test]
fn legacy_task_numbers_are_ignored_without_read_churn_and_removed_on_write() {
    let dir = tempfile::tempdir().unwrap();
    let req = request(dir.path());
    let mut s = store(&req);
    let a = task(&mut s, "one");
    task(&mut s, "two");
    let root = Path::new(&req.location);
    let mut files = journal::inventory(root).unwrap();
    let mut format: codec::Format = codec::parse(&files["format.json"], "format.json").unwrap();
    format.schema_version = 2;
    files.insert("format.json".into(), codec::bytes(&format).unwrap());
    for (path, bytes) in files
        .iter_mut()
        .filter(|(p, _)| p.starts_with("records/agent_tasks/"))
    {
        let mut record: codec::Record = codec::parse(bytes, path).unwrap();
        record.data.insert("number".into(), 5.into()); // Previously a merged-number conflict.
        *bytes = codec::bytes(&record).unwrap();
    }
    write_files(root, &files);
    assert_eq!(s.snapshot().unwrap().task_by_number(1).unwrap().id, a.id);
    assert_eq!(journal::inventory(root).unwrap(), files);
    create(&mut s, "upgrade", "Upgrade");
    let files = journal::inventory(root).unwrap();
    assert_eq!(
        codec::parse::<codec::Format>(&files["format.json"], "format.json")
            .unwrap()
            .schema_version,
        3
    );
    assert!(engine::parse_records(&files)
        .unwrap()
        .values()
        .filter(|r| r.collection == "agent_tasks")
        .all(|r| !r.data.contains_key("number")));
}
