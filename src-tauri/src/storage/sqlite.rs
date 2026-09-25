use std::{fs, time::Duration};

use rusqlite::{params, Connection, TransactionBehavior};

use super::{ChangeCursor, ProjectIdentity, StorageError, StorageResult};
#[cfg(test)]
use crate::settings;
use crate::settings::ProjectSettings;
pub(crate) mod concurrency;
pub(crate) mod design;
pub(crate) mod fixed_hooks;
pub(crate) mod health;
pub(crate) mod memory;
pub(crate) mod markdown;
pub(crate) mod mockups;
pub(crate) mod prompt_hygiene;
pub(crate) mod qa;
pub(crate) mod rules;
pub(crate) mod schema;
pub(crate) mod seed;
pub(crate) mod state;
pub(crate) mod tasks;
pub(crate) mod transfer;

mod mutations;
pub(super) mod references;
pub(super) mod snapshot;
#[cfg(test)]
pub(super) mod tests;
use adashi_storage_api::{
    check_versions, ChangeNotification, ChangeOutcome, CommitResult, IntentUpdate, OpenMode,
    OpenRequest, PreparedMutation, ReadSnapshot, ResourceKey, StorageBackend, StorageFactory,
};

pub struct SqliteFactory;

/// Internal driver failures are deliberately opaque across the adapter boundary.
pub(crate) fn failure(_: impl std::fmt::Display) -> String {
    "storage.sqlite_failure".into()
}

pub(super) struct SqliteStorage {
    db: Option<Connection>,
    project_id: i64,
    read_only: bool,
    cursor_scope: Option<String>,
}

fn require_rule(db: &Connection, project_id: i64, id: i64) -> StorageResult<()> {
    let exists = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM rules WHERE project_id=?1 AND id=?2)",
            params![project_id, id],
            |row| row.get::<_, bool>(0),
        )
        .map_err(StorageError::backend)?;
    if exists {
        Ok(())
    } else {
        Err(StorageError::NotFound { kind: "rule", id })
    }
}

impl StorageFactory for SqliteFactory {
    fn open(&self, request: &OpenRequest) -> StorageResult<Box<dyn StorageBackend>> {
        Ok(Box::new(SqliteStorage::open_request(request)?))
    }
}

impl SqliteStorage {
    /// Disposable projection used by the text adapter; never opens a disk database.
    pub(super) fn from_projection(db: Connection, project_id: i64) -> Self {
        Self {
            db: Some(db),
            project_id,
            read_only: false,
            cursor_scope: None,
        }
    }
    pub(super) fn connection(&self) -> StorageResult<&Connection> {
        self.db.as_ref().ok_or(StorageError::Closed)
    }
    pub(super) fn open_request(request: &OpenRequest) -> StorageResult<Self> {
        use rusqlite::OpenFlags;
        let initialize = matches!(request.mode, OpenMode::InitializeOrMigrate);
        let read_only = matches!(request.mode, OpenMode::ReadOnly);
        if initialize {
            let parent = std::path::Path::new(&request.location)
                .parent()
                .ok_or_else(|| {
                    StorageError::InvalidConfiguration(
                        "SQLite location needs a parent directory".into(),
                    )
                })?;
            fs::create_dir_all(parent).map_err(StorageError::backend)?;
        }
        let flags = if read_only {
            OpenFlags::SQLITE_OPEN_READ_ONLY
        } else if initialize {
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
        } else {
            OpenFlags::SQLITE_OPEN_READ_WRITE
        };
        let mut db =
            Connection::open_with_flags(&request.location, flags).map_err(StorageError::backend)?;
        db.busy_timeout(Duration::from_secs(5))
            .map_err(StorageError::backend)?;
        db.pragma_update(None, "foreign_keys", true)
            .map_err(StorageError::backend)?;
        let version: i64 = db
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(StorageError::backend)?;
        if version > schema::SCHEMA_VERSION {
            return Err(StorageError::IncompatibleSchema {
                found: version,
                supported: schema::SCHEMA_VERSION,
            });
        }
        if initialize {
            schema::migrate(&mut db).map_err(StorageError::backend)?;
            seed::seed_initial_data(
                &mut db,
                &ProjectSettings {
                    id: request.registered_identity.id.clone(),
                    name: request.registered_identity.name.clone(),
                    folder: request.checkout_path.clone(),
                },
            )
            .map_err(StorageError::backend)?;
            let initialized = version != schema::SCHEMA_VERSION || db.total_changes() > 0;
            if initialized {
                let project: i64 = db
                    .query_row("SELECT id FROM projects", [], |r| r.get(0))
                    .map_err(StorageError::backend)?;
                memory::ensure_project_memory(&db, project).map_err(StorageError::backend)?;
                fixed_hooks::ensure_fixed_hook_prompts(&db).map_err(StorageError::backend)?;
                db.execute_batch(include_str!("../concurrency_schema.sql"))
                    .map_err(StorageError::backend)?;
            }
            let registered: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM project_computers WHERE computer_id=?1 AND repository_path=?2)",params![request.computer_id,request.checkout_path],|row|row.get(0)).map_err(StorageError::backend)?;
            // Already initialized, registered opens perform no writes or write transactions.
            if !registered {
                let tx = db
                    .transaction_with_behavior(TransactionBehavior::Immediate)
                    .map_err(StorageError::backend)?;
                let changed = tx.execute("INSERT INTO project_computers(project_id,computer_id,repository_path) SELECT id,?1,?2 FROM projects WHERE true ON CONFLICT(project_id,computer_id) DO UPDATE SET repository_path=excluded.repository_path WHERE project_computers.repository_path IS NOT excluded.repository_path",params![request.computer_id,request.checkout_path]).map_err(StorageError::backend)?;
                let project: i64 = tx
                    .query_row("SELECT id FROM projects", [], |r| r.get(0))
                    .map_err(StorageError::backend)?;
                if changed > 0 {
                    concurrency::bump_version(&tx, project, "computer", &request.computer_id)
                        .map_err(StorageError::backend)?;
                    if !initialized {
                        state::bump_project_revision(&tx, project)
                            .map_err(StorageError::backend)?;
                    }
                }
                tx.commit().map_err(StorageError::backend)?;
            }
            // A pinned reader and a writer must coexist. This lifecycle step is
            // explicit; read-only/read-write opens never change journal mode.
            let journal: String = db
                .pragma_query_value(None, "journal_mode", |r| r.get(0))
                .map_err(StorageError::backend)?;
            if journal != "wal" {
                db.pragma_update(None, "journal_mode", "WAL")
                    .map_err(StorageError::backend)?;
            }
        } else if version != schema::SCHEMA_VERSION {
            return Err(StorageError::Unavailable(
                "Project requires explicit storage initialization or migration".into(),
            ));
        }
        let ids = db
            .prepare("SELECT id FROM projects ORDER BY id")
            .map_err(StorageError::backend)?
            .query_map([], |r| r.get::<_, i64>(0))
            .map_err(StorageError::backend)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(StorageError::backend)?;
        let [project_id] = ids.as_slice() else {
            return Err(StorageError::InvalidConfiguration(
                "SQLite project storage must contain exactly one project".into(),
            ));
        };
        if read_only {
            db.pragma_update(None, "query_only", true)
                .map_err(StorageError::backend)?;
        }
        Ok(Self {
            db: Some(db),
            project_id: *project_id,
            read_only,
            cursor_scope: request.cursor_scope.clone(),
        })
    }
}

impl StorageBackend for SqliteStorage {
    fn snapshot(&mut self) -> StorageResult<Box<dyn ReadSnapshot + '_>> {
        let tx = self
            .db
            .as_mut()
            .ok_or(StorageError::Closed)?
            .transaction()
            .map_err(StorageError::backend)?;
        let mut metadata = snapshot::metadata(&tx, self.project_id)?;
        metadata.cursor = metadata.cursor.in_scope(self.cursor_scope.as_deref());
        Ok(Box::new(snapshot::Snapshot { tx, metadata }))
    }
    fn commit(&mut self, prepared: PreparedMutation) -> StorageResult<CommitResult> {
        let db = self.db.as_mut().ok_or(StorageError::Closed)?;
        if self.read_only {
            return Err(StorageError::AccessDenied);
        }
        let mut tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(StorageError::backend)?;
        let operation = &prepared.mutation().operation_id;
        if let Some(result) =
            prepared.replay(snapshot::receipt(&tx, self.project_id, operation)?.as_ref())?
        {
            return Ok(result);
        }
        let before = snapshot::all_versions(&tx, self.project_id)?;
        check_versions(&prepared.expected_versions(), &before)?;
        let mut outcomes = Vec::new();
        for change in &prepared.mutation().changes {
            outcomes.push(mutations::apply(
                &mut tx,
                self.project_id,
                operation,
                change,
            )?);
        }
        references::validate(&tx, self.project_id)?;
        let versions = snapshot::all_versions(&tx, self.project_id)?
            .into_iter()
            .filter(|v| {
                !before.iter().any(|old| {
                    old.resource_kind == v.resource_kind
                        && old.resource_id == v.resource_id
                        && old.version == v.version
                })
            })
            .collect::<Vec<_>>();
        let changed = !versions.is_empty();
        if changed {
            state::bump_project_revision(&tx, self.project_id).map_err(StorageError::backend)?;
        }
        let mut metadata = snapshot::metadata(&tx, self.project_id)?;
        metadata.cursor = metadata.cursor.in_scope(self.cursor_scope.as_deref());
        let mut read_tokens = Vec::new();
        for outcome in &mut outcomes {
            if let ChangeOutcome::Design(value) = outcome {
                value.revision = metadata.revision;
                let ids = value
                    .read_tokens
                    .iter()
                    .map(|v| v.document_id.clone())
                    .collect::<Vec<_>>();
                value.read_tokens = design::documents::load_documents(&tx, self.project_id, &ids)
                    .map_err(mutations::domain_error)?
                    .into_iter()
                    .map(|doc| adashi_storage_api::documents::DocumentReadToken {
                        document_id: doc.document_id,
                        read_token: doc.read_token,
                    })
                    .collect();
                read_tokens.extend(value.read_tokens.clone());
            }
        }
        let result = CommitResult {
            cursor: metadata.cursor,
            revision: metadata.revision,
            changed,
            outcomes,
            versions,
            read_tokens,
        };
        concurrency::record_no_op(
            &tx,
            self.project_id,
            operation,
            &prepared.receipt(result.clone()),
        )
        .map_err(StorageError::backend)?;
        tx.commit().map_err(StorageError::backend)?;
        Ok(result)
    }
    fn poll_changes(&mut self, after: &ChangeCursor) -> StorageResult<ChangeNotification> {
        let db = self.db.as_ref().ok_or(StorageError::Closed)?;
        let mut metadata = snapshot::metadata(db, self.project_id)?;
        metadata.cursor = metadata.cursor.in_scope(self.cursor_scope.as_deref());
        let cursor = metadata.cursor;
        if *after == cursor {
            return Ok(ChangeNotification::Unchanged { cursor });
        }
        let value = serde_json::to_value(after).map_err(StorageError::backend)?;
        let prefix = format!("sqlite:{}:", metadata.identity.id);
        let known = value
            .as_str()
            .and_then(|s| s.strip_prefix(&prefix))
            .and_then(|s| s.parse::<i64>().ok())
            .is_some_and(|r| r >= 0 && r <= metadata.revision);
        Ok(if known {
            ChangeNotification::Changed { cursor }
        } else {
            ChangeNotification::Reset { cursor }
        })
    }
    fn publish_intents(
        &mut self,
        update: &IntentUpdate,
    ) -> StorageResult<Vec<adashi_storage_api::coordination::ResourceIntent>> {
        let db = self.db.as_mut().ok_or(StorageError::Closed)?;
        if self.read_only {
            return Err(StorageError::AccessDenied);
        }
        adashi_storage_api::validate_intent(update)?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(StorageError::backend)?;
        for key in &update.resources {
            if update.ttl_seconds == 0 {
                tx.execute("DELETE FROM resource_intents WHERE project_id=?1 AND agent_run_id=?2 AND resource_kind=?3 AND resource_id=?4",params![self.project_id,update.agent_run_id,key.kind,key.id]).map_err(StorageError::backend)?;
            } else {
                concurrency::publish_intent(
                    &tx,
                    self.project_id,
                    &update.agent_run_id,
                    &key.kind,
                    &key.id,
                    i64::from(update.ttl_seconds),
                )
                .map_err(StorageError::backend)?;
            }
        }
        let result =
            concurrency::load_live_intents(&tx, self.project_id).map_err(StorageError::backend)?;
        tx.commit().map_err(StorageError::backend)?;
        Ok(result)
    }
    fn close(&mut self) -> StorageResult<()> {
        if let Some(db) = self.db.take() {
            if let Err((db, error)) = db.close() {
                self.db = Some(db);
                return Err(StorageError::backend(error));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) fn open_test_database(
    project: &ProjectSettings,
    computer_id: &str,
) -> StorageResult<Connection> {
    let (descriptor, _) = super::config::resolve(project)?;
    descriptor.require_available()?;
    let mut store = SqliteStorage::open_request(&OpenRequest {
        location: settings::project_database_path(project)
            .to_string_lossy()
            .into_owned(),
        registered_identity: ProjectIdentity {
            id: project.id.clone(),
            name: project.name.clone(),
        },
        computer_id: computer_id.into(),
        checkout_path: project.folder.clone(),
        mode: OpenMode::InitializeOrMigrate,
        cursor_scope: None,
    })?;
    store.db.take().ok_or(StorageError::Closed)
}

#[cfg(test)]
pub(crate) fn test_snapshot(db: &Connection, project_id: i64) -> Box<dyn ReadSnapshot + '_> {
    let tx = db.unchecked_transaction().unwrap();
    let metadata = snapshot::metadata(&tx, project_id).unwrap();
    Box::new(snapshot::Snapshot { tx, metadata })
}
