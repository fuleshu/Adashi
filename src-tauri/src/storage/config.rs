use std::{fs, io::ErrorKind};

use serde::{Deserialize, Serialize};

use super::{StorageError, StorageResult};
use crate::settings::{project_data_dir, ProjectSettings};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StorageDescriptor {
    pub schema_version: u32,
    pub backend: BackendSelection,
    /// Changes only on an explicit, verified backend activation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum BackendSelection {
    Sqlite {},
    Text {},
    ServerSql {
        #[serde(rename = "connectionProfile")]
        connection_profile: String,
        namespace: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DescriptorSource {
    LegacyDefault,
    ProjectFile,
}

impl Default for StorageDescriptor {
    fn default() -> Self {
        Self {
            schema_version: 1,
            backend: BackendSelection::Sqlite {},
            generation: None,
        }
    }
}

impl BackendSelection {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Sqlite {} => "sqlite",
            Self::Text {} => "text",
            Self::ServerSql { .. } => "serverSql",
        }
    }
}

impl StorageDescriptor {
    fn validate(&self) -> StorageResult<()> {
        if self
            .generation
            .as_ref()
            .is_some_and(|g| uuid::Uuid::parse_str(g).is_err())
        {
            return Err(StorageError::InvalidConfiguration(
                "Storage generation must be a UUID".into(),
            ));
        }
        if self.schema_version != 1 {
            return Err(StorageError::InvalidConfiguration(format!(
                "Unsupported storage descriptor schemaVersion {}; this build supports 1",
                self.schema_version
            )));
        }
        if let BackendSelection::ServerSql {
            connection_profile,
            namespace,
        } = &self.backend
        {
            // These are portable lookup keys, never URLs, SQL or credentials.
            for (field, value) in [
                ("connectionProfile", connection_profile),
                ("namespace", namespace),
            ] {
                if value.is_empty()
                    || !value
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
                {
                    return Err(StorageError::InvalidConfiguration(format!(
                        "{field} must be a nonempty key containing only letters, digits, '.', '_' or '-'"
                    )));
                }
            }
        }
        Ok(())
    }

    pub(super) fn require_available(&self) -> StorageResult<()> {
        match self.backend {
            BackendSelection::Sqlite {} | BackendSelection::Text {} => Ok(()),
            _ => Err(StorageError::BackendUnavailable(self.backend.kind().into())),
        }
    }
}

/// Pure resolution: do not create .adashi, rewrite settings or inspect a database.
pub(super) fn resolve(
    project: &ProjectSettings,
) -> StorageResult<(StorageDescriptor, DescriptorSource)> {
    let path = project_data_dir(project).join("storage.json");
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            if fs::symlink_metadata(&path).is_ok() {
                return Err(StorageError::InvalidConfiguration(
                    "Project storage descriptor exists but its target cannot be read".into(),
                ));
            }
            return Ok((
                StorageDescriptor::default(),
                DescriptorSource::LegacyDefault,
            ));
        }
        Err(error) => {
            return Err(StorageError::InvalidConfiguration(format!(
                "Cannot read project storage descriptor: {error}"
            )))
        }
    };
    let descriptor: StorageDescriptor = serde_json::from_str(&text).map_err(|error| {
        // Do not echo arbitrary descriptor values (a mistaken credential, for example).
        StorageError::InvalidConfiguration(format!(
            "Invalid project storage descriptor at line {}, column {}; expected the closed version-1 schema",
            error.line(), error.column()
        ))
    })?;
    descriptor.validate()?;
    Ok((descriptor, DescriptorSource::ProjectFile))
}
