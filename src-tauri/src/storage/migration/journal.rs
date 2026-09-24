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
    pub fn publish(
        &self,
        root: &Path,
        check_source: impl FnOnce() -> StorageResult<()>,
    ) -> StorageResult<()> {
        self.verify(root)?;
        for (name, item) in &self.files {
            if name != DESCRIPTOR && !name.starts_with(".adashi/local/") {
                replace(root, name, bytes(&item.after)?)?;
            }
        }
        check_source()?;
        for (name, item) in &self.files {
            if name.starts_with(".adashi/local/") {
                replace(root, name, bytes(&item.after)?)?;
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
        fs::remove_file(root.join(PENDING)).map_err(StorageError::backend)
    }
}
pub(super) fn recover(root: &Path) -> StorageResult<()> {
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
    fs::remove_file(root.join(PENDING)).map_err(StorageError::backend)
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
