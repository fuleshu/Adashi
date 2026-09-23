use std::fs;

use super::conformance::{draft, mutation};
use super::*;
use crate::settings::{self, ProjectSettings};

struct Fixture {
    _root: tempfile::TempDir,
    project: ProjectSettings,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let project = ProjectSettings {
            id: "storage-fixture".into(),
            name: "Storage fixture".into(),
            folder: root.path().join("project").to_string_lossy().into_owned(),
        };
        Self {
            _root: root,
            project,
        }
    }

    fn descriptor(&self, text: &str) {
        fs::create_dir_all(settings::project_data_dir(&self.project)).unwrap();
        fs::write(
            settings::project_data_dir(&self.project).join("storage.json"),
            text,
        )
        .unwrap();
    }

    fn open(&self) -> ProjectStore {
        ProjectStore::open_for_computer(&self.project, "fixture-computer").unwrap()
    }
}

#[test]
fn sqlite_runs_shared_conformance() {
    let fixture = Fixture::new();
    conformance::assert_rules_contract(|| Box::new(fixture.open()));
}

#[test]
fn legacy_and_explicit_sqlite_select_the_same_store_without_materializing_defaults() {
    let fixture = Fixture::new();
    let mut legacy = fixture.open();
    assert_eq!(legacy.descriptor_source(), DescriptorSource::LegacyDefault);
    assert_eq!(legacy.descriptor(), &StorageDescriptor::default());
    let identity = legacy.rules_snapshot().unwrap().project;
    assert!(!settings::project_data_dir(&fixture.project)
        .join("storage.json")
        .exists());
    drop(legacy);
    fixture.descriptor(r#"{"schemaVersion":1,"backend":{"kind":"sqlite"}}"#);
    let mut explicit = fixture.open();
    assert_eq!(explicit.descriptor_source(), DescriptorSource::ProjectFile);
    assert_eq!(explicit.rules_snapshot().unwrap().project, identity);
}

#[test]
fn invalid_and_unavailable_descriptors_never_create_a_database() {
    let cases = [
        ("", "storage.invalid_configuration"),
        ("{}", "storage.invalid_configuration"),
        (
            r#"{"schemaVersion":2,"backend":{"kind":"sqlite"}}"#,
            "storage.invalid_configuration",
        ),
        (
            r#"{"schemaVersion":1,"backend":{"kind":"unknown"}}"#,
            "storage.invalid_configuration",
        ),
        (
            r#"{"schemaVersion":1,"backend":{"kind":"sqlite","password":"secret"}}"#,
            "storage.invalid_configuration",
        ),
        (
            r#"{"schemaVersion":1,"schemaVersion":1,"backend":{"kind":"sqlite"}}"#,
            "storage.invalid_configuration",
        ),
        (
            r#"{"schemaVersion":1,"backend":{"kind":"sqlite"},"extra":1}"#,
            "storage.invalid_configuration",
        ),
        (
            r#"{"schemaVersion":1,"backend":{"kind":"serverSql","connectionProfile":"postgres://secret","namespace":"p"}}"#,
            "storage.invalid_configuration",
        ),
        (
            r#"{"schemaVersion":1,"backend":{"kind":"serverSql","connectionProfile":"team"}}"#,
            "storage.invalid_configuration",
        ),
        (
            r#"{"schemaVersion":1,"backend":{"kind":"text"}}"#,
            "storage.backend_unavailable",
        ),
        (
            r#"{"schemaVersion":1,"backend":{"kind":"serverSql","connectionProfile":"team","namespace":"p"}}"#,
            "storage.backend_unavailable",
        ),
    ];
    for (text, code) in cases {
        let fixture = Fixture::new();
        fixture.descriptor(text);
        let error = match ProjectStore::open_for_computer(&fixture.project, "fixture-computer") {
            Ok(_) => panic!("descriptor must fail: {text}"),
            Err(error) => error,
        };
        assert_eq!(error.code(), code, "{text}: {error}");
        assert!(!error.to_string().contains("secret"));
        assert!(!settings::project_database_path(&fixture.project).exists());
        assert_eq!(
            fs::read_to_string(settings::project_data_dir(&fixture.project).join("storage.json"))
                .unwrap(),
            text
        );
        assert_eq!(
            fs::read_dir(settings::project_data_dir(&fixture.project))
                .unwrap()
                .count(),
            1
        );
    }
}

#[test]
fn project_selection_is_independent_and_invalid_selection_cannot_modify_existing_data() {
    let first = Fixture::new();
    let second = Fixture::new();
    drop(first.open());
    let path = settings::project_database_path(&first.project);
    let before = fs::read(&path).unwrap();
    first.descriptor(r#"{"schemaVersion":1,"backend":{"kind":"text"}}"#);
    assert!(matches!(
        ProjectStore::open_for_computer(&first.project, "fixture-computer"),
        Err(StorageError::BackendUnavailable(_))
    ));
    assert_eq!(fs::read(&path).unwrap(), before);
    let mut available = second.open();
    assert_eq!(
        available.rules_snapshot().unwrap().project.id,
        second.project.id
    );
}

#[test]
fn initialized_reads_preserve_database_bytes_mtime_and_identity() {
    let fixture = Fixture::new();
    let mut store = fixture.open();
    let original = store.rules_snapshot().unwrap();
    drop(store);
    let path = settings::project_database_path(&fixture.project);
    let bytes = fs::read(&path).unwrap();
    let mtime = fs::metadata(&path).unwrap().modified().unwrap();
    let mut alias = fixture.project.clone();
    alias.id = "local-alias".into();
    alias.name = "Other local name".into();
    for _ in 0..3 {
        let mut reopened = ProjectStore::open_for_computer(&alias, "fixture-computer").unwrap();
        let snapshot = reopened.rules_snapshot().unwrap();
        assert_eq!(snapshot.project, original.project);
        assert_eq!(snapshot.cursor, original.cursor);
        drop(reopened);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), mtime);
    }
}

#[test]
fn storage_failure_rolls_back_the_entire_batch_and_receipt() {
    let fixture = Fixture::new();
    let db = sqlite::open_test_database(&fixture.project, "fixture-computer").unwrap();
    db.execute_batch("CREATE TRIGGER fail_second_rule BEFORE INSERT ON rules WHEN NEW.name='Fail' BEGIN SELECT RAISE(ABORT, 'fixture failure'); END;").unwrap();
    drop(db);
    let mut store = fixture.open();
    let before = serde_json::to_value(store.rules_snapshot().unwrap()).unwrap();
    let change = mutation(
        "rollback",
        vec![
            RuleChange::Create {
                rule: draft("Valid", "first"),
            },
            RuleChange::Create {
                rule: draft("Fail", "second"),
            },
        ],
    );
    assert!(matches!(
        store.mutate_rules(change.clone()),
        Err(StorageError::Backend(_))
    ));
    assert_eq!(
        serde_json::to_value(store.rules_snapshot().unwrap()).unwrap(),
        before
    );
    drop(store);
    let db = sqlite::open_test_database(&fixture.project, "fixture-computer").unwrap();
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM mutation_operations WHERE operation_id='rollback'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    db.execute_batch("DROP TRIGGER fail_second_rule").unwrap();
    drop(db);
    assert_eq!(
        fixture.open().mutate_rules(change).unwrap().outcomes.len(),
        2
    );
}

#[test]
fn concurrent_identical_requests_commit_once() {
    let fixture = Fixture::new();
    drop(fixture.open());
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles = (0..2)
        .map(|_| {
            let project = fixture.project.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut store =
                    ProjectStore::open_for_computer(&project, "fixture-computer").unwrap();
                barrier.wait();
                store
                    .mutate_rules(mutation(
                        "concurrent-replay",
                        vec![RuleChange::Create {
                            rule: draft("Once", "once"),
                        }],
                    ))
                    .unwrap()
            })
        })
        .collect::<Vec<_>>();
    let results = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        serde_json::to_value(&results[0]).unwrap(),
        serde_json::to_value(&results[1]).unwrap()
    );
    assert_eq!(fixture.open().rules_snapshot().unwrap().rules.len(), 1);
}

#[test]
fn snapshots_do_not_mix_concurrent_batch_versions() {
    let fixture = Fixture::new();
    let mut reader = fixture.open();
    reader
        .mutate_rules(mutation(
            "initial",
            vec![
                RuleChange::Create {
                    rule: draft("A", "0"),
                },
                RuleChange::Create {
                    rule: draft("B", "0"),
                },
            ],
        ))
        .unwrap();
    let project = fixture.project.clone();
    let writer = std::thread::spawn(move || {
        let mut writer = ProjectStore::open_for_computer(&project, "fixture-computer").unwrap();
        for i in 1..=30 {
            let snapshot = writer.rules_snapshot().unwrap();
            let changes = snapshot
                .rules
                .iter()
                .map(|rule| RuleChange::Update {
                    id: rule.id,
                    expected_version: rule.version,
                    rule: draft(&rule.name, &i.to_string()),
                })
                .collect();
            writer
                .mutate_rules(mutation(&format!("batch-{i}"), changes))
                .unwrap();
        }
    });
    for _ in 0..100 {
        let snapshot = reader.rules_snapshot().unwrap();
        assert_eq!(snapshot.rules.len(), 2);
        assert_eq!(snapshot.rules[0].prompt, snapshot.rules[1].prompt);
        assert_eq!(snapshot.rules[0].version, snapshot.rules[1].version);
    }
    writer.join().unwrap();
}

#[test]
fn future_sqlite_schema_is_rejected_without_changes() {
    let fixture = Fixture::new();
    let db = sqlite::open_test_database(&fixture.project, "fixture-computer").unwrap();
    db.pragma_update(None, "user_version", crate::schema::SCHEMA_VERSION + 1)
        .unwrap();
    drop(db);
    let path = settings::project_database_path(&fixture.project);
    let before = fs::read(&path).unwrap();
    assert!(matches!(
        ProjectStore::open_for_computer(&fixture.project, "fixture-computer"),
        Err(StorageError::IncompatibleSchema { .. })
    ));
    assert_eq!(fs::read(path).unwrap(), before);
}
