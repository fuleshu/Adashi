//! Compatibility checks for retiring tracked request history without read churn.
use super::*;
use crate::storage::transfer::{MigrationAdapter, ProjectImage};

fn legacy_files(req: &api::OpenRequest, receipt: Value) -> (Files, String) {
    let mut files = journal::inventory(Path::new(&req.location)).unwrap();
    let records = engine::parse_records(&files).unwrap();
    let project = records
        .values()
        .find(|r| r.collection == "projects")
        .unwrap();
    let identity = uuid::Uuid::new_v4().to_string();
    let path = format!("records/mutation_operations/{identity}.json");
    let record = codec::Record {
        schema_version: 1,
        collection: "mutation_operations".into(),
        identity,
        deleted: false,
        data: BTreeMap::from([
            (
                "project_id".into(),
                serde_json::json!({"ref": project.identity}),
            ),
            ("operation_id".into(), "first".into()),
            (
                "result_json".into(),
                serde_json::to_string(&receipt).unwrap().into(),
            ),
            ("created_at".into(), "2026-09-23 12:00:00".into()),
        ]),
    };
    files.insert(path.clone(), codec::bytes(&record).unwrap());
    files.insert(
        "format.json".into(),
        codec::bytes(&codec::Format {
            schema_version: 1,
            relational_schema: sqlite::schema::SCHEMA_VERSION,
        })
        .unwrap(),
    );
    let mut local: Value = codec::parse(&files[local::PATH], local::PATH).unwrap();
    local["receipts"] = serde_json::json!({});
    files.insert(local::PATH.into(), codec::bytes(&local).unwrap());
    (files, path)
}

#[test]
fn legacy_history_moves_local_on_write_and_interrupted_removal_recovers() {
    let dir = tempfile::tempdir().unwrap();
    let req = request(dir.path());
    let root = Path::new(&req.location);
    let local = root.parent().unwrap().join("local");
    let mut s = store(&req);
    let rule = create(&mut s, "first", "First");
    let receipt =
        serde_json::to_value(s.snapshot().unwrap().receipt("first").unwrap().unwrap()).unwrap();
    let (before, history_path) = legacy_files(&req, receipt.clone());
    write_files(root, &before);
    drop(s);

    let mut s = store(&req);
    assert_eq!(s.snapshot().unwrap().rules().unwrap()[0].name, "First");
    assert_eq!(
        journal::inventory(root).unwrap(),
        before,
        "legacy reads must not upgrade files"
    );

    // The first successful write upgrades the format, even if its domain change is a no-op.
    s.commit(update(&rule, "upgrade", &rule.prompt)).unwrap();
    let after = journal::inventory(root).unwrap();
    assert!(!after.contains_key(&history_path));
    assert_eq!(
        codec::parse::<codec::Format>(&after["format.json"], "format.json")
            .unwrap()
            .schema_version,
        2
    );
    for (path, bytes) in portable(&before) {
        if path != "format.json" && path != history_path {
            assert_eq!(after[&path], bytes, "upgrade changed domain file {path}");
        }
    }
    assert_eq!(
        serde_json::to_value(s.snapshot().unwrap().receipt("first").unwrap().unwrap()).unwrap(),
        receipt
    );
    assert_eq!(create(&mut s, "first", "First").id, rule.id);
    assert_eq!(journal::inventory(root).unwrap(), after);
    s.commit(update(&rule, "another-noop", &rule.prompt))
        .unwrap();
    assert_eq!(
        portable(&journal::inventory(root).unwrap()),
        portable(&after)
    );

    // Recovery checks the deletion preimage before touching any other journaled file.
    write_files(root, &before);
    journal::stage_for_test(&local, &before, &after).unwrap();
    fs::write(root.join(&history_path), b"external edit").unwrap();
    let blocked = journal::inventory(root).unwrap();
    assert!(s
        .snapshot()
        .err()
        .unwrap()
        .to_string()
        .contains("external edit"));
    assert_eq!(journal::inventory(root).unwrap(), blocked);
    fs::write(root.join(&history_path), &before[&history_path]).unwrap();
    // Simulate death after removing the history file but before completing publication.
    fs::remove_file(root.join(&history_path)).unwrap();
    s.snapshot().unwrap();
    assert_eq!(journal::inventory(root).unwrap(), after);
    assert!(!local.join("text-transaction.json").exists());
}

#[test]
fn migration_normalizes_legacy_history_and_v2_rejects_shared_requests() {
    let dir = tempfile::tempdir().unwrap();
    let req = request(dir.path());
    let mut s = store(&req);
    create(&mut s, "first", "First");
    let receipt =
        serde_json::to_value(s.snapshot().unwrap().receipt("first").unwrap().unwrap()).unwrap();
    let (files, history_path) = legacy_files(&req, receipt);
    let legacy = ProjectImage {
        schema_version: 1,
        files,
    };
    let destination = tempfile::tempdir().unwrap();
    let adapter = transfer::TextMigrationAdapter;
    adapter.stage(&legacy, destination.path(), "test").unwrap();
    let upgraded = adapter.inspect_staged(destination.path()).unwrap();
    assert!(!upgraded.files.contains_key(&history_path));
    assert_eq!(
        legacy.semantic_fingerprint().unwrap(),
        upgraded.semantic_fingerprint().unwrap()
    );
    assert_eq!(legacy.counts().unwrap(), upgraded.counts().unwrap());
    let mut invalid = upgraded.clone();
    invalid
        .files
        .insert(history_path.clone(), legacy.files[&history_path].clone());
    assert!(invalid
        .validate()
        .unwrap_err()
        .to_string()
        .contains("request history belongs"));
}
