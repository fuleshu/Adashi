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
fn task_ids_and_numbers_are_sequential_across_clients_and_retries() {
    let dir = tempfile::tempdir().unwrap();
    let req = request(dir.path());
    let mut a = store(&req);
    let mut b = store(&req);
    let first = task(&mut a, "first-task");
    let second = task(&mut b, "second-task");
    assert_eq!((first.id, first.number), (1, 1));
    assert_eq!((second.id, second.number), (2, 2));
    let before = journal::inventory(Path::new(&req.location)).unwrap();
    assert_eq!(task(&mut a, "first-task").id, first.id);
    assert_eq!(
        journal::inventory(Path::new(&req.location)).unwrap(),
        before
    );
    drop(a);
    drop(b);
    let mut reopened = store(&req);
    let third = task(&mut reopened, "third-task");
    assert_eq!((third.id, third.number), (3, 3));
}

#[test]
fn task_ids_do_not_reuse_deleted_aliases() {
    let dir = tempfile::tempdir().unwrap();
    let req = request(dir.path());
    let mut s = store(&req);
    let first = task(&mut s, "first");
    let second = task(&mut s, "second");
    s.commit(Mutation {
        operation_id: "delete-second".into(),
        changes: vec![Change::Task(api::TaskWrite::Delete {
            id: second.id,
            expected_version: second.version,
        })],
    })
    .unwrap();
    let next = task(&mut s, "next");
    assert_eq!(next.id, second.id + 1);
    assert_eq!(next.number, first.number + 1);
}

#[test]
fn independent_task_number_collisions_fail_without_reassigning_identity() {
    let a_dir = tempfile::tempdir().unwrap();
    let b_dir = tempfile::tempdir().unwrap();
    let ar = request(a_dir.path());
    let br = request(b_dir.path());
    let mut a = store(&ar);
    task(&mut a, "base");
    let base = portable(&journal::inventory(Path::new(&ar.location)).unwrap());
    write_files(Path::new(&br.location), &base);
    let mut b = store(&br);
    let left = task(&mut a, "left");
    let right = task(&mut b, "right");
    assert_eq!((left.id, left.number), (right.id, right.number));
    let bf = portable(&journal::inventory(Path::new(&br.location)).unwrap());
    for (path, bytes) in bf.iter().filter(|(p, v)| base.get(*p) != Some(*v)) {
        journal::atomic_write(&Path::new(&ar.location).join(path), bytes).unwrap();
    }
    let before = journal::inventory(Path::new(&ar.location)).unwrap();
    let error = a
        .snapshot()
        .err()
        .expect("duplicate task aliases must fail");
    assert!(error.to_string().contains("duplicate"), "{error}");
    assert_eq!(journal::inventory(Path::new(&ar.location)).unwrap(), before);
}
