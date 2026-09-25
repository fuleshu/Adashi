//! Complete image conversion and semantic verification for both local adapters.
use super::*;
use crate::storage::{
    transfer::{MigrationAdapter, ProjectImage, SourceSnapshot},
    ProjectRegistration,
};
use rusqlite::{params, Connection};

pub(crate) struct TextMigrationAdapter;
struct TextSource {
    image: ProjectImage,
    root: PathBuf,
    before: Files,
    _lock: Option<journal::FileLock>,
}
impl SourceSnapshot for TextSource {
    fn image(&self) -> &ProjectImage {
        &self.image
    }
    fn ensure_unchanged(&self) -> StorageResult<()> {
        if journal::inventory(&self.root)? == self.before {
            Ok(())
        } else {
            Err(invalid(
                "conversion",
                "source text changed during conversion; reload the preview",
            ))
        }
    }
    fn backup(&self, folder: &Path) -> StorageResult<()> {
        write_text(&self.image, folder)
    }
}
impl MigrationAdapter for TextMigrationAdapter {
    fn capture(
        &self,
        project: &ProjectRegistration,
        freeze: bool,
    ) -> StorageResult<Box<dyn SourceSnapshot>> {
        let root = crate::settings::project_data_dir(project).join("text");
        let local_dir = root.parent().unwrap().join("local");
        let guard = journal::FileLock::acquire(&local_dir.join("text.lock"))?;
        journal::recover(&root, &local_dir)?;
        let store = TextStorage {
            root: root.clone(),
            local: local_dir,
            request: api::OpenRequest {
                location: root.to_string_lossy().into_owned(),
                registered_identity: api::ProjectIdentity {
                    id: project.id.clone(),
                    name: project.name.clone(),
                },
                computer_id: crate::computer::id().map_err(StorageError::backend)?.into(),
                checkout_path: project.folder.clone(),
                mode: api::OpenMode::ReadOnly,
                cursor_scope: None,
            },
            projection: None,
            cached: Files::new(),
            closed: false,
        };
        let loaded = store.load()?;
        let mut files = loaded.files.clone();
        loaded.local.add_to(&mut files)?;
        let image = ProjectImage {
            schema_version: 1,
            files,
        };
        image.validate()?;
        Ok(Box::new(TextSource {
            image,
            root,
            before: loaded.files,
            _lock: if freeze { Some(guard) } else { None },
        }))
    }
    fn stage(&self, image: &ProjectImage, folder: &Path, _generation: &str) -> StorageResult<()> {
        // Older text images may contain tracked request records. A destination
        // always uses the current format with retry results confined to local state.
        let (_, tables, rows, local, _) = decode(image)?;
        let records = engine::parse_records(&image.files)?;
        let mut files = engine::export(&rows, &tables, &records, &rows, false)?;
        local.add_to(&mut files)?;
        write_text(
            &ProjectImage {
                schema_version: 1,
                files,
            },
            folder,
        )
    }
    fn inspect_staged(&self, folder: &Path) -> StorageResult<ProjectImage> {
        let image = ProjectImage {
            schema_version: 1,
            files: journal::inventory(&folder.join(".adashi/text"))?,
        };
        image.validate()?;
        Ok(image)
    }
}

pub(crate) fn write_text(image: &ProjectImage, folder: &Path) -> StorageResult<()> {
    image.validate()?;
    for (path, bytes) in &image.files {
        let target = if path == local::PATH {
            folder.join(".adashi/local/state.json")
        } else {
            folder.join(".adashi/text").join(path)
        };
        journal::atomic_write(&target, bytes)?;
    }
    Ok(())
}

fn decode(
    image: &ProjectImage,
) -> StorageResult<(
    Connection,
    Vec<engine::Table>,
    engine::Rows,
    local::LocalState,
    i64,
)> {
    if image.schema_version != 1 {
        return Err(invalid("image", "unsupported image version"));
    }
    if image
        .files
        .keys()
        .any(|p| p.starts_with("$local/") && p != local::PATH)
    {
        return Err(invalid("image", "unknown local image file"));
    }
    let db = engine::empty()?;
    let tables = engine::tables(&db)?;
    let records = engine::parse_records(&image.files)?;
    let rows = engine::rows_from_records(&records, &tables)?;
    engine::import(&db, &tables, &rows)?;
    let project = validation::validate(&db, &rows)?;
    let mut local = local::LocalState::read(&image.files)?;
    local.install(&db, project)?;
    local.capture(&db, project)?;
    guards::calculate(&local.guard_rows(&rows, project))?;
    Ok((db, tables, rows, local, project))
}
pub(crate) fn validate_image(image: &ProjectImage) -> StorageResult<()> {
    decode(image).map(|_| ())
}
pub(crate) fn fingerprint(image: &ProjectImage) -> StorageResult<String> {
    Ok(digest(&codec::bytes(image)?))
}
pub(crate) fn semantic_fingerprint(image: &ProjectImage) -> StorageResult<String> {
    let (db, tables, _, local, project) = decode(image)?;
    let mut rows = local.guard_rows(&engine::dump(&db, &tables)?, project);
    // Resource guards are rebased on activation. Request results are local state;
    // verify their exact values separately from incidental SQL insertion times.
    for ((table, _), data) in &mut rows {
        if table == "resource_versions" {
            data.remove("version");
            data.remove("updated_at");
        }
    }
    let values = rows
        .into_iter()
        .filter(|((table, _), _)| table != "mutation_operations")
        .map(|((table, key), data)| serde_json::json!([table, key, data]))
        .collect::<Vec<_>>();
    Ok(digest(&codec::bytes(&(values, local.retry_results()))?))
}
pub(crate) fn counts(image: &ProjectImage) -> StorageResult<BTreeMap<String, usize>> {
    let (_, tables, rows, _, _) = decode(image)?;
    let mut counts = tables
        .iter()
        .filter(|t| t.name != "mutation_operations")
        .map(|t| (t.name.clone(), 0))
        .collect::<BTreeMap<_, _>>();
    for ((table, _), _) in rows
        .into_iter()
        .filter(|((table, _), _)| table != "mutation_operations")
    {
        *counts.get_mut(&table).unwrap() += 1;
    }
    Ok(counts)
}
pub(crate) fn warnings(image: &ProjectImage) -> StorageResult<Vec<String>> {
    let (_, _, rows, _, _) = decode(image)?;
    if rows
        .iter()
        .any(|((t, _), r)| t == "qa_runs" && r["status"] == "running")
    {
        return Err(invalid("conversion","finish running QA reservations before switching storage; the current source remains usable"));
    }
    let absolute = rows
        .iter()
        .filter(|((t, _), r)| {
            t == "qa_jobs"
                && r["working_directory"].as_str().is_some_and(|p| {
                    Path::new(p).is_absolute() || p.as_bytes().get(1) == Some(&b':')
                })
        })
        .count();
    Ok(if absolute > 0 {
        vec![format!("{absolute} QA job(s) use absolute working folders. These paths are preserved and may need adjustment in another clone.")]
    } else {
        vec![]
    })
}

/// Capture every classified row from a pinned SQL snapshot. Identity hints retain
/// UUIDs and tombstones across repeated SQLite/text round trips.
pub(crate) fn from_sqlite(
    db: &Connection,
    hints: Option<&ProjectImage>,
    local_dir: &Path,
) -> StorageResult<ProjectImage> {
    let tables = engine::tables(db)?;
    let rows = engine::dump(db, &tables)?;
    let project = db
        .query_row("SELECT id FROM projects", [], |r| r.get(0))
        .map_err(StorageError::backend)?;
    let mut state = journal::read_optional(&local_dir.join("state.json"))?
        .map(|b| codec::parse::<local::LocalState>(&b, "local/state.json"))
        .transpose()?
        .unwrap_or_default();
    state.capture(db, project)?;
    let mut old = hints
        .map(|h| engine::parse_records(&h.files))
        .transpose()?
        .unwrap_or_default();
    sequences::retain(db, &tables, &mut old)?;
    let old_rows = if old.is_empty() {
        engine::Rows::new()
    } else {
        engine::rows_from_records(&old, &tables)?
    };
    let mut files = engine::export(&rows, &tables, &old, &old_rows, true)?;
    state.add_to(&mut files)?;
    let image = ProjectImage {
        schema_version: 1,
        files,
    };
    image.validate()?;
    Ok(image)
}

/// Import a staged SQL database. Fresh guards ensure pre-switch drafts cannot
/// become valid merely because an imported branch reused a numeric counter.
pub(crate) fn write_sqlite(
    image: &ProjectImage,
    folder: &Path,
    generation: Option<&str>,
) -> StorageResult<()> {
    let (projection, tables, rows, mut local, project) = decode(image)?;
    let path = folder.join(".adashi/adashi.sqlite3");
    if path.exists() {
        return Err(invalid(
            "destination",
            "refusing to overwrite an existing staged database",
        ));
    }
    fs::create_dir_all(path.parent().unwrap()).map_err(StorageError::backend)?;
    let mut db = Connection::open(&path).map_err(StorageError::backend)?;
    sqlite::schema::migrate(&mut db).map_err(StorageError::backend)?;
    engine::import(&db, &tables, &rows)?;
    local.install(&db, project)?;
    validation::validate(&db, &rows)?;
    let records = engine::parse_records(&image.files)?;
    sequences::restore(&db, &tables, &records)?;
    if let Some(generation) = generation {
        let mut seen = BTreeSet::new();
        for version in sqlite::snapshot::all_versions(&db, project)? {
            let token = numeric_digest(&digest(
                format!(
                    "{generation}:{}:{}:{}",
                    version.resource_kind, version.resource_id, version.version
                )
                .as_bytes(),
            ));
            if !seen.insert(token) {
                return Err(invalid(
                    "conversion",
                    "guard fingerprint collision; retry with a fresh conversion",
                ));
            }
            db.execute("UPDATE resource_versions SET version=?1 WHERE project_id=?2 AND resource_kind=?3 AND resource_id=?4",params![token,project,version.resource_kind,version.resource_id]).map_err(StorageError::backend)?;
        }
    }
    local.capture(&db, project)?;
    journal::atomic_write(
        &folder.join(".adashi/local/state.json"),
        &codec::bytes(&local)?,
    )?;
    db.pragma_update(None, "journal_mode", "DELETE")
        .map_err(StorageError::backend)?;
    db.close().map_err(|(_, e)| StorageError::backend(e))?;
    drop(projection);
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .and_then(|f| f.sync_all())
        .map_err(StorageError::backend)
}

pub(crate) fn read_hints(local_dir: &Path) -> StorageResult<Option<ProjectImage>> {
    journal::read_optional(&local_dir.join("text-identities.json"))?
        .map(|b| codec::parse(&b, "local/text-identities.json"))
        .transpose()
}
pub(crate) fn image_bytes(image: &ProjectImage) -> StorageResult<Vec<u8>> {
    codec::bytes(image)
}
