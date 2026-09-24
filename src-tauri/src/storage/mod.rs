//! Project selection and the shared client boundary used by desktop and MCP.
pub use crate::rules::{NewRule, Rule};
pub use crate::settings::ProjectSettings as ProjectRegistration;
pub use adashi_storage_api as api;
pub use api::coordination::ResourceConflict;
use api::StorageFactory;
pub use api::{
    ChangeCursor, ProjectIdentity, ProjectStorage, StorageBackend, StorageClient, StorageError,
    StorageResult,
};
pub use config::{BackendSelection, DescriptorSource, StorageDescriptor};

mod config;
pub(crate) mod documents;
pub(crate) mod local_cache;
pub(crate) mod migration;
pub(crate) mod sqlite;
pub(crate) mod text;
pub(crate) mod transfer;

/// One project and one selected backend. No driver connection escapes this facade.
pub struct ProjectStore {
    descriptor: StorageDescriptor,
    descriptor_source: DescriptorSource,
    client: StorageClient<Box<dyn StorageBackend>>,
    registration: ProjectRegistration,
}
impl ProjectStore {
    pub fn open(project: &ProjectRegistration) -> StorageResult<Self> {
        let computer = crate::computer::id().map_err(StorageError::backend)?;
        Self::open_resolved(project, computer, api::OpenMode::InitializeOrMigrate)
    }
    /// Explicit lifecycle and read-only opening are available to both clients.
    pub fn open_mode(project: &ProjectRegistration, mode: api::OpenMode) -> StorageResult<Self> {
        let computer = crate::computer::id().map_err(StorageError::backend)?;
        Self::open_resolved(project, computer, mode)
    }
    #[cfg(test)]
    pub(crate) fn open_for_computer(
        project: &ProjectRegistration,
        computer: &str,
    ) -> StorageResult<Self> {
        Self::open_resolved(project, computer, api::OpenMode::InitializeOrMigrate)
    }
    fn open_resolved(
        project: &ProjectRegistration,
        computer: &str,
        mode: api::OpenMode,
    ) -> StorageResult<Self> {
        let (descriptor, _) = config::resolve(project)?;
        descriptor.require_available()?;
        let _lock = migration::lock(project)?;
        migration::recover(project)?;
        let (descriptor, descriptor_source) = config::resolve(project)?;
        descriptor.require_available()?;
        let location = if matches!(descriptor.backend, BackendSelection::Text {}) {
            crate::settings::project_data_dir(project).join("text")
        } else {
            crate::settings::project_database_path(project)
        };
        if descriptor.generation.is_some() {
            let marker = if matches!(descriptor.backend, BackendSelection::Text {}) {
                location.join("format.json")
            } else {
                location.clone()
            };
            if !marker.is_file()
                || std::fs::metadata(&marker)
                    .map_err(StorageError::backend)?
                    .len()
                    == 0
            {
                return Err(StorageError::Unavailable("The selected project storage is missing. Restore or fetch its data together with storage.json; an activated project cannot be initialized as an empty store.".into()));
            }
        }
        let request = api::OpenRequest {
            location: location.to_string_lossy().into_owned(),
            registered_identity: ProjectIdentity {
                id: project.id.clone(),
                name: project.name.clone(),
            },
            computer_id: computer.into(),
            checkout_path: project.folder.clone(),
            mode,
            cursor_scope: Some(migration::selection_token(&descriptor)?),
        };
        let backend = match descriptor.backend {
            BackendSelection::Sqlite {} => sqlite::SqliteFactory.open(&request)?,
            BackendSelection::Text {} => text::TextFactory.open(&request)?,
            _ => {
                return Err(StorageError::BackendUnavailable(
                    descriptor.backend.kind().into(),
                ))
            }
        };
        Ok(Self {
            descriptor,
            descriptor_source,
            client: StorageClient::new(backend),
            registration: project.clone(),
        })
    }
    pub fn descriptor(&self) -> &StorageDescriptor {
        &self.descriptor
    }
    pub fn descriptor_source(&self) -> DescriptorSource {
        self.descriptor_source
    }
    fn ensure_current(&mut self) -> StorageResult<()> {
        let (current, source) = config::resolve(&self.registration)?;
        if current != self.descriptor || source != self.descriptor_source {
            let _ = self.client.close();
            return Err(StorageError::Unavailable("Project storage changed; reopen the project and reload current values before saving. Unsaved drafts must keep their original guards.".into()));
        }
        Ok(())
    }
}
impl ProjectStorage for ProjectStore {
    fn snapshot(&mut self) -> StorageResult<Box<dyn api::ReadSnapshot + '_>> {
        let _lock = migration::lock(&self.registration)?;
        migration::recover(&self.registration)?;
        self.ensure_current()?;
        self.client.snapshot()
    }
    fn commit(&mut self, mutation: api::Mutation) -> StorageResult<api::CommitResult> {
        let _lock = migration::lock(&self.registration)?;
        migration::recover(&self.registration)?;
        self.ensure_current()?;
        self.client.commit(mutation)
    }
    fn poll_changes(&mut self, after: &ChangeCursor) -> StorageResult<api::ChangeNotification> {
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
        let _lock = migration::lock(&self.registration)?;
        migration::recover(&self.registration)?;
        self.ensure_current()?;
        self.client.publish_intents(update)
    }
    fn close(&mut self) -> StorageResult<()> {
        self.client.close()
    }
}

// The earlier, deliberately narrow conformance suite is retained as regression
// coverage through a test-only translation to the complete production API.
#[cfg(test)]
mod contract;
#[cfg(test)]
pub(crate) use contract::{
    RuleChange, RuleMutation, RuleMutationResult, RuleOutcome, RuleSnapshot, RuleStorage,
};
#[cfg(test)]
impl RuleStorage for ProjectStore {
    fn rules_snapshot(&mut self) -> StorageResult<RuleSnapshot> {
        let snapshot = self.snapshot()?;
        Ok(RuleSnapshot {
            project: snapshot.metadata().identity.clone(),
            cursor: snapshot.metadata().cursor.clone(),
            rules: snapshot.rules()?,
        })
    }
    fn change_cursor(&mut self) -> StorageResult<ChangeCursor> {
        Ok(self.snapshot()?.metadata().cursor.clone())
    }
    fn mutate_rules(&mut self, mutation: RuleMutation) -> StorageResult<RuleMutationResult> {
        let changes = mutation
            .changes
            .into_iter()
            .map(|v| {
                api::Change::Rule(match v {
                    RuleChange::Create { rule } => api::RuleWrite::Create { input: rule },
                    RuleChange::Update {
                        id,
                        expected_version,
                        rule,
                    } => api::RuleWrite::Update {
                        id,
                        expected_version,
                        input: rule,
                    },
                    RuleChange::Delete {
                        id,
                        expected_version,
                    } => api::RuleWrite::Delete {
                        id,
                        expected_version,
                    },
                })
            })
            .collect();
        let result = self.commit(api::Mutation {
            operation_id: mutation.operation_id,
            changes,
        })?;
        let outcomes = result
            .outcomes
            .into_iter()
            .map(|v| match v {
                api::ChangeOutcome::Rule(Some(rule)) => RuleOutcome::Saved { rule },
                api::ChangeOutcome::Rule(None) => {
                    let deleted = result
                        .versions
                        .iter()
                        .find(|v| v.resource_kind == "rule")
                        .expect("deleted rule version");
                    RuleOutcome::Deleted {
                        id: deleted.resource_id.parse().unwrap(),
                        version: deleted.version,
                    }
                }
                _ => unreachable!(),
            })
            .collect();
        Ok(RuleMutationResult {
            cursor: result.cursor,
            outcomes,
        })
    }
}
#[cfg(test)]
mod conformance;
#[cfg(test)]
mod tests;
