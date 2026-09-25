//! Shared discovery acceptance runs against each implemented backend.
use super::*;
use api::{*, documents::DocumentReadToken, qa::QaJobQuery};
use serde_json::{json, Value};

fn save(id: &str, changes: Value, read_tokens: Vec<DocumentReadToken>) -> Mutation {
    Mutation { operation_id:id.into(), changes:vec![Change::Design(DesignWrite::Save {
        change_intent:"Discovery acceptance".into(), changes:serde_json::from_value(changes).unwrap(),read_tokens
    })] }
}

#[test]
fn markdown_discovery_references_and_search_on_both_backends() {
    for text in [false,true] {
        let root = tempfile::tempdir().unwrap();
        let request = OpenRequest { location:root.path().join(if text {".adashi/text"} else {".adashi/adashi.sqlite3"}).to_string_lossy().into(), registered_identity:ProjectIdentity { id:"discovery".into(),name:"Discovery".into() },computer_id:"test".into(),checkout_path:root.path().to_string_lossy().into(),mode:OpenMode::InitializeOrMigrate,cursor_scope:None };
        let backend = if text { text::TextFactory.open(&request) } else { sqlite::SqliteFactory.open(&request) }.unwrap();
        let mut store = StorageClient::new(backend);
        let body = format!("# Specification\n{}\n日本語 Needle Grüße\n```rust\nlet exact = true;\n```\n", "long prose\n".repeat(200));
        store.commit(save("seed",json!([
            {"op":"upsert_element","externalId":"app","elementType":"Software System","name":"App","tags":"External"},
            {"op":"upsert_markdown","externalId":"spec","title":"Original","body":body,"designLinks":[{"targetType":"element","designExternalId":"app"}]},
            {"op":"upsert_markdown","externalId":"project-prose","title":"Project prose","body":"Independent project specification","designLinks":[]},
            {"op":"upsert_binding","designExternalId":"spec","targetType":"file","target":"src/example.rs"}
        ]),vec![])).unwrap();
        let links = json!([{"targetType":"markdown","designExternalId":"spec"}]);
        store.commit(Mutation { operation_id:"references".into(),changes:vec![
            Change::Task(TaskWrite::Create {input:serde_json::from_value(json!({"title":"Implement","designSpecificationLinks":links})).unwrap()}),
            Change::Qa(QaWrite::CreateJob {input:serde_json::from_value(json!({"name":"Verify","command":"echo ok","designSpecificationLinks":links})).unwrap()})
        ] }).unwrap();
        let token;
        {
            let s = store.snapshot().unwrap();
            assert_eq!(s.design_overview(None).unwrap().markdown.total_count,2);
            let scope = s.design_scope(&ScopeQuery {element_id:"spec".into(),children_depth:Some(0),include_ancestors:true}).unwrap();
            assert_eq!(scope.backlinks.len(),3);
            assert_eq!(scope.elements[0].external_id,"app");
            let expanded = super::documents::with_documents(s.as_ref(), scope).unwrap();
            let document = expanded["documents"].as_array().unwrap().iter().find(|d|d["documentId"]=="markdown:spec").unwrap();
            assert_eq!(document["document"]["body"],body);
            token = DocumentReadToken {document_id:"markdown:spec".into(),read_token:document["readToken"].as_str().unwrap().into()};
            assert_eq!(s.design_scope(&ScopeQuery {element_id:"app".into(),children_depth:Some(0),include_ancestors:false}).unwrap().markdown.documents[0].external_id,"spec");
            assert_eq!(s.design_by_ids(&["project-prose".into()]).unwrap().markdown.documents.len(),1);
            assert_eq!(s.design_bindings(&BindingQuery {files:vec!["src/example.rs".into()],symbols:vec![]}).unwrap().markdown.documents.len(),1);
            let hits = s.design_search(&DesignSearchQuery {query:"needle".into(),kinds:vec!["markdown".into()],limit:10}).unwrap();
            assert_eq!(hits.hits[0].id,"spec"); assert!(hits.hits[0].summary.contains("Needle"));
            let grep = crate::grep::search(s.as_ref(),&serde_json::from_value(json!({"projectName":"discovery","pattern":"needle in:design file:src/example.rs"})).unwrap()).unwrap();
            let rendered = serde_json::to_string(&grep).unwrap();
            assert!(rendered.contains("design:spec")); assert!(rendered.len()<8192);
            assert_eq!(s.qa_jobs(&serde_json::from_value::<QaJobQuery>(json!({"designExternalIds":["spec"]})).unwrap()).unwrap().len(),1);
            let health = crate::design_health::scan_snapshot(s.as_ref(),root.path()).unwrap();
            let health = serde_json::to_string(&health).unwrap();
            assert!(health.contains("project-prose"));
        }
        store.commit(save("rename",json!([{"op":"upsert_markdown","externalId":"spec","title":"Renamed","body":body,"designLinks":[{"targetType":"element","designExternalId":"app"}]}]),vec![token])).unwrap();
        let s = store.snapshot().unwrap();
        assert_eq!(s.task(1).unwrap().design_specification_links[0].title,"Renamed");
        assert_eq!(s.qa_job(1).unwrap().design_specification_links[0].title,"Renamed");
    }
}
