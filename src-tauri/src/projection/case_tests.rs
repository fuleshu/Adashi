//! Case-independent discovery, publication and cleanup over real project snapshots.
use super::*;
use adashi_storage_api::{Change, DesignWrite, Mutation, ProjectStorage};
use serde_json::json;

fn project(root: &Path) -> crate::settings::ProjectSettings {
    crate::settings::ProjectSettings {
        id: "case-projection".into(), name: "Case projection".into(),
        folder: root.to_string_lossy().into(),
    }
}

#[test]
fn mixed_case_projection_preserves_instructions_links_and_ownership() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("Src/Nested")).unwrap();
    fs::create_dir_all(root.path().join("Docs/Adashi")).unwrap();
    fs::write(root.path().join("agents.md"), "Root house rules\n").unwrap();
    fs::write(root.path().join("Src/Nested/AgEnTs.Md"), "Nested house rules\n").unwrap();
    let mut store = crate::storage::ProjectStore::open(&project(root.path())).unwrap();
    store.commit(Mutation {
        operation_id: "case-projection".into(),
        changes: vec![Change::Design(DesignWrite::Save {
            change_intent: "Case-insensitive projection regression".into(), read_tokens: vec![],
            changes: serde_json::from_value(json!([
                {"op":"upsert_markdown","externalId":"prose","title":"Case specification","body":"Keep the complete design.","designLinks":[]},
                {"op":"upsert_binding","designExternalId":"prose","targetType":"file","target":"src/nested/example.rs"}
            ])).unwrap(),
        })],
    }).unwrap();
    let snapshot = store.snapshot().unwrap();
    let report = regenerate(snapshot.as_ref(), root.path(), "AGENTS.md", true).unwrap();
    assert!(report.written.contains(&"agents.md".into()));
    assert!(report.written.contains(&"Src/Nested/AgEnTs.Md".into()));
    let instruction = root.path().join("agents.md");
    let nested = root.path().join("Src/Nested/AgEnTs.Md");
    let before = fs::read_to_string(&instruction).unwrap();
    assert!(before.starts_with("Root house rules\n"));
    assert!(before.contains("Docs/Adashi/index.md"));
    assert!(fs::read_to_string(&nested).unwrap().starts_with("Nested house rules\n"));
    assert!(fs::read_to_string(&nested).unwrap().contains("../../Docs/Adashi/design-"));
    let mtime = fs::metadata(&instruction).unwrap().modified().unwrap();
    regenerate_configured(snapshot.as_ref(), root.path(), "aGeNtS.MD", true, "DOCS/ADASHI").unwrap();
    assert_eq!(fs::read_to_string(&instruction).unwrap(), before);
    assert_eq!(fs::metadata(&instruction).unwrap().modified().unwrap(), mtime);
    let current = status_configured(snapshot.as_ref(), root.path(), "AGENTS.md", true, "docs/adashi").unwrap();
    assert!(current.error.is_none());
    assert!(current.files.iter().all(|file| file.state == "current"));
    assert!(current.files.iter().any(|file| file.path == "agents.md"));
    assert_eq!(fs::read_dir(root.path()).unwrap().filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().eq_ignore_ascii_case("agents.md")).count(), 1);

    // Simulate a Git checkout changing only a generated filename's spelling.
    let directory = root.path().join("Docs/Adashi");
    let original = directory.join("agent-workflow.md");
    let temporary = directory.join("workflow-case-rename.tmp");
    let renamed = directory.join("AGENT-WORKFLOW.MD");
    fs::rename(&original, &temporary).unwrap();
    fs::rename(&temporary, &renamed).unwrap();
    regenerate(snapshot.as_ref(), root.path(), "AGENTS.md", true).unwrap();
    assert!(fs::read_to_string(&instruction).unwrap().contains("Docs/Adashi/AGENT-WORKFLOW.MD"));
    assert!(status(snapshot.as_ref(), root.path(), "AGENTS.md", true).unwrap().files.iter().all(|file| file.state == "current"));
    fs::write(directory.join("user.md"), "Unrelated document").unwrap();
    regenerate_configured(snapshot.as_ref(), root.path(), "AGENTS.md", false, "DOCS/ADASHI").unwrap();
    assert!(!renamed.exists());
    assert_eq!(fs::read_to_string(directory.join("user.md")).unwrap(), "Unrelated document");
    let removed = remove_managed_blocks(root.path(), "AGENTS.md").unwrap();
    assert_eq!(removed, vec!["Src/Nested/AgEnTs.Md", "agents.md"]);
    assert_eq!(fs::read_to_string(&instruction).unwrap(), "Root house rules\n");
    assert_eq!(fs::read_to_string(&nested).unwrap(), "Nested house rules\n");
}

#[test]
fn differently_cased_unowned_output_is_preserved() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("Docs/Adashi")).unwrap();
    let unowned = root.path().join("Docs/Adashi/AGENT-WORKFLOW.MD");
    fs::write(&unowned, "User-owned workflow").unwrap();
    let mut store = crate::storage::ProjectStore::open(&project(root.path())).unwrap();
    let snapshot = store.snapshot().unwrap();
    let error = regenerate(snapshot.as_ref(), root.path(), "AGENTS.md", true).unwrap_err();
    assert!(error.contains("Unowned"), "{error}");
    assert_eq!(fs::read_to_string(&unowned).unwrap(), "User-owned workflow");
}

#[cfg(unix)]
#[test]
fn distinct_case_aliases_are_ambiguous_even_for_an_exact_match() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("agents.md"), "Lowercase rules").unwrap();
    fs::write(root.path().join("AGENTS.md"), "Uppercase rules").unwrap();
    for name in ["agents.md", "AGENTS.md", "Agents.md"] {
        assert!(files::checked(root.path(), name).unwrap_err().contains("Ambiguous"));
    }
    assert!(remove_managed_blocks(root.path(), "AGENTS.md").unwrap_err().contains("Ambiguous"));
    assert_eq!(fs::read_to_string(root.path().join("agents.md")).unwrap(), "Lowercase rules");
    assert_eq!(fs::read_to_string(root.path().join("AGENTS.md")).unwrap(), "Uppercase rules");
}
