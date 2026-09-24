//! Checkout-local publication protocol. Cooperating readers recover before use.
use super::*;
use std::{
    fs::{File, OpenOptions},
    io::Write,
    time::{Duration, Instant},
};

pub(crate) struct FileLock {
    _file: File,
}
impl FileLock {
    pub(crate) fn acquire(path: &Path) -> StorageResult<Self> {
        reject_symlinks(path.parent().and_then(Path::parent).unwrap_or(path), path)?;
        fs::create_dir_all(
            path.parent()
                .ok_or_else(|| invalid("lock", "missing parent"))?,
        )
        .map_err(StorageError::backend)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)
            .map_err(StorageError::backend)?;
        let start = Instant::now();
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(Self { _file: file }),
                Err(std::fs::TryLockError::WouldBlock)
                    if start.elapsed() < Duration::from_secs(5) =>
                {
                    std::thread::sleep(Duration::from_millis(20))
                }
                Err(std::fs::TryLockError::WouldBlock) => return Err(StorageError::TimedOut),
                Err(e) => return Err(StorageError::backend(e)),
            }
        }
    }
}

pub(crate) fn atomic_write(path: &Path, data: &[u8]) -> StorageResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid("publication", "missing parent"))?;
    fs::create_dir_all(parent).map_err(StorageError::backend)?;
    let mut file = tempfile::Builder::new()
        .prefix(".adashi-publish-")
        .suffix(".tmp")
        .tempfile_in(parent)
        .map_err(StorageError::backend)?;
    file.write_all(data).map_err(StorageError::backend)?;
    file.as_file().sync_all().map_err(StorageError::backend)?;
    file.persist(path).map_err(StorageError::backend)?;
    #[cfg(unix)]
    File::open(parent)
        .and_then(|f| f.sync_all())
        .map_err(StorageError::backend)?;
    Ok(())
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Journal {
    schema_version: u32,
    files: Vec<Replacement>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Replacement {
    path: String,
    before: Option<String>,
    after: Option<String>,
}

pub(crate) fn safe_path(root: &Path, name: &str) -> StorageResult<PathBuf> {
    if name == super::local::PATH {
        let path = root
            .parent()
            .ok_or_else(|| invalid(name, "missing parent"))?
            .join("local/state.json");
        reject_symlinks(root.parent().unwrap(), &path)?;
        return Ok(path);
    }
    let parts: Vec<_> = name.split('/').collect();
    let valid = name == "format.json"
        || (parts.len() == 3
            && parts[0] == "records"
            && engine::COLLECTIONS.contains(&parts[1])
            && parts[2]
                .strip_suffix(".json")
                .is_some_and(codec::valid_identity));
    if !valid {
        return Err(invalid(name, "invalid journal path"));
    }
    let path = root.join(name);
    reject_symlinks(root, &path)?;
    Ok(path)
}

pub(super) fn recover(root: &Path, local: &Path) -> StorageResult<()> {
    let pending = local.join("text-transaction.json");
    let data = match fs::read(&pending) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(StorageError::backend(e)),
    };
    let journal: Journal = codec::parse(&data, "local/text-transaction.json")?;
    if ![1, 2].contains(&journal.schema_version) {
        return Err(invalid(
            "local/text-transaction.json",
            "unsupported journal schema",
        ));
    }
    let mut seen = BTreeSet::new();
    // Validate every preimage before changing any file, including late entries.
    for item in &journal.files {
        if item.after.is_none()
            && (journal.schema_version != 2
                || !item.path.starts_with("records/mutation_operations/"))
        {
            return Err(invalid(
                &item.path,
                "only legacy request history may be removed without a tombstone",
            ));
        }
        if !seen.insert(&item.path) {
            return Err(invalid(&item.path, "duplicate journal path"));
        }
        let path = safe_path(root, &item.path)?;
        let current = read_optional(&path)?;
        if current.as_deref() != item.before.as_deref().map(str::as_bytes)
            && current.as_deref() != item.after.as_deref().map(str::as_bytes)
        {
            return Err(invalid(&item.path,"recovery found an external edit; preserve the journal and reconcile this file with its before/after images"));
        }
    }
    for item in &journal.files {
        let path = safe_path(root, &item.path)?;
        if read_optional(&path)?.as_deref() != item.after.as_deref().map(str::as_bytes) {
            match &item.after {
                Some(bytes) => atomic_write(&path, bytes.as_bytes())?,
                None => {
                    fs::remove_file(&path).map_err(StorageError::backend)?;
                    #[cfg(unix)]
                    File::open(path.parent().unwrap())
                        .and_then(|f| f.sync_all())
                        .map_err(StorageError::backend)?;
                }
            }
        }
    }
    fs::remove_file(pending).map_err(StorageError::backend)
}

fn replacements(before: &Files, after: &Files) -> StorageResult<Vec<Replacement>> {
    let mut files = after
        .iter()
        .filter(|(p, b)| before.get(*p) != Some(*b))
        .map(|(p, b)| {
            Ok(Replacement {
                path: p.clone(),
                before: before
                    .get(p)
                    .map(|v| String::from_utf8(v.clone()).map_err(StorageError::backend))
                    .transpose()?,
                after: Some(String::from_utf8(b.clone()).map_err(StorageError::backend)?),
            })
        })
        .collect::<StorageResult<Vec<_>>>()?;
    for (path, bytes) in before.iter().filter(|(p, _)| !after.contains_key(*p)) {
        if !path.starts_with("records/mutation_operations/") {
            return Err(invalid(
                "publication",
                "domain deletions must retain tombstones",
            ));
        }
        files.push(Replacement {
            path: path.clone(),
            before: Some(String::from_utf8(bytes.clone()).map_err(StorageError::backend)?),
            after: None,
        });
    }
    Ok(files)
}

pub(super) fn publish(
    root: &Path,
    local: &Path,
    before: &Files,
    after: &Files,
    operation: &str,
) -> StorageResult<()> {
    if inventory(root)? != *before {
        return Err(invalid(
            "text",
            "files changed externally; reload after Git or the editor finishes",
        ));
    }
    let files = replacements(before, after)?;
    if files.is_empty() {
        return Ok(());
    }
    let prepared = atomic_write(
        &local.join("text-transaction.json"),
        &codec::bytes(&Journal {
            schema_version: 2,
            files,
        })?,
    );
    if let Err(error) = prepared {
        return Err(if local.join("text-transaction.json").exists() {
            StorageError::CommitUncertain {
                operation_id: operation.into(),
            }
        } else {
            error
        });
    }
    recover(root, local).map_err(|_| StorageError::CommitUncertain {
        operation_id: operation.into(),
    })
}

pub(crate) fn read_optional(path: &Path) -> StorageResult<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(StorageError::backend(e)),
    }
}

pub(crate) fn reject_symlinks(root: &Path, path: &Path) -> StorageResult<()> {
    let mut item = Some(path);
    while let Some(p) = item {
        if let Ok(meta) = fs::symlink_metadata(p) {
            if meta.file_type().is_symlink() {
                return Err(invalid(
                    &p.display().to_string(),
                    "symbolic links are not permitted in storage files",
                ));
            }
        }
        if p == root {
            break;
        }
        item = p.parent();
    }
    Ok(())
}

pub(crate) fn inventory(root: &Path) -> StorageResult<Files> {
    let mut result = Files::new();
    fn walk(root: &Path, dir: &Path, result: &mut Files) -> StorageResult<()> {
        reject_symlinks(root, dir)?;
        let entries = match fs::read_dir(dir) {
            Ok(v) => v,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(StorageError::backend(e)),
        };
        for entry in entries {
            let entry = entry.map_err(StorageError::backend)?;
            let path = entry.path();
            reject_symlinks(root, &path)?;
            let name = path
                .strip_prefix(root)
                .map_err(StorageError::backend)?
                .to_string_lossy()
                .replace('\\', "/");
            if entry.file_type().map_err(StorageError::backend)?.is_dir() {
                if name != "records"
                    && !engine::COLLECTIONS
                        .iter()
                        .any(|c| name == format!("records/{c}"))
                {
                    return Err(invalid(&name, "unknown collection directory"));
                }
                walk(root, &path, result)?;
            } else {
                if entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".adashi-publish-")
                    && entry.file_name().to_string_lossy().ends_with(".tmp")
                {
                    continue;
                }
                safe_path(root, &name)?;
                if entry.metadata().map_err(StorageError::backend)?.len() > 64 * 1024 * 1024 {
                    return Err(invalid(&name, "record exceeds the 64 MiB limit"));
                }
                result.insert(name, fs::read(path).map_err(StorageError::backend)?);
            }
        }
        Ok(())
    }
    walk(root, root, &mut result)?;
    if let Some(data) = read_optional(&safe_path(root, super::local::PATH)?)? {
        result.insert(super::local::PATH.into(), data);
    }
    Ok(result)
}

#[cfg(test)]
pub(super) fn stage_for_test(local: &Path, before: &Files, after: &Files) -> StorageResult<()> {
    atomic_write(
        &local.join("text-transaction.json"),
        &codec::bytes(&Journal {
            schema_version: 2,
            files: replacements(before, after)?,
        })?,
    )
}
