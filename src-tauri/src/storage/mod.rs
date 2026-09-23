//! Shared project storage boundary. See docs/project-storage.md for the extraction plan.
//!
//! Task 13 supplies configuration, lifecycle and a typed rules slice. Other domains
//! use the explicit SQLite bridge until task 14; no alternate store is synthesized.

mod config;
mod contract;
mod sqlite;

pub use config::{BackendSelection, DescriptorSource, StorageDescriptor};
pub use contract::{
    ChangeCursor, ProjectIdentity, ProjectStorage, ResourceConflict, RuleChange, RuleMutation,
    RuleMutationResult, RuleOutcome, RuleSnapshot, StorageError, StorageResult,
};

pub use crate::rules::{NewRule, Rule};
pub use crate::settings::ProjectSettings as ProjectRegistration;
use crate::settings::ProjectSettings;
use sqlite::SqliteStorage;

/// One project and one resolved backend. No process-global active connection.
pub struct ProjectStore {
    descriptor: StorageDescriptor,
    descriptor_source: DescriptorSource,
    backend: Backend,
}

enum Backend {
    Sqlite(SqliteStorage),
}

impl ProjectStore {
    pub fn open(project: &ProjectRegistration) -> StorageResult<Self> {
        let (descriptor, source) = config::resolve(project)?;
        descriptor.require_available()?;
        let computer_id = crate::computer::id().map_err(StorageError::backend)?;
        Self::open_resolved(project, computer_id, descriptor, source)
    }

    #[cfg(test)]
    pub(crate) fn open_for_computer(
        project: &ProjectSettings,
        computer_id: &str,
    ) -> StorageResult<Self> {
        let (descriptor, source) = config::resolve(project)?;
        descriptor.require_available()?;
        Self::open_resolved(project, computer_id, descriptor, source)
    }

    fn open_resolved(
        project: &ProjectSettings,
        computer_id: &str,
        descriptor: StorageDescriptor,
        descriptor_source: DescriptorSource,
    ) -> StorageResult<Self> {
        let backend = match descriptor.backend {
            BackendSelection::Sqlite {} => {
                Backend::Sqlite(SqliteStorage::open(project, computer_id)?)
            }
            _ => {
                return Err(StorageError::BackendUnavailable(
                    descriptor.backend.kind().into(),
                ))
            }
        };
        Ok(Self {
            descriptor,
            descriptor_source,
            backend,
        })
    }

    pub fn descriptor(&self) -> &StorageDescriptor {
        &self.descriptor
    }

    pub fn descriptor_source(&self) -> DescriptorSource {
        self.descriptor_source
    }

    /// Temporary, explicit task-14 migration bridge. Not part of ProjectStorage.
    /// Every legacy caller still resolves/validates the project descriptor first.
    pub(crate) fn into_legacy_sqlite(self) -> rusqlite::Connection {
        match self.backend {
            Backend::Sqlite(backend) => backend.into_connection(),
        }
    }
}

impl ProjectStorage for ProjectStore {
    fn rules_snapshot(&mut self) -> StorageResult<RuleSnapshot> {
        match &mut self.backend {
            Backend::Sqlite(backend) => backend.rules_snapshot(),
        }
    }

    fn change_cursor(&mut self) -> StorageResult<ChangeCursor> {
        match &mut self.backend {
            Backend::Sqlite(backend) => backend.change_cursor(),
        }
    }

    fn mutate_rules(&mut self, mutation: RuleMutation) -> StorageResult<RuleMutationResult> {
        let prepared = contract::prepare(mutation)?;
        match &mut self.backend {
            Backend::Sqlite(backend) => backend.mutate_rules(prepared),
        }
    }
}

#[cfg(test)]
mod conformance;
#[cfg(test)]
mod tests;
