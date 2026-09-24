//! Project-scoped, adapter-neutral conversion with verified staging and an
//! explicit descriptor commit point. No client selects a backend independently.
use super::{
    config, sqlite, text, transfer::MigrationAdapter, BackendSelection, ProjectRegistration,
    StorageDescriptor, StorageError, StorageResult,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
mod journal;
#[cfg(test)]
mod tests;

fn invalid(message: &str) -> StorageError {
    StorageError::Validation(message.into())
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub(crate) fn selection_token(descriptor: &StorageDescriptor) -> StorageResult<String> {
    Ok(hash(
        &serde_json::to_vec(descriptor).map_err(StorageError::backend)?,
    ))
}
pub(crate) fn lock(project: &ProjectRegistration) -> StorageResult<text::journal::FileLock> {
    text::journal::FileLock::acquire(
        &crate::settings::project_data_dir(project).join("local/storage.lock"),
    )
}
pub(crate) fn recover(project: &ProjectRegistration) -> StorageResult<()> {
    journal::recover(Path::new(&project.folder))
}
fn adapter(kind: &str) -> StorageResult<Box<dyn MigrationAdapter>> {
    match kind {
        "sqlite" => Ok(Box::new(sqlite::transfer::SqliteMigrationAdapter)),
        "text" => Ok(Box::new(text::transfer::TextMigrationAdapter)),
        _ => Err(StorageError::BackendUnavailable(kind.into())),
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StorageStatus {
    pub backend: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MigrationPreview {
    pub source: String,
    pub target: String,
    pub plan_token: String,
    pub counts: BTreeMap<String, usize>,
    pub destination_populated: bool,
    pub warnings: Vec<String>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MigrationReport {
    pub backend: String,
    pub backup_folder: String,
    pub counts: BTreeMap<String, usize>,
}

pub(crate) fn status(project: &ProjectRegistration) -> StorageResult<StorageStatus> {
    let _lock = lock(project)?;
    recover(project)?;
    let (descriptor, _) = config::resolve(project)?;
    descriptor.require_available()?;
    Ok(StorageStatus {
        backend: descriptor.backend.kind().into(),
    })
}
fn destination(root: &Path, target: &str) -> StorageResult<BTreeMap<String, Vec<u8>>> {
    let mut files = BTreeMap::new();
    if target == "text" {
        for (name, bytes) in text::journal::inventory(&root.join(".adashi/text"))? {
            if !name.starts_with("$local/") {
                files.insert(format!(".adashi/text/{name}"), bytes);
            }
        }
    } else {
        for name in [
            ".adashi/adashi.sqlite3",
            ".adashi/adashi.sqlite3-wal",
            ".adashi/adashi.sqlite3-shm",
        ] {
            text::journal::reject_symlinks(root, &root.join(name))?;
            if let Some(bytes) = text::journal::read_optional(&root.join(name))? {
                files.insert(name.into(), bytes);
            }
        }
    }
    Ok(files)
}
fn plan_token(
    project: &ProjectRegistration,
    descriptor: &StorageDescriptor,
    image: &super::transfer::ProjectImage,
    target: &str,
) -> StorageResult<String> {
    let root = Path::new(&project.folder);
    Ok(hash(
        &serde_json::to_vec(&(
            descriptor,
            image.fingerprint()?,
            target,
            destination(root, target)?,
            text::journal::read_optional(&root.join(".gitignore"))?,
        ))
        .map_err(StorageError::backend)?,
    ))
}
pub(crate) fn preview(
    project: &ProjectRegistration,
    target: &str,
) -> StorageResult<MigrationPreview> {
    let _lock = lock(project)?;
    recover(project)?;
    let (descriptor, _) = config::resolve(project)?;
    descriptor.require_available()?;
    adapter(target)?;
    if target == descriptor.backend.kind() {
        return Err(invalid("This project already uses the selected storage"));
    }
    let source = adapter(descriptor.backend.kind())?.capture(project, false)?;
    let warnings = source.image().warnings()?;
    Ok(MigrationPreview {
        source: descriptor.backend.kind().into(),
        target: target.into(),
        plan_token: plan_token(project, &descriptor, source.image(), target)?,
        counts: source.image().counts()?,
        destination_populated: !destination(Path::new(&project.folder), target)?.is_empty(),
        warnings,
    })
}
const IGNORE_BLOCK:&str="\n# Adashi portable project storage (local state and conversion backups stay private)\n!/.adashi/\n/.adashi/*\n!/.adashi/storage.json\n!/.adashi/text/\n!/.adashi/text/**\n/.adashi/text/**/.adashi-publish-*.tmp\n";
fn gitignore(root: &Path) -> StorageResult<Vec<u8>> {
    let mut original = text::journal::read_optional(&root.join(".gitignore"))?.unwrap_or_default();
    let content = std::str::from_utf8(&original).map_err(|_| {
        invalid("The existing .gitignore is not UTF-8; convert it before switching storage")
    })?;
    if !content.ends_with(IGNORE_BLOCK) {
        original.extend_from_slice(IGNORE_BLOCK.as_bytes());
    }
    Ok(original)
}
pub(crate) fn convert(
    project: &ProjectRegistration,
    target: &str,
    expected: &str,
    archive_destination: bool,
) -> StorageResult<MigrationReport> {
    let _lock = lock(project)?;
    recover(project)?;
    let root = Path::new(&project.folder);
    let (descriptor, _) = config::resolve(project)?;
    descriptor.require_available()?;
    if target == descriptor.backend.kind() {
        return Err(invalid("This project already uses the selected storage"));
    }
    let target_adapter = adapter(target)?;
    let source = adapter(descriptor.backend.kind())?.capture(project, true)?;
    source.image().warnings()?;
    if plan_token(project, &descriptor, source.image(), target)? != expected {
        return Err(invalid("Project data, selection or destination changed after preview; review a fresh conversion before continuing"));
    }
    let previous = destination(root, target)?;
    if !previous.is_empty() && !archive_destination {
        return Err(invalid("The destination already contains data. Review and explicitly choose to preserve that destination in a backup before replacing it"));
    }
    let generation = uuid::Uuid::new_v4().to_string();
    let backup = crate::settings::project_data_dir(project)
        .join("local/migrations")
        .join(&generation);
    text::journal::reject_symlinks(root, &backup)?;
    source.backup(&backup.join("source"))?;
    text::journal::atomic_write(
        &backup.join("source/.adashi/local/text-identities.json"),
        &text::transfer::image_bytes(source.image())?,
    )?;
    text::journal::atomic_write(
        &backup.join("source/.adashi/storage.json"),
        &serde_json::to_vec_pretty(&descriptor).map_err(StorageError::backend)?,
    )?;
    let staged = backup.join("destination");
    target_adapter.stage(source.image(), &staged, &generation)?;
    let actual = target_adapter.inspect_staged(&staged)?;
    let counts = source.image().counts()?;
    if counts != actual.counts()?
        || source.image().semantic_fingerprint()? != actual.semantic_fingerprint()?
    {
        return Err(invalid("Staged conversion failed full data verification. The source remains selected and its backup is retained"));
    }
    source.ensure_unchanged()?;
    if config::resolve(project)?.0 != descriptor
        || plan_token(project, &descriptor, source.image(), target)? != expected
    {
        return Err(invalid(
            "Source or destination changed during conversion; the source remains selected",
        ));
    }
    let selected = StorageDescriptor {
        schema_version: 1,
        backend: if target == "text" {
            BackendSelection::Text {}
        } else {
            BackendSelection::Sqlite {}
        },
        generation: Some(generation),
    };
    let mut after = previous
        .into_keys()
        .map(|p| (p, None))
        .collect::<BTreeMap<_, _>>();
    for (name, bytes) in destination(&staged, target)? {
        after.insert(name, Some(bytes));
    }
    after.insert(
        ".adashi/local/state.json".into(),
        text::journal::read_optional(&staged.join(".adashi/local/state.json"))?,
    );
    after.insert(
        ".adashi/local/text-identities.json".into(),
        Some(text::transfer::image_bytes(source.image())?),
    );
    after.insert(".gitignore".into(), Some(gitignore(root)?));
    after.insert(
        journal::DESCRIPTOR.into(),
        Some(serde_json::to_vec_pretty(&selected).map_err(StorageError::backend)?),
    );
    let activation = journal::Activation::prepare(root, after)?;
    activation.save(root, &backup)?;
    // Local checkout state is part of the image and is shared by adapters. The
    // journal publishes it, so check the source immediately before publication.
    let result = activation.publish(root, || source.ensure_unchanged());
    drop(source);
    if let Err(error) = result {
        recover(project)?;
        if config::resolve(project)?.0 != selected {
            return Err(error);
        }
    }
    Ok(MigrationReport {
        backend: target.into(),
        backup_folder: backup.to_string_lossy().into_owned(),
        counts,
    })
}
