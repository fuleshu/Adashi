//! Full output acceptance with real canonical snapshots and filesystem ownership.
use super::*;
use super::markdown;
use adashi_storage_api::{*, documents::DocumentReadToken};
use serde_json::json;

fn save(store:&mut dyn ProjectStorage, title:&str, body:&str, existing:bool) {
    let tokens=if existing {
        let d=store.snapshot().unwrap().design_documents(&["markdown:prose".into()]).unwrap().remove(0);
        vec![DocumentReadToken {document_id:d.document_id,read_token:d.read_token}]
    } else {vec![]};
    store.commit(Mutation {operation_id:uuid::Uuid::new_v4().to_string(),changes:vec![Change::Design(DesignWrite::Save {change_intent:"Projection acceptance".into(),read_tokens:tokens,changes:serde_json::from_value(json!([{"op":"upsert_markdown","externalId":"prose","title":title,"body":body,"designLinks":[]}])).unwrap()})]}).unwrap();
}

#[test]
fn markdown_output_is_complete_stable_owned_and_recoverable() {
    let root=tempfile::tempdir().unwrap();
    let project=crate::settings::ProjectSettings {id:"projection".into(),name:"Projection".into(),folder:root.path().to_string_lossy().into()};
    let mut store=crate::storage::ProjectStore::open(&project).unwrap();
    let body="# Specification\r\n\r\n日本語 Grüße\n```rust\n let x=1;  \n```\n".repeat(2000);
    save(&mut store,"Original",&body,false);
    store.commit(Mutation {operation_id:"binding".into(),changes:vec![Change::Design(DesignWrite::Save {change_intent:"Bind specification".into(),read_tokens:vec![],changes:serde_json::from_value(json!([{"op":"upsert_binding","designExternalId":"prose","targetType":"file","target":"src/nested/example.rs"}])).unwrap()})]}).unwrap();
    fs::write(root.path().join("AGENTS.md"),"User instructions\n").unwrap();
    fs::write(root.path().join("agents_template.md"),"Custom project guidance\n").unwrap();
    let path=markdown::document_path("docs/adashi","prose");
    {
        let s=store.snapshot().unwrap();
        assert!(status(s.as_ref(),root.path(),"AGENTS.md",true).unwrap().files.iter().any(|f|f.path==path && f.state=="missing"));
        regenerate(s.as_ref(),root.path(),"AGENTS.md",true).unwrap();
        let full=fs::read_to_string(root.path().join(&path)).unwrap();
        assert!(full.ends_with(&body));assert!(full.contains("readToken"));assert!(full.contains("Design ID: \"prose\""));
        let root_short=fs::read_to_string(root.path().join("AGENTS.md")).unwrap();
        assert!(root_short.starts_with("User instructions"));assert!(root_short.contains("docs/adashi/index.md"));
        assert!(root_short.contains("docs/adashi/agent-workflow.md"));
        assert_eq!(fs::read_to_string(root.path().join("agents_template.md")).unwrap(),"Custom project guidance\n");
        assert!(fs::read_to_string(root.path().join("docs/adashi/agent-workflow.md")).unwrap().ends_with(AGENT_WORKFLOW));
        let short=fs::read_to_string(root.path().join("src/nested/AGENTS.md")).unwrap();
        assert!(short.contains("../../docs/adashi/design-"));assert!(!short.contains("let x=1"));assert!(short.len()<=FOLDER_BUDGET+1);
        let mtime=fs::metadata(root.path().join(&path)).unwrap().modified().unwrap();
        regenerate(s.as_ref(),root.path(),"AGENTS.md",true).unwrap();
        assert_eq!(fs::metadata(root.path().join(&path)).unwrap().modified().unwrap(),mtime);
        fs::write(root.path().join(&path),"External drift").unwrap();
        assert!(status(s.as_ref(),root.path(),"AGENTS.md",true).unwrap().files.iter().any(|f|f.path==path && f.state=="drifted"));
        assert!(regenerate(s.as_ref(),root.path(),"AGENTS.md",true).unwrap().repaired.contains(&path));
        assert_eq!(s.markdown_document("prose").unwrap().body,body);
    }
    #[cfg(windows)] {
        let target=root.path().join(&path);
        let mut permissions=fs::metadata(&target).unwrap().permissions();
        permissions.set_readonly(true);fs::set_permissions(&target,permissions.clone()).unwrap();
        save(&mut store,"Permission test",&body,true);
        let s=store.snapshot().unwrap();
        assert!(regenerate(s.as_ref(),root.path(),"AGENTS.md",true).is_err());
        assert_eq!(s.markdown_document("prose").unwrap().title,"Permission test");
        permissions.set_readonly(false);fs::set_permissions(&target,permissions).unwrap();
        regenerate(s.as_ref(),root.path(),"AGENTS.md",true).unwrap();
    }
    save(&mut store,"Renamed",&body,true);
    let s=store.snapshot().unwrap();
    assert!(status(s.as_ref(),root.path(),"AGENTS.md",true).unwrap().files.iter().any(|f|f.path==path && f.state=="stale"));
    regenerate(s.as_ref(),root.path(),"AGENTS.md",true).unwrap();
    assert!(root.path().join(&path).is_file());
    fs::write(root.path().join("docs/adashi/user.md"),"Keep me").unwrap();
    regenerate_configured(s.as_ref(),root.path(),"AGENTS.md",true,"specifications").unwrap();
    assert!(!root.path().join(&path).exists());assert!(root.path().join("docs/adashi/user.md").exists());
    let moved=markdown::document_path("specifications","prose");
    // Simulate an interrupted publication after ownership was persisted.
    fs::remove_file(root.path().join(&moved)).unwrap();
    regenerate_configured(s.as_ref(),root.path(),"AGENTS.md",true,"specifications").unwrap();
    regenerate_configured(s.as_ref(),root.path(),"AGENTS.md",false,"specifications").unwrap();
    assert!(!root.path().join(&moved).exists());assert!(root.path().join("docs/adashi/user.md").exists());
    fs::write(root.path().join(&moved),"Unowned user file").unwrap();
    assert!(regenerate_configured(s.as_ref(),root.path(),"AGENTS.md",true,"specifications").unwrap_err().contains("Unowned"));
    assert_eq!(fs::read_to_string(root.path().join(&moved)).unwrap(),"Unowned user file");
    assert!(regenerate_configured(s.as_ref(),root.path(),"AGENTS.md",true,"../escape").is_err());
    fs::write(root.path().join("blocked"),"A file, not a directory").unwrap();
    assert!(regenerate_configured(s.as_ref(),root.path(),"AGENTS.md",true,"blocked/output").is_err());
    assert_eq!(s.markdown_document("prose").unwrap().body,body);
}

#[test]
fn markdown_paths_reject_case_collisions_and_symlink_escapes() {
    let root=tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("Docs")).unwrap();
    assert!(files::checked(root.path(),"docs/adashi/index.md").is_err());
    #[cfg(unix)] {
        std::os::unix::fs::symlink(std::env::temp_dir(),root.path().join("escape")).unwrap();
        assert!(files::checked(root.path(),"escape/design.md").is_err());
    }
    #[cfg(windows)] {
        let target=tempfile::tempdir().unwrap();
        let output=std::process::Command::new("cmd").args(["/c","mklink","/J"]).arg(root.path().join("escape")).arg(target.path()).output().unwrap();
        assert!(output.status.success());
        assert!(files::checked(root.path(),"escape/design.md").is_err());
        // Remove only the junction, never recursively traverse its destination.
        fs::remove_dir(root.path().join("escape")).unwrap();
    }
}
