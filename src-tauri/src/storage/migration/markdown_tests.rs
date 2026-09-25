//! Complete Markdown images pass the real conversion and backup protocol.
use super::*;
use crate::storage::{api::*, ProjectStore};
use serde_json::{json, Value};

fn design(
    store: &mut dyn ProjectStorage,
    op: &str,
    changes: Value,
    read_tokens: Vec<documents::DocumentReadToken>,
) {
    store
        .commit(Mutation {
            operation_id: op.into(),
            changes: vec![Change::Design(DesignWrite::Save {
                change_intent: "Migration fixture".into(),
                changes: serde_json::from_value(changes).unwrap(),
                read_tokens,
            })],
        })
        .unwrap();
}
fn token(store: &mut dyn ProjectStorage, id: &str) -> documents::DocumentReadToken {
    let doc = store
        .snapshot()
        .unwrap()
        .design_documents(&[format!("markdown:{id}")])
        .unwrap()
        .remove(0);
    documents::DocumentReadToken {
        document_id: doc.document_id,
        read_token: doc.read_token,
    }
}
pub(super) fn populate(store: &mut dyn ProjectStorage) {
    design(
        store,
        "markdown-create",
        json!([
            {"op":"upsert_markdown","externalId":"project-notes","title":"Original title","body":"# Überblick 日本語\n\n```rust\n  let x = 1;\n```\n","designLinks":[]},
            {"op":"upsert_markdown","externalId":"component-spec","title":"Component design","body":"Complete body\r\n","designLinks":[{"targetType":"element","designExternalId":"app"},{"targetType":"markdown","designExternalId":"project-notes"}]},
            {"op":"upsert_markdown","externalId":"deleted-prose","title":"Delete me","body":"Historical identity","designLinks":[]},
            {"op":"upsert_binding","designExternalId":"component-spec","targetType":"file","target":"src/main.rs"}
        ]),
        vec![],
    );
    let renamed = token(store, "project-notes");
    let deleted = token(store, "deleted-prose");
    design(
        store,
        "markdown-rename-delete",
        json!([
            {"op":"upsert_markdown","externalId":"project-notes","title":"Renamed title","body":"# Überblick 日本語\n\n```rust\n  let x = 1;\n```\n","designLinks":[]},
            {"op":"delete_markdown","externalId":"deleted-prose"}
        ]),
        vec![renamed, deleted],
    );
    store.commit(Mutation {operation_id:"markdown-links".into(),changes:vec![
        Change::Task(TaskWrite::Create {input:serde_json::from_value(json!({"title":"Markdown task","designSpecificationLinks":[{"targetType":"markdown","designExternalId":"component-spec"}]})).unwrap()}),
        Change::Qa(QaWrite::CreateJob {input:serde_json::from_value(json!({"name":"Markdown QA","command":"echo test","enabled":false,"designSpecificationLinks":[{"targetType":"markdown","designExternalId":"component-spec"}]})).unwrap()})
    ]}).unwrap();
}

#[test]
fn markdown_conversion_preserves_complete_content_identities_references_and_backups() {
    let (root, project) = super::tests::fixture();
    let mut store = ProjectStore::open(&project).unwrap();
    let snapshots = store
        .snapshot()
        .unwrap()
        .design_documents(&[
            "markdown:project-notes".into(),
            "markdown:component-spec".into(),
            "markdown:deleted-prose".into(),
        ])
        .unwrap();
    let backlinks = serde_json::to_value(
        store
            .snapshot()
            .unwrap()
            .markdown_backlinks("component-spec")
            .unwrap(),
    )
    .unwrap();
    drop(store);
    let original = adapter("sqlite")
        .unwrap()
        .capture(&project, false)
        .unwrap()
        .image()
        .clone();
    assert_eq!(original.counts().unwrap()["markdown_design_documents"], 2);
    assert_eq!(original.counts().unwrap()["markdown_design_links"], 2);
    fs::write(
        root.path().join("generated-design.md"),
        "DERIVED_OUTPUT_CANARY",
    )
    .unwrap();
    fs::write(
        root.path().join(".adashi/local/markdown-projection.json"),
        "LOCAL_OUTPUT_CANARY",
    )
    .unwrap();
    let image = adapter("sqlite")
        .unwrap()
        .capture(&project, false)
        .unwrap()
        .image()
        .clone();
    assert_eq!(
        original.fingerprint().unwrap(),
        image.fingerprint().unwrap()
    );
    for target in ["text", "sqlite", "text"] {
        let plan = preview(&project, target).unwrap();
        let report = convert(&project, target, &plan.plan_token, true).unwrap();
        assert!(Path::new(&report.backup_folder)
            .join("source/.adashi/storage.json")
            .exists());
        let image = adapter(target)
            .unwrap()
            .capture(&project, false)
            .unwrap()
            .image()
            .clone();
        assert_eq!(
            original.semantic_fingerprint().unwrap(),
            image.semantic_fingerprint().unwrap()
        );
        assert_eq!(original.counts().unwrap(), image.counts().unwrap());
        let records = |image: &crate::storage::transfer::ProjectImage| {
            image
                .files
                .iter()
                .filter(|(p, _)| p.starts_with("records/markdown_design_"))
                .map(|(p, b)| (p.clone(), serde_json::from_slice::<Value>(b).unwrap()))
                .collect::<BTreeMap<_, _>>()
        };
        assert_eq!(records(&original), records(&image));
        let mut store = ProjectStore::open(&project).unwrap();
        for expected in &snapshots {
            let actual = store
                .snapshot()
                .unwrap()
                .design_documents(&[expected.document_id.clone()])
                .unwrap()
                .remove(0);
            assert_eq!(actual.document, expected.document);
            assert_eq!(actual.read_token, expected.read_token);
        }
        assert_eq!(
            serde_json::to_value(
                store
                    .snapshot()
                    .unwrap()
                    .markdown_backlinks("component-spec")
                    .unwrap()
            )
            .unwrap(),
            backlinks
        );
        assert!(store
            .snapshot()
            .unwrap()
            .markdown_document("deleted-prose")
            .is_err());
    }
    assert_eq!(
        fs::read_to_string(root.path().join("generated-design.md")).unwrap(),
        "DERIVED_OUTPUT_CANARY"
    );
}
