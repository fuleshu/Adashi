//! Recoverable activation. The descriptor is the commit point and is written last.
//! Permanent before/after images also preserve a populated, inactive destination.
use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};

const PENDING: &str = ".adashi/local/storage-migration.json";
pub(super) const DESCRIPTOR: &str = ".adashi/storage.json";

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Activation {
    schema_version: u32,
    files: BTreeMap<String, Replacement>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Replacement {
    before: Option<String>,
    after: Option<String>,
}

fn path(root: &Path, name: &str) -> StorageResult<PathBuf> {
    let allowed = matches!(
        name,
        DESCRIPTOR
            | ".gitignore"
            | ".adashi/adashi.sqlite3"
            | ".adashi/adashi.sqlite3-wal"
            | ".adashi/adashi.sqlite3-shm"
            | ".adashi/local/state.json"
            | ".adashi/local/text-identities.json"
    );
    if !allowed {
        let relative = name
            .strip_prefix(".adashi/text/")
            .ok_or_else(|| invalid("Invalid activation path"))?;
        if relative.starts_with('$') {
            return Err(invalid("Invalid activation path"));
        }
        return text::journal::safe_path(&root.join(".adashi/text"), relative);
    }
    let target = root.join(name);
    text::journal::reject_symlinks(root, &target)?;
    Ok(target)
}
fn bytes(value: &Option<String>) -> StorageResult<Option<Vec<u8>>> {
    value
        .as_ref()
        .map(|v| STANDARD.decode(v).map_err(StorageError::backend))
        .transpose()
}
fn replace(root: &Path, name: &str, value: Option<Vec<u8>>) -> StorageResult<()> {
    let target = path(root, name)?;
    // In particular, do not rewrite untouched files during rollback: the file
    // that rejected publication may still be locked, but already has its old data.
    if text::journal::read_optional(&target)? == value {
        return Ok(());
    }
    match value {
        Some(data) => text::journal::atomic_write(&target, &data),
        None => match fs::remove_file(target) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(StorageError::backend(e)),
        },
    }
}
impl Activation {
    /// Return an error only after rollback. Once the descriptor commits, report
    /// success; journal cleanup is maintenance and cannot undo a verified switch.
    pub fn execute(
        &self,
        root: &Path,
        backup: &Path,
        check_source: impl FnOnce() -> StorageResult<()>,
    ) -> StorageResult<Vec<String>> {
        self.execute_with(root, backup, check_source, |_| Ok(()))
    }

    fn execute_with(
        &self,
        root: &Path,
        backup: &Path,
        check_source: impl FnOnce() -> StorageResult<()>,
        after_write: impl FnMut(&str) -> StorageResult<()>,
    ) -> StorageResult<Vec<String>> {
        let result = self
            .save(root, backup)
            .and_then(|()| self.publish_with(root, check_source, after_write));
        if let Err(error) = result {
            let committed = text::journal::read_optional(&path(root, DESCRIPTOR)?)?
                == bytes(&self.files[DESCRIPTOR].after)?;
            recover_for_read(root).map_err(|recovery| invalid(&format!(
                "Conversion could not finish recovery: {recovery}. Original error: {error}. The source backup is retained at {}.", backup.display()
            )))?;
            if !committed {
                return Err(invalid(&format!("Conversion failed and was rolled back; the previous storage remains selected. {error}")));
            }
        }
        Ok(cleanup(root).err().map(|error| format!(
            "Conversion completed. Temporary recovery-file cleanup will be retried when the project is reopened: {error}"
        )).into_iter().collect())
    }

    pub fn prepare(root: &Path, after: BTreeMap<String, Option<Vec<u8>>>) -> StorageResult<Self> {
        if !after.contains_key(DESCRIPTOR) {
            return Err(invalid("Activation is missing its descriptor"));
        }
        let files = after
            .into_iter()
            .map(|(name, after)| {
                let before = text::journal::read_optional(&path(root, &name)?)?;
                Ok((
                    name,
                    Replacement {
                        before: before.map(|v| STANDARD.encode(v)),
                        after: after.map(|v| STANDARD.encode(v)),
                    },
                ))
            })
            .collect::<StorageResult<_>>()?;
        Ok(Self {
            schema_version: 1,
            files,
        })
    }
    fn verify(&self, root: &Path) -> StorageResult<()> {
        if self.schema_version != 1 || !self.files.contains_key(DESCRIPTOR) {
            return Err(invalid("Unsupported activation journal"));
        }
        for (name, item) in &self.files {
            let current = text::journal::read_optional(&path(root, name)?)?;
            if current != bytes(&item.before)? && current != bytes(&item.after)? {
                return Err(invalid(&format!("Storage recovery found an external edit in {name}; its before/after images and backups are retained. Reconcile that edit before reopening the project.")));
            }
        }
        Ok(())
    }
    pub fn save(&self, root: &Path, backup: &Path) -> StorageResult<()> {
        self.verify(root)?;
        let data = serde_json::to_vec_pretty(self).map_err(StorageError::backend)?;
        text::journal::atomic_write(&backup.join("activation.json"), &data)?;
        text::journal::atomic_write(&root.join(PENDING), &data)
    }
    #[cfg(test)]
    pub fn publish(
        &self,
        root: &Path,
        check_source: impl FnOnce() -> StorageResult<()>,
    ) -> StorageResult<()> {
        self.publish_with(root, check_source, |_| Ok(()))
    }
    fn publish_with(
        &self,
        root: &Path,
        check_source: impl FnOnce() -> StorageResult<()>,
        mut after_write: impl FnMut(&str) -> StorageResult<()>,
    ) -> StorageResult<()> {
        self.verify(root)?;
        for (name, item) in &self.files {
            if name != DESCRIPTOR && !name.starts_with(".adashi/local/") {
                replace(root, name, bytes(&item.after)?)?;
                after_write(name)?;
            }
        }
        check_source()?;
        for (name, item) in &self.files {
            if name.starts_with(".adashi/local/") {
                replace(root, name, bytes(&item.after)?)?;
                after_write(name)?;
            }
        }
        // An editor must not have changed either selection or published data.
        self.verify(root)?;
        for (name, item) in &self.files {
            if name != DESCRIPTOR
                && text::journal::read_optional(&path(root, name)?)? != bytes(&item.after)?
            {
                return Err(invalid(
                    "Destination changed during publication; storage was not activated",
                ));
            }
        }
        let descriptor = &self.files[DESCRIPTOR];
        if text::journal::read_optional(&path(root, DESCRIPTOR)?)? != bytes(&descriptor.before)? {
            return Err(invalid("Storage selection changed during conversion"));
        }
        replace(root, DESCRIPTOR, bytes(&descriptor.after)?)?;
        after_write(DESCRIPTOR)
    }
}

fn cleanup(root: &Path) -> StorageResult<()> {
    match fs::remove_file(root.join(PENDING)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(StorageError::backend(error)),
    }
}
pub(super) fn recover(root: &Path) -> StorageResult<()> {
    recover_inner(root, true)
}

/// Reads may use completely restored data while a cleanup-only lock is held.
/// Writers must remove the journal first, otherwise later recovery could mistake
/// their new changes for interference with the old conversion.
pub(super) fn recover_for_read(root: &Path) -> StorageResult<()> {
    recover_inner(root, false)
}

fn recover_inner(root: &Path, require_cleanup: bool) -> StorageResult<()> {
    let Some(data) = text::journal::read_optional(&root.join(PENDING))? else {
        return Ok(());
    };
    let journal: Activation = serde_json::from_slice(&data).map_err(StorageError::backend)?;
    journal.verify(root)?;
    let committed = text::journal::read_optional(&path(root, DESCRIPTOR)?)?
        == bytes(&journal.files[DESCRIPTOR].after)?;
    for (name, item) in &journal.files {
        if name != DESCRIPTOR {
            replace(
                root,
                name,
                bytes(if committed { &item.after } else { &item.before })?,
            )?;
        }
    }
    let result = cleanup(root);
    if require_cleanup {
        result.map_err(|error| invalid(&format!(
            "Project data is fully recovered, but the temporary conversion journal is still locked. Close the process holding it and retry before making further changes. {error}"
        )))
    } else {
        Ok(())
    }
}

#[cfg(test)]
pub(super) fn interrupt(
    root: &Path,
    backup: &Path,
    after: BTreeMap<String, Option<Vec<u8>>>,
    activate: bool,
) {
    let plan = Activation::prepare(root, after).unwrap();
    plan.save(root, backup).unwrap();
    for (name, item) in &plan.files {
        if name != DESCRIPTOR || activate {
            replace(root, name, bytes(&item.after).unwrap()).unwrap();
        }
    }
}

#[cfg(test)]
#[path = "recovery_tests.rs"]
mod tests;
