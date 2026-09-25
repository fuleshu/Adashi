//! Exercise Markdown through the same public boundary used by both clients.
use super::*;
use adashi_storage_api::{documents::DocumentReadToken, markdown::MarkdownQuery};
use serde_json::{json, Value};

fn token(store: &mut dyn ProjectStorage, id: &str) -> DocumentReadToken {
    let doc = store
        .snapshot()
        .unwrap()
        .design_documents(&[format!("markdown:{id}")])
        .unwrap()
        .remove(0);
    DocumentReadToken {
        document_id: doc.document_id,
        read_token: doc.read_token,
    }
}
fn save(operation: &str, changes: Value, tokens: Vec<DocumentReadToken>) -> Mutation {
    mutation(
        operation,
        vec![Change::Design(DesignWrite::Save {
            change_intent: "Markdown conformance".into(),
            changes: serde_json::from_value(changes).unwrap(),
            read_tokens: tokens,
        })],
    )
}
fn upsert(id: &str, body: &str) -> Value {
    json!({"op":"upsert_markdown","externalId":id,"title":format!("Title {id}"),"body":body,"designLinks":[]})
}

#[test]
fn exact_bodies_tokens_replay_noops_and_rollback() {
    let (_root, request, mut store) = fixture();
    let body = "# Überlegung 日本語\n\n```rust\n  let x = 1;  \n```\n".repeat(4000);
    let create = save(
        "create",
        json!([upsert("a", &body), upsert("b", "second")]),
        vec![],
    );
    let result = store.commit(create.clone()).unwrap();
    assert_eq!(
        store
            .snapshot()
            .unwrap()
            .markdown_document("a")
            .unwrap()
            .body,
        body
    );
    assert_eq!(
        serde_json::to_value(store.commit(create).unwrap()).unwrap(),
        serde_json::to_value(result).unwrap()
    );
    assert!(matches!(
        store.commit(save(
            "create",
            json!([upsert("other", "different")]),
            vec![]
        )),
        Err(StorageError::OperationReused)
    ));
    let a = token(&mut store, "a");
    let b = token(&mut store, "b");
    let before = store.snapshot().unwrap().metadata().revision;
    store
        .commit(save("no-op", json!([upsert("a", &body)]), vec![a.clone()]))
        .unwrap();
    assert_eq!(store.snapshot().unwrap().metadata().revision, before);
    assert_eq!(a.read_token, token(&mut store, "a").read_token);
    store
        .commit(save(
            "update-a",
            json!([upsert("a", "new")]),
            vec![a.clone()],
        ))
        .unwrap();
    store
        .commit(save(
            "disjoint",
            json!([upsert("b", "disjoint")]),
            vec![b.clone()],
        ))
        .unwrap();
    assert!(store
        .commit(save("stale", json!([upsert("a", "lost")]), vec![a.clone()]))
        .is_err());
    assert!(store
        .commit(save(
            "stale-delete",
            json!([{"op":"delete_markdown","externalId":"a"}]),
            vec![a]
        ))
        .is_err());
    assert!(store
        .commit(save("duplicate", json!([upsert("a", "unguarded")]), vec![]))
        .is_err());
    let mut invalid = upsert("bad", "must roll back");
    invalid["designLinks"] = json!([{"targetType":"markdown","designExternalId":"missing"}]);
    assert!(store
        .commit(save(
            "bad-ref",
            json!([upsert("temporary", "rollback"), invalid]),
            vec![]
        ))
        .is_err());
    assert!(store
        .snapshot()
        .unwrap()
        .markdown_document("temporary")
        .is_err());
    assert!(store
        .snapshot()
        .unwrap()
        .receipt("bad-ref")
        .unwrap()
        .is_none());
    let page = store
        .snapshot()
        .unwrap()
        .markdown_documents(&MarkdownQuery {
            limit: 1,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(page.documents.len(), 1);
    assert_eq!(page.next_after_id, Some("a".into()));
    let page = store
        .snapshot()
        .unwrap()
        .markdown_documents(&MarkdownQuery {
            query: Some("disjoint".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(page.documents[0].external_id, "b");
    store.close().unwrap();
    let bytes = std::fs::read(&request.location).unwrap();
    let mtime = std::fs::metadata(&request.location)
        .unwrap()
        .modified()
        .unwrap();
    let mut reader = StorageClient::new(
        SqliteFactory
            .open(&OpenRequest {
                mode: OpenMode::ReadOnly,
                ..request.clone()
            })
            .unwrap(),
    );
    reader.snapshot().unwrap().markdown_document("a").unwrap();
    reader
        .snapshot()
        .unwrap()
        .markdown_documents(&Default::default())
        .unwrap();
    reader.close().unwrap();
    assert_eq!(bytes, std::fs::read(&request.location).unwrap());
    assert_eq!(
        mtime,
        std::fs::metadata(&request.location)
            .unwrap()
            .modified()
            .unwrap()
    );
}

#[test]
fn associations_bindings_delete_protection_and_atomic_detachment() {
    let (_root, _request, mut store) = fixture();
    let mut b = upsert("b", "B");
    b["designLinks"] = json!([{"targetType":"markdown","designExternalId":"a"},{"targetType":"element","designExternalId":"1"}]);
    store.commit(save("initial",json!([upsert("a","A"),b.clone(),{"op":"upsert_binding","designExternalId":"a","targetType":"file","target":"src/main.rs"}]),vec![])).unwrap();
    assert_eq!(
        serde_json::to_value(
            store
                .snapshot()
                .unwrap()
                .markdown_document("b")
                .unwrap()
                .design_links
        )
        .unwrap(),
        b["designLinks"]
    );
    assert_eq!(
        store
            .snapshot()
            .unwrap()
            .markdown_backlinks("a")
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        store
            .snapshot()
            .unwrap()
            .markdown_documents(&MarkdownQuery {
                file: Some("src/main.rs".into()),
                ..Default::default()
            })
            .unwrap()
            .documents[0]
            .external_id,
        "a"
    );
    let a = token(&mut store, "a");
    assert!(store
        .commit(save(
            "blocked-delete",
            json!([{"op":"delete_markdown","externalId":"a"}]),
            vec![a.clone()]
        ))
        .is_err());
    let binding = store
        .snapshot()
        .unwrap()
        .design_documents(&["binding:a|file|src/main.rs".into()])
        .unwrap()
        .remove(0);
    let btoken = token(&mut store, "b");
    store.commit(save("detach-delete",json!([upsert("b","Retained prose"),{"op":"delete_binding","designExternalId":"a","targetType":"file","target":"src/main.rs"},{"op":"delete_markdown","externalId":"a"}]),vec![a,btoken,DocumentReadToken {document_id:binding.document_id,read_token:binding.read_token}])).unwrap();
    assert!(store.snapshot().unwrap().markdown_document("a").is_err());
    assert_eq!(
        store
            .snapshot()
            .unwrap()
            .markdown_document("b")
            .unwrap()
            .body,
        "Retained prose"
    );
}

#[test]
fn migration_preserves_old_data_and_project_isolation() {
    let (_root, request, mut store) = fixture();
    store.commit(mutation("task", vec![new_task()])).unwrap();
    store.close().unwrap();
    let mut db = Connection::open(&request.location).unwrap();
    // Reconstruct the prior CHECK constraints and consumed (empty-table) sequences.
    for table in ["task_design_specification_links", "qa_job_design_links"] {
        let sql: String = db
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name=?1",
                [table],
                |r| r.get(0),
            )
            .unwrap();
        db.execute_batch(&format!(
            "DROP TABLE {table}; {}",
            sql.replace(", 'markdown'", "")
        ))
        .unwrap();
        db.execute(
            "INSERT INTO sqlite_sequence(name,seq) VALUES(?1,123)",
            [table],
        )
        .unwrap();
    }
    db.execute_batch("DROP TABLE markdown_design_links; DROP TABLE markdown_design_documents; PRAGMA user_version=14;").unwrap();
    schema::migrate(&mut db).unwrap();
    for table in ["task_design_specification_links", "qa_job_design_links"] {
        assert_eq!(
            db.query_row(
                "SELECT seq FROM sqlite_sequence WHERE name=?1",
                [table],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            123
        );
    }
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM agent_tasks", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM markdown_design_documents", [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap(),
        0
    );
    let project = db
        .query_row("SELECT id FROM projects", [], |r| r.get::<_, i64>(0))
        .unwrap();
    super::super::markdown::upsert(
        &db,
        project,
        &serde_json::from_value(
            json!({"externalId":"isolated","title":"Private","body":"content"}),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(super::super::markdown::get(&db, project + 1, "isolated")
        .unwrap()
        .is_none());
    assert!(
        super::super::markdown::list(&db, project + 1, &Default::default())
            .unwrap()
            .documents
            .is_empty()
    );
}

#[test]
fn task_and_qa_links_resolve_titles_and_prevent_deletion() {
    let (_root, _request, mut store) = fixture();
    store
        .commit(save(
            "document",
            json!([upsert("spec", "Canonical prose")]),
            vec![],
        ))
        .unwrap();
    let link = json!([{"targetType":"markdown","designExternalId":"spec"}]);
    store.commit(mutation("links",vec![
        Change::Task(TaskWrite::Create {input:serde_json::from_value(json!({"title":"Implement","designSpecificationLinks":link})).unwrap()}),
        Change::Qa(QaWrite::CreateJob {input:serde_json::from_value(json!({"name":"Verify","command":"echo test","designSpecificationLinks":link})).unwrap()})
    ])).unwrap();
    assert_eq!(
        store
            .snapshot()
            .unwrap()
            .task(1)
            .unwrap()
            .design_specification_links[0]
            .title,
        "Title spec"
    );
    assert_eq!(
        store
            .snapshot()
            .unwrap()
            .qa_job(1)
            .unwrap()
            .design_specification_links[0]
            .title,
        "Title spec"
    );
    assert_eq!(
        store
            .snapshot()
            .unwrap()
            .markdown_backlinks("spec")
            .unwrap()
            .len(),
        2
    );
    let token = token(&mut store, "spec");
    assert!(store
        .commit(save(
            "delete",
            json!([{"op":"delete_markdown","externalId":"spec"}]),
            vec![token]
        ))
        .is_err());
    assert!(store.snapshot().unwrap().markdown_document("spec").is_ok());
}
