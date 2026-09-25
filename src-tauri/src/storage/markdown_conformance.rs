//! The same document transaction assertions apply to every available storage implementation.
use super::*;
use api::{*, documents::{DesignDocument,DocumentReadToken}, markdown::MarkdownQuery};
use serde_json::{json,Value};

fn read(store:&mut dyn ProjectStorage,id:&str)->DesignDocument {
    store.snapshot().unwrap().design_documents(&[format!("markdown:{id}")]).unwrap().remove(0)
}
fn write(id:&str,changes:Value,documents:&[DesignDocument])->Mutation {
    Mutation {operation_id:id.into(),changes:vec![Change::Design(DesignWrite::Save {
        change_intent:"Common Markdown contract".into(),changes:serde_json::from_value(changes).unwrap(),
        read_tokens:documents.iter().map(|d|DocumentReadToken {document_id:d.document_id.clone(),read_token:d.read_token.clone()}).collect(),
    })]}
}
fn upsert(id:&str,body:&str)->Value {json!({"op":"upsert_markdown","externalId":id,"title":id,"body":body,"designLinks":[]})}
fn cursor(store:&mut dyn ProjectStorage)->ChangeCursor {store.snapshot().unwrap().metadata().cursor.clone()}

/// Future server SQL must call this same contract with its reopen factory before enabling it.
fn assert_contract(open:impl Fn()->Box<dyn ProjectStorage>) {
    let mut first=open();let mut second=open();
    let before=cursor(first.as_mut());
    let body="# Complete\r\n日本語 Grüße\r\n```rust\r\nlet x=1;\r\n```\r\n".repeat(4000);
    let create=write("create",json!([upsert("a",&body),upsert("b","second")]),&[]);
    let committed=first.commit(create.clone()).unwrap();
    assert_ne!(cursor(first.as_mut()),before);
    assert_eq!(cursor(second.as_mut()),cursor(first.as_mut()));
    assert_eq!(serde_json::to_value(second.commit(create.clone()).unwrap()).unwrap(),serde_json::to_value(committed).unwrap());
    let a=read(first.as_mut(),"a");let b=read(first.as_mut(),"b");
    assert_eq!(a.document["body"],body);
    let before_noop=cursor(first.as_mut());
    first.commit(write("no-op",json!([upsert("a",&body)]),&[a.clone()])).unwrap();
    assert_eq!(cursor(first.as_mut()),before_noop);
    assert_eq!(read(first.as_mut(),"a").read_token,a.read_token);
    second.commit(write("peer",json!([upsert("a","peer edit")]),&[a.clone()])).unwrap();
    first.commit(write("disjoint",json!([upsert("b","independent")]),&[b.clone()])).unwrap();
    let current_b=read(first.as_mut(),"b");
    assert!(first.commit(write("stale-batch",json!([upsert("b","must roll back"),upsert("a","lost update")]),&[current_b.clone(),a.clone()])).is_err());
    assert_eq!(read(first.as_mut(),"b").read_token,current_b.read_token);
    assert!(first.snapshot().unwrap().receipt("stale-batch").unwrap().is_none());
    assert!(first.commit(write("stale-delete",json!([{"op":"delete_markdown","externalId":"a"}]),&[a])).is_err());
    let page=first.snapshot().unwrap().markdown_documents(&MarkdownQuery{limit:1,..Default::default()}).unwrap();
    assert_eq!(page.total_count,2);assert_eq!(page.documents.len(),1);assert!(page.next_after_id.is_some());
    let current_a=read(first.as_mut(),"a");
    let deletion=write("delete",json!([{"op":"delete_markdown","externalId":"a"}]),&[current_a.clone()]);
    first.commit(deletion.clone()).unwrap();second.commit(deletion).unwrap();
    assert!(read(second.as_mut(),"a").document.is_null());
    second.commit(create).unwrap(); // Historical receipt cannot recreate deleted content.
    assert!(read(second.as_mut(),"a").document.is_null());
    assert!(first.commit(write("resurrect",json!([upsert("a","stale resurrection")]),&[current_a])).is_err());
    first.close().unwrap();second.close().unwrap();
    let mut reopened=open();assert!(read(reopened.as_mut(),"a").document.is_null());assert_eq!(read(reopened.as_mut(),"b").document["body"],"independent");
}

#[test]
fn all_available_adapters_pass_the_identical_markdown_contract() {
    for text in [false,true] {
        let root=tempfile::tempdir().unwrap();
        let request=OpenRequest{location:root.path().join(if text{".adashi/text"}else{".adashi/adashi.sqlite3"}).to_string_lossy().into(),registered_identity:ProjectIdentity{id:"contract".into(),name:"Contract".into()},computer_id:"test".into(),checkout_path:root.path().to_string_lossy().into(),mode:OpenMode::InitializeOrMigrate,cursor_scope:None};
        assert_contract(||Box::new(StorageClient::new(if text{text::TextFactory.open(&request)}else{sqlite::SqliteFactory.open(&request)}.unwrap())));
    }
}
