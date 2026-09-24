//! Git-friendly canonical records with a disposable domain-query projection.
use super::{api, sqlite, StorageError, StorageResult};
use api::{ReadSnapshot, StorageBackend, StorageFactory};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

mod codec;
mod engine;
mod guards;
pub(crate) mod journal;
mod local;
mod records;
#[cfg(test)]
mod tests;
pub(crate) mod transfer;
mod validation;

type Files = BTreeMap<String, Vec<u8>>;
const MAX_SAFE: i64 = 9_007_199_254_740_991;
fn invalid(path: &str, message: &str) -> StorageError {
    StorageError::Validation(format!("{path}: {message}"))
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn numeric_digest(hash: &str) -> i64 {
    ((u64::from_str_radix(&hash[..14], 16).unwrap() & MAX_SAFE as u64).max(1)) as i64
}
fn domain_digest(files: &Files, local: &local::LocalState) -> StorageResult<String> {
    let mut hash = Sha256::new();
    for (path, bytes) in files.iter().filter(|(p, _)| {
        !p.starts_with("records/mutation_operations/") && !p.starts_with("$local/")
    }) {
        hash.update((path.len() as u64).to_le_bytes());
        hash.update(path.as_bytes());
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    hash.update(local.change_fingerprint()?);
    Ok(format!("{hash:x}", hash = hash.finalize()))
}

pub(crate) struct TextFactory;
pub(crate) struct TextStorage {
    root: PathBuf,
    local: PathBuf,
    request: api::OpenRequest,
    projection: Option<sqlite::SqliteStorage>,
    cached: Files,
    closed: bool,
}

struct Loaded {
    files: Files,
    records: BTreeMap<String, codec::Record>,
    rows: engine::Rows,
    tables: Vec<engine::Table>,
    db: rusqlite::Connection,
    project: i64,
    local: local::LocalState,
}

impl StorageFactory for TextFactory {
    fn open(&self, request: &api::OpenRequest) -> StorageResult<Box<dyn StorageBackend>> {
        let root = PathBuf::from(&request.location);
        let local = root
            .parent()
            .ok_or_else(|| invalid("text", "missing project directory"))?
            .join("local");
        let mut store = TextStorage {
            root,
            local,
            request: request.clone(),
            projection: None,
            cached: Files::new(),
            closed: false,
        };
        let _lock = store.lock()?;
        journal::recover(&store.root, &store.local)?;
        if !store.root.join("format.json").exists() {
            if !matches!(request.mode, api::OpenMode::InitializeOrMigrate) {
                return Err(invalid("text", "project requires explicit initialization"));
            }
            let sqlite = store.root.parent().unwrap().join("adashi.sqlite3");
            if sqlite.exists() {
                return Err(invalid("storage.json","SQLite data already exists; select SQLite and use the verified migration workflow"));
            }
            store.initialize()?;
        }
        store.refresh()?;
        Ok(Box::new(store))
    }
}

impl TextStorage {
    fn lock(&self) -> StorageResult<journal::FileLock> {
        if self.closed {
            return Err(StorageError::Closed);
        }
        journal::FileLock::acquire(&self.local.join("text.lock"))
    }
    fn initialize(&mut self) -> StorageResult<()> {
        let before = journal::inventory(&self.root)?;
        if before.keys().any(|p| !p.starts_with("$local/")) {
            return Err(invalid(
                "text",
                "refusing to initialize a populated directory",
            ));
        }
        let mut db = engine::empty()?;
        sqlite::seed::seed_initial_data(
            &mut db,
            &crate::settings::ProjectSettings {
                id: self.request.registered_identity.id.clone(),
                name: self.request.registered_identity.name.clone(),
                folder: String::new(),
            },
        )
        .map_err(StorageError::backend)?;
        let project = db
            .query_row("SELECT id FROM projects", [], |r| r.get(0))
            .map_err(StorageError::backend)?;
        sqlite::memory::ensure_project_memory(&db, project).map_err(StorageError::backend)?;
        sqlite::fixed_hooks::ensure_fixed_hook_prompts(&db).map_err(StorageError::backend)?;
        db.execute_batch(include_str!("../../concurrency_schema.sql"))
            .map_err(StorageError::backend)?;
        let tables = engine::tables(&db)?;
        let rows = engine::dump(&db, &tables)?;
        validation::validate(&db, &rows)?;
        let mut after =
            engine::export(&rows, &tables, &BTreeMap::new(), &engine::Rows::new(), true)?;
        for (p, b) in before.iter().filter(|(p, _)| p.starts_with("$local/")) {
            after.insert(p.clone(), b.clone());
        }
        journal::publish(&self.root, &self.local, &before, &after, "text-initialize")
    }
    fn load(&self) -> StorageResult<Loaded> {
        let files = journal::inventory(&self.root)?;
        let records = engine::parse_records(&files)?;
        let db = engine::empty()?;
        let tables = engine::tables(&db)?;
        let rows = engine::rows_from_records(&records, &tables)?;
        engine::import(&db, &tables, &rows)?;
        let project = validation::validate(&db, &rows)?;
        let mut local = local::LocalState::read(&files)?;
        local.install(&db, project)?;
        // Current checkout mappings are local, never inferred into shared records.
        db.execute("INSERT OR IGNORE INTO project_computers(project_id,computer_id,repository_path) VALUES(?1,?2,?3)",rusqlite::params![project,self.request.computer_id,self.request.checkout_path]).map_err(StorageError::backend)?;
        local.capture(&db, project)?;
        if journal::inventory(&self.root)? != files {
            return Err(invalid(
                "text",
                "files changed while reading; retry after the external operation finishes",
            ));
        }
        Ok(Loaded {
            files,
            records,
            rows,
            tables,
            db,
            project,
            local,
        })
    }
    fn refresh(&mut self) -> StorageResult<()> {
        let files = journal::inventory(&self.root)?;
        if self.projection.is_some() && files == self.cached {
            return Ok(());
        }
        let loaded = self.load()?;
        let hash = domain_digest(&loaded.files, &loaded.local)?;
        let revision = numeric_digest(&hash);
        let versions = guards::calculate(&loaded.local.guard_rows(&loaded.rows, loaded.project))?;
        guards::remember(&self.local, &versions)?;
        guards::install(&loaded.db, loaded.project, &versions)?;
        context(
            &loaded.db,
            loaded.project,
            &hash,
            revision,
            self.request.cursor_scope.as_deref(),
        )?;
        self.cached = loaded.files;
        self.projection = Some(sqlite::SqliteStorage::from_projection(
            loaded.db,
            loaded.project,
        ));
        Ok(())
    }
}

fn context(
    db: &rusqlite::Connection,
    project: i64,
    hash: &str,
    revision: i64,
    scope: Option<&str>,
) -> StorageResult<()> {
    db.execute_batch("CREATE TEMP TABLE IF NOT EXISTS adashi_text_context(cursor TEXT NOT NULL); DELETE FROM temp.adashi_text_context;").map_err(StorageError::backend)?;
    db.execute(
        "INSERT INTO temp.adashi_text_context(cursor) VALUES(?1)",
        [match scope {
            Some(scope) => format!("selected:{scope}:text:{hash}"),
            None => format!("text:{hash}"),
        }],
    )
    .map_err(StorageError::backend)?;
    db.execute(
        "UPDATE project_state SET revision=?1 WHERE project_id=?2",
        rusqlite::params![revision, project],
    )
    .map_err(StorageError::backend)?;
    Ok(())
}

/// Text labels are independent of immutable aliases and never use a shared counter.
pub(crate) fn random_number(db: &rusqlite::Connection, table: &str) -> Result<Option<i64>, String> {
    if !db
        .table_exists(Some("temp"), "adashi_text_context")
        .map_err(|e| e.to_string())?
    {
        return Ok(None);
    }
    if !["agent_tasks", "qa_jobs"].contains(&table) {
        return Err("Invalid label collection".into());
    }
    for _ in 0..100 {
        let random = uuid::Uuid::new_v4();
        let b = random.as_bytes();
        let number = ((u32::from_le_bytes([b[0], b[1], b[2], b[3]]) & 0x7fff_ffff) as i64).max(1);
        let exists: bool = db
            .query_row(
                &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE number=?1)"),
                [number],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if !exists {
            return Ok(Some(number));
        }
    }
    Err("Could not allocate an independent display number".into())
}

impl StorageBackend for TextStorage {
    fn snapshot(&mut self) -> StorageResult<Box<dyn ReadSnapshot + '_>> {
        let _lock = self.lock()?;
        journal::recover(&self.root, &self.local)?;
        self.refresh()?;
        self.projection
            .as_mut()
            .ok_or(StorageError::Closed)?
            .snapshot()
    }
    fn commit(&mut self, prepared: api::PreparedMutation) -> StorageResult<api::CommitResult> {
        if matches!(self.request.mode, api::OpenMode::ReadOnly) {
            return Err(StorageError::AccessDenied);
        }
        let _lock = self.lock()?;
        journal::recover(&self.root, &self.local)?;
        let mut loaded = self.load()?;
        let operation = &prepared.mutation().operation_id;
        if let Some(result) = prepared
            .replay(sqlite::snapshot::receipt(&loaded.db, loaded.project, operation)?.as_ref())?
        {
            return Ok(result);
        }
        let versions = guards::calculate(&loaded.local.guard_rows(&loaded.rows, loaded.project))?;
        guards::remember(&self.local, &versions)?;
        let raw = guards::translate(&prepared, &versions)?;
        loaded
            .local
            .require_owned_claims(&loaded.db, prepared.mutation())?;
        validation::prepare_ids(&loaded.db, &loaded.tables, &loaded.records)?;
        context(
            &loaded.db,
            loaded.project,
            &domain_digest(&loaded.files, &loaded.local)?,
            0,
            self.request.cursor_scope.as_deref(),
        )?;
        let mut projection = sqlite::SqliteStorage::from_projection(loaded.db, loaded.project);
        let mut result = projection.commit(raw)?;
        if prepared
            .mutation()
            .changes
            .iter()
            .any(|c| matches!(c, api::Change::Qa(api::QaWrite::StartRun { .. })))
        {
            for o in &result.outcomes {
                if let api::ChangeOutcome::QaRun(run) = o {
                    loaded.local.owned_runs.insert(run.id);
                }
            }
        }
        let db = projection.connection()?;
        // Keep the original request fingerprint and exposed result locally,
        // replacing the projection's internal receipt with translated guards.
        db.execute(
            "DELETE FROM mutation_operations WHERE project_id=?1 AND operation_id=?2",
            rusqlite::params![loaded.project, operation],
        )
        .map_err(StorageError::backend)?;
        let rows = engine::dump(db, &loaded.tables)?;
        validation::validate(db, &rows)?;
        loaded.local.capture(db, loaded.project)?;
        let mut interim =
            engine::export(&rows, &loaded.tables, &loaded.records, &loaded.rows, false)?;
        preserve_unchanged(&loaded.files, &mut interim)?;
        let hash = domain_digest(&interim, &loaded.local)?;
        let revision = numeric_digest(&hash);
        let versions = guards::calculate(&loaded.local.guard_rows(&rows, loaded.project))?;
        guards::remember(&self.local, &versions)?;
        guards::result(
            &mut result,
            &versions,
            api::ChangeCursor::from_token(format!("text:{hash}"))
                .in_scope(self.request.cursor_scope.as_deref()),
            revision,
        );
        sqlite::concurrency::record_no_op(
            db,
            loaded.project,
            operation,
            &prepared.receipt(result.clone()),
        )
        .map_err(StorageError::backend)?;
        loaded.local.capture(db, loaded.project)?;
        loaded.local.add_to(&mut interim)?;
        journal::publish(&self.root, &self.local, &loaded.files, &interim, operation)?;
        self.projection = None;
        self.cached.clear();
        Ok(result)
    }
    fn poll_changes(
        &mut self,
        after: &api::ChangeCursor,
    ) -> StorageResult<api::ChangeNotification> {
        let cursor = self.snapshot()?.metadata().cursor.clone();
        Ok(if *after == cursor {
            api::ChangeNotification::Unchanged { cursor }
        } else {
            api::ChangeNotification::Reset { cursor }
        })
    }
    fn publish_intents(
        &mut self,
        update: &api::IntentUpdate,
    ) -> StorageResult<Vec<api::coordination::ResourceIntent>> {
        if matches!(self.request.mode, api::OpenMode::ReadOnly) {
            return Err(StorageError::AccessDenied);
        }
        let _lock = self.lock()?;
        journal::recover(&self.root, &self.local)?;
        let mut loaded = self.load()?;
        let mut projection = sqlite::SqliteStorage::from_projection(loaded.db, loaded.project);
        let result = projection.publish_intents(update)?;
        loaded
            .local
            .capture(projection.connection()?, loaded.project)?;
        let mut after = loaded.files.clone();
        loaded.local.add_to(&mut after)?;
        journal::publish(
            &self.root,
            &self.local,
            &loaded.files,
            &after,
            "local-intent",
        )?;
        self.projection = None;
        Ok(result)
    }
    fn close(&mut self) -> StorageResult<()> {
        self.projection = None;
        self.cached.clear();
        self.closed = true;
        Ok(())
    }
}

fn preserve_unchanged(before: &Files, after: &mut Files) -> StorageResult<()> {
    for (path, bytes) in after.iter_mut() {
        if let Some(old) = before.get(path) {
            if codec::parse::<Value>(old, path)? == codec::parse::<Value>(bytes, path)? {
                *bytes = old.clone();
            }
        }
    }
    Ok(())
}
