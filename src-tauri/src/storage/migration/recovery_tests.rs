//! Faults at the publication boundary must leave one complete selected dataset.
use super::*;

fn fixture() -> (tempfile::TempDir, Activation) {
    let root = tempfile::tempdir().unwrap();
    let before = BTreeMap::from([
        (DESCRIPTOR, b"old selection".to_vec()),
        (".gitignore", b"old ignore".to_vec()),
        (".adashi/local/state.json", b"old local".to_vec()),
        (".adashi/text/format.json", b"old destination".to_vec()),
    ]);
    for (name, data) in before {
        replace(root.path(), name, Some(data)).unwrap();
    }
    let plan = Activation::prepare(
        root.path(),
        BTreeMap::from([
            (DESCRIPTOR.into(), Some(b"new selection".to_vec())),
            (".gitignore".into(), Some(b"new ignore".to_vec())),
            (
                ".adashi/local/state.json".into(),
                Some(b"new local".to_vec()),
            ),
            (
                ".adashi/local/text-identities.json".into(),
                Some(b"new identities".to_vec()),
            ),
            (
                ".adashi/text/format.json".into(),
                Some(b"new destination".to_vec()),
            ),
        ]),
    )
    .unwrap();
    (root, plan)
}

#[test]
fn every_precommit_publication_error_restores_all_original_bytes() {
    let (_, sample) = fixture();
    for fail_at in sample.files.keys().filter(|p| p.as_str() != DESCRIPTOR) {
        let (root, plan) = fixture();
        let error = plan
            .execute_with(
                root.path(),
                &root.path().join("backup"),
                || Ok(()),
                |name| {
                    if name == fail_at {
                        Err(StorageError::TimedOut)
                    } else {
                        Ok(())
                    }
                },
            )
            .unwrap_err();
        assert!(error.to_string().contains("rolled back"), "{error}");
        for (name, item) in &plan.files {
            assert_eq!(
                text::journal::read_optional(&path(root.path(), name).unwrap()).unwrap(),
                bytes(&item.before).unwrap(),
                "{fail_at}: {name}"
            );
        }
        assert!(!root.path().join(PENDING).exists());
    }
}

#[test]
fn failed_source_check_rolls_back_published_destination() {
    let (root, plan) = fixture();
    assert!(plan
        .execute(root.path(), &root.path().join("backup"), || Err(invalid(
            "source changed"
        )))
        .unwrap_err()
        .to_string()
        .contains("rolled back"));
    for (name, item) in &plan.files {
        assert_eq!(
            text::journal::read_optional(&path(root.path(), name).unwrap()).unwrap(),
            bytes(&item.before).unwrap()
        );
    }
}

#[test]
fn error_after_descriptor_commit_reports_completed_conversion() {
    let (root, plan) = fixture();
    plan.execute_with(
        root.path(),
        &root.path().join("backup"),
        || Ok(()),
        |name| {
            if name == DESCRIPTOR {
                Err(StorageError::TimedOut)
            } else {
                Ok(())
            }
        },
    )
    .unwrap();
    for (name, item) in &plan.files {
        assert_eq!(
            text::journal::read_optional(&path(root.path(), name).unwrap()).unwrap(),
            bytes(&item.after).unwrap()
        );
    }
    assert!(!root.path().join(PENDING).exists());
}

#[cfg(windows)]
#[test]
fn locked_untouched_destination_does_not_block_rollback() {
    use std::os::windows::fs::OpenOptionsExt;
    let (root, plan) = fixture();
    let _locked = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(root.path().join(".gitignore"))
        .unwrap();
    let error = plan
        .execute(root.path(), &root.path().join("backup"), || Ok(()))
        .unwrap_err();
    assert!(error.to_string().contains("rolled back"), "{error}");
    for (name, item) in &plan.files {
        assert_eq!(
            text::journal::read_optional(&path(root.path(), name).unwrap()).unwrap(),
            bytes(&item.before).unwrap()
        );
    }
    assert!(!root.path().join(PENDING).exists());
}

#[cfg(windows)]
#[test]
fn cleanup_failure_is_a_warning_and_reopening_keeps_completed_data() {
    use std::os::windows::fs::OpenOptionsExt;
    let (root, plan) = fixture();
    let mut locked = None;
    let warnings = plan
        .execute_with(
            root.path(),
            &root.path().join("backup"),
            || Ok(()),
            |name| {
                if name == DESCRIPTOR {
                    locked = Some(
                        fs::OpenOptions::new()
                            .read(true)
                            .share_mode(1)
                            .open(root.path().join(PENDING))
                            .unwrap(),
                    );
                }
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(warnings.len(), 1);
    recover_for_read(root.path()).unwrap();
    assert!(root.path().join(PENDING).exists());
    assert!(
        recover(root.path()).is_err(),
        "new writes must wait for cleanup"
    );
    for (name, item) in &plan.files {
        assert_eq!(
            text::journal::read_optional(&path(root.path(), name).unwrap()).unwrap(),
            bytes(&item.after).unwrap()
        );
    }
    drop(locked);
    recover(root.path()).unwrap();
    assert!(!root.path().join(PENDING).exists());
}
