use adashi_storage_api::{design::DesignChange, documents::*, markdown::*, *};
use serde_json::json;
use std::collections::BTreeSet;

fn document() -> MarkdownDesignDocument {
    serde_json::from_value(json!({"externalId":"decision-a", "title":"Decision", "body":"# Exact body\n\n  code  \n", "designLinks":[{"targetType":"element", "designExternalId":"core"}]})).unwrap()
}

#[test]
fn canonical_content_tokens_and_complete_reads() {
    let doc = document();
    let snapshot = doc.snapshot().unwrap();
    assert_eq!(snapshot.document["body"], doc.body);
    assert_eq!(snapshot.document_id, "markdown:decision-a");
    assert_eq!(snapshot.read_token, doc.snapshot().unwrap().read_token);
    assert!(serde_json::to_value(doc.summary().unwrap())
        .unwrap()
        .get("body")
        .is_none());
    for field in ["title", "body", "designLinks"] {
        let mut changed = snapshot.document.clone();
        changed[field] = if field == "designLinks" {
            json!([])
        } else {
            json!("Changed")
        };
        assert_ne!(
            snapshot.read_token,
            document_token(&snapshot.document_id, &changed).unwrap()
        );
    }
    assert!(matches!(
        check_document_tokens(&[], &[snapshot.clone()]),
        Err(StorageError::Documents(_))
    ));
    let token = DocumentReadToken {
        document_id: snapshot.document_id.clone(),
        read_token: snapshot.read_token.clone(),
    };
    check_document_tokens(&[token.clone()], &[snapshot.clone()]).unwrap();
    let mut changed = doc.clone();
    changed.body.push('!');
    assert!(matches!(
        check_document_tokens(&[token], &[changed.snapshot().unwrap()]),
        Err(StorageError::Documents(_))
    ));
}

#[test]
fn closed_schema_validation_and_typed_mutations() {
    let mut value = serde_json::to_value(document()).unwrap();
    value["projectId"] = json!("foreign-project");
    assert!(serde_json::from_value::<MarkdownDesignDocument>(value).is_err());
    let mut doc = document();
    doc.external_id = " ".into();
    assert!(doc.validate().is_err());
    let mut doc = document();
    doc.design_links.push(doc.design_links[0].clone());
    assert!(doc.validate().is_err());
    let mut value = serde_json::to_value(document()).unwrap();
    value["op"] = json!("upsert_markdown");
    let change: DesignChange = serde_json::from_value(value).unwrap();
    let mutation = Mutation {
        operation_id: "md-create".into(),
        changes: vec![Change::Design(DesignWrite::Save {
            change_intent: "Create design".into(),
            changes: vec![change.clone()],
            read_tokens: vec![],
        })],
    };
    prepare_mutation(mutation.clone()).unwrap();
    let mut duplicate = mutation;
    if let Change::Design(DesignWrite::Save { changes, .. }) = &mut duplicate.changes[0] {
        changes.push(change);
    }
    assert!(prepare_mutation(duplicate).is_err());
    assert!(serde_json::from_value::<DesignChange>(
        json!({"op":"delete_markdown", "externalId":"decision-a"})
    )
    .is_ok());
    let schema = serde_json::to_value(schemars::schema_for!(DesignChange))
        .unwrap()
        .to_string();
    assert!(schema.contains("upsert_markdown") && schema.contains("delete_markdown"));
    assert!(MarkdownQuery {
        limit: 101,
        ..Default::default()
    }
    .validate()
    .is_err());
}

#[test]
fn references_use_the_final_project_graph_and_block_dangling_deletes() {
    let edges = document().references();
    let mut resources = BTreeSet::from([ResourceKey {
        kind: "design.element".into(),
        id: "core".into(),
    }]);
    check_references(&edges, &resources).unwrap();
    resources.clear();
    assert!(matches!(
        check_references(&edges, &resources),
        Err(StorageError::References(_))
    ));
    // Explicitly detaching in the same transaction leaves no incoming edge.
    check_references(&[], &resources).unwrap();
    assert_eq!(
        DesignAssociation {
            target_type: DesignTargetKind::Markdown,
            design_external_id: "decision-a".into()
        }
        .resource_key()
        .kind,
        "design.markdown"
    );
}
