//! Consistent SQLite migration source, held behind SQLite's own writer barrier.
use super::*;
use crate::storage::{
    text,
    transfer::{MigrationAdapter, ProjectImage, SourceSnapshot},
    ProjectRegistration,
};
use std::path::{Path, PathBuf};

pub(crate) struct SqliteMigrationAdapter;
struct SqliteSource {
    db: Connection,
    image: ProjectImage,
    project: ProjectRegistration,
    local_dir: PathBuf,
}
impl SourceSnapshot for SqliteSource {
    fn image(&self) -> &ProjectImage {
        &self.image
    }
    fn ensure_unchanged(&self) -> StorageResult<()> {
        let current = text::transfer::from_sqlite(&self.db, Some(&self.image), &self.local_dir)?;
        if current.fingerprint()? != self.image.fingerprint()? {
            return Err(StorageError::Validation(
                "SQLite source changed during conversion; reload the preview".into(),
            ));
        }
        Ok(())
    }
    fn backup(&self, folder: &Path) -> StorageResult<()> {
        fs::create_dir_all(folder.join(".adashi")).map_err(StorageError::backend)?;
        // SQLite cannot back up through a connection holding a write transaction.
        // A second reader sees the same committed state while our writer barrier
        // prevents any intervening commits, including from non-Adashi clients.
        let reader = Connection::open_with_flags(
            crate::settings::project_database_path(&self.project),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(StorageError::backend)?;
        reader
            .backup("main", folder.join(".adashi/adashi.sqlite3"), None)
            .map_err(StorageError::backend)?;
        // Preserve local machine/provenance records in the backup as well.
        if let Some(bytes) = self.image.files.get("$local/state.json") {
            text::journal::atomic_write(&folder.join(".adashi/local/state.json"), bytes)?;
        }
        Ok(())
    }
}
impl MigrationAdapter for SqliteMigrationAdapter {
    fn capture(
        &self,
        project: &ProjectRegistration,
        freeze: bool,
    ) -> StorageResult<Box<dyn SourceSnapshot>> {
        let path = crate::settings::project_database_path(project);
        let db = Connection::open_with_flags(
            path,
            if freeze {
                rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
            } else {
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
            },
        )
        .map_err(StorageError::backend)?;
        db.busy_timeout(Duration::from_secs(5))
            .map_err(StorageError::backend)?;
        let version: i64 = db
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(StorageError::backend)?;
        if version != schema::SCHEMA_VERSION {
            return Err(StorageError::Validation(
                "Initialize or upgrade SQLite before converting its storage".into(),
            ));
        }
        db.execute_batch(if freeze { "BEGIN IMMEDIATE" } else { "BEGIN" })
            .map_err(StorageError::backend)?;
        let local_dir = crate::settings::project_data_dir(project).join("local");
        let hints = text::transfer::read_hints(&local_dir)?;
        let image = text::transfer::from_sqlite(&db, hints.as_ref(), &local_dir)?;
        Ok(Box::new(SqliteSource {
            db,
            image,
            project: project.clone(),
            local_dir,
        }))
    }
    fn stage(&self, image: &ProjectImage, folder: &Path, generation: &str) -> StorageResult<()> {
        text::transfer::write_sqlite(image, folder, Some(generation))
    }
    fn inspect_staged(&self, folder: &Path) -> StorageResult<ProjectImage> {
        let db = Connection::open_with_flags(
            folder.join(".adashi/adashi.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(StorageError::backend)?;
        text::transfer::from_sqlite(&db, None, &folder.join(".adashi/local"))
    }
}
