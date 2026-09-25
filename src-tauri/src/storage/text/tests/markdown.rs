//! Markdown compatibility, content guards and recoverable publication.
use super::*;
use api::{documents::DocumentReadToken, DesignWrite};
use serde_json::json;

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
fn save(op: &str, id: &str, body: &str, tokens: Vec<DocumentReadToken>) -> Mutation {
    Mutation { operation_id:op.into(), changes:vec![Change::Design(DesignWrite::Save {change_intent:"Markdown text test".into(), changes:serde_json::from_value(json!([{"op":"upsert_markdown","externalId":id,"title":id,"body":body,"designLinks":[]}])).unwrap(),read_tokens:tokens})] }
}

#[test]
fn old_reads_do_not_upgrade_and_markdown_writes_are_stable_guarded_and_recoverable() {
    let dir = tempfile::tempdir().unwrap();
    let req = request(dir.path());
    let root = Path::new(&req.location);
    let local = root.parent().unwrap().join("local");
    let mut s = store(&req);
    let mut files = journal::inventory(root).unwrap();
    files.insert(
        "format.json".into(),
        codec::bytes(&codec::Format {
            schema_version: 2,
            relational_schema: 14,
        })
        .unwrap(),
    );
    write_files(root, &files);
    assert!(s
        .snapshot()
        .unwrap()
        .markdown_documents(&Default::default())
        .unwrap()
        .documents
        .is_empty());
    assert_eq!(journal::inventory(root).unwrap(), files);
    let body = "# Grüße 日本語\r\n\r\n```rust\n let x = 1;  \n```\n".repeat(100);
    let request = save("create", "spec", &body, vec![]);
    s.commit(request.clone()).unwrap();
    assert_eq!(
        s.snapshot()
            .unwrap()
            .markdown_document("spec")
            .unwrap()
            .body,
        body
    );
    let first = token(&mut s, "spec");
    let before = journal::inventory(root).unwrap();
    let mtimes = portable(&before)
        .keys()
        .map(|p| {
            (
                p.clone(),
                fs::metadata(root.join(p)).unwrap().modified().unwrap(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    s.commit(save("fresh-noop", "spec", &body, vec![first.clone()]))
        .unwrap();
    s.commit(request).unwrap();
    assert_eq!(
        portable(&before),
        portable(&journal::inventory(root).unwrap())
    );
    for (p, t) in mtimes {
        assert_eq!(fs::metadata(root.join(p)).unwrap().modified().unwrap(), t);
    }
    // External Git content edit without bumping a branch counter must invalidate tokens.
    let (path, bytes) = before
        .iter()
        .find(|(p, _)| p.starts_with("records/markdown_design_documents/"))
        .unwrap();
    let mut record: codec::Record = codec::parse(bytes, path).unwrap();
    record
        .data
        .insert("body".into(), codec::encode(&json!("Git edit\n")));
    journal::atomic_write(&root.join(path), &codec::bytes(&record).unwrap()).unwrap();
    assert!(s
        .commit(save("stale", "spec", "lost", vec![first]))
        .is_err());
    // Make timestamp churn observable even when the test finishes in one second.
    for (p,b) in journal::inventory(root).unwrap() {
        if p.starts_with("records/design_workspaces/") || p.starts_with("records/diagrams/") {
            let mut record:codec::Record=codec::parse(&b,&p).unwrap();
            if record.data.contains_key("updated_at") {
                record.data.insert("updated_at".into(),json!("2000-01-01 00:00:00"));
                // Text canonicalization pins derived-source timestamps to creation.
                record.data.insert("created_at".into(),json!("2000-01-01 00:00:00"));
                journal::atomic_write(&root.join(&p),&codec::bytes(&record).unwrap()).unwrap();
            }
        }
    }
    let current = token(&mut s, "spec");
    let before = journal::inventory(root).unwrap();
    s.commit(save("edit", "spec", "Recovered\n", vec![current]))
        .unwrap();
    let after = journal::inventory(root).unwrap();
    for (p,b) in &before {
        if p.starts_with("records/design_workspaces/") || p.starts_with("records/diagrams/") { assert_eq!(serde_json::from_slice::<Value>(&after[p]).unwrap(),serde_json::from_slice::<Value>(b).unwrap(),"Markdown edit changed shared architecture record {p}"); }
    }
    write_files(root, &before);
    journal::stage_for_test(&local, &before, &after).unwrap();
    assert_eq!(
        s.snapshot()
            .unwrap()
            .markdown_document("spec")
            .unwrap()
            .body,
        "Recovered\n"
    );
    assert_eq!(journal::inventory(root).unwrap(), after);
    journal::stage_for_test(&local, &before, &after).unwrap();
    journal::atomic_write(&root.join(path), b"external unresolved edit").unwrap();
    assert!(s.snapshot().is_err());
    assert_eq!(
        fs::read(root.join(path)).unwrap(),
        b"external unresolved edit"
    );
}

#[test]
fn links_use_uuid_references_and_deleted_documents_remain_tombstones() {
    let dir = tempfile::tempdir().unwrap();
    let req = request(dir.path());
    let root = Path::new(&req.location);
    let mut s = store(&req);
    s.commit(save("first", "a", "body", vec![])).unwrap();
    let mut request = save("second", "b", "B", vec![]);
    if let Change::Design(DesignWrite::Save { changes, .. }) = &mut request.changes[0] {
        if let api::design::DesignChange::UpsertMarkdown { design_links, .. } = &mut changes[0] {
            design_links.push(api::markdown::DesignAssociation {
                target_type: api::markdown::DesignTargetKind::Markdown,
                design_external_id: "a".into(),
            });
        }
    }
    s.commit(request).unwrap();
    let files = journal::inventory(root).unwrap();
    let (_, bytes) = files
        .iter()
        .find(|(p, _)| p.starts_with("records/markdown_design_links/"))
        .unwrap();
    let link: Value = serde_json::from_slice(bytes).unwrap();
    assert!(link["data"]["document_id"]["ref"].is_string());
    let a = token(&mut s, "a");
    let deletion = |tokens| Mutation {
        operation_id: "delete".into(),
        changes: vec![Change::Design(DesignWrite::Save {
            change_intent: "Remove".into(),
            changes: serde_json::from_value(json!([{"op":"delete_markdown","externalId":"a"}]))
                .unwrap(),
            read_tokens: tokens,
        })],
    };
    assert!(s.commit(deletion(vec![a.clone()])).is_err());
    let b = token(&mut s, "b");
    s.commit(save("detach", "b", "B", vec![b])).unwrap();
    s.commit(deletion(vec![a])).unwrap();
    drop(s);
    let mut s = store(&req);
    assert!(s.snapshot().unwrap().markdown_document("a").is_err());
    let records = engine::parse_records(&journal::inventory(root).unwrap()).unwrap();
    assert!(records
        .values()
        .any(|r| r.collection == "markdown_design_documents" && r.deleted));
}
