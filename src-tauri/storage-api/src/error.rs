use crate::{coordination::ResourceConflict, DocumentConflict, MissingReference};
use serde::{Deserialize, Serialize};
use std::fmt;

pub type StorageResult<T> = Result<T, StorageError>;

#[derive(Debug)]
pub enum StorageError {
    InvalidConfiguration(String),
    BackendUnavailable(String),
    IncompatibleSchema { found: i64, supported: i64 },
    Validation(String),
    Conflict(Vec<ResourceConflict>),
    OperationReused,
    NotFound { kind: &'static str, id: i64 },
    ResourceNotFound(crate::ResourceKey),
    Unavailable(String),
    TimedOut,
    AccessDenied,
    Backend(String),
    Documents(Vec<DocumentConflict>),
    DesignRejected(crate::design::DesignSaveResult),
    References(Vec<MissingReference>),
    Closed,
    CommitUncertain { operation_id: String },
}

impl StorageError {
    pub fn backend(error: impl fmt::Display) -> Self {
        Self::Backend(error.to_string())
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidConfiguration(_) => "storage.invalid_configuration",
            Self::BackendUnavailable(_) => "storage.backend_unavailable",
            Self::IncompatibleSchema { .. } => "storage.incompatible_schema",
            Self::Validation(_) => "storage.validation",
            Self::Conflict(_) => "resource.conflict",
            Self::OperationReused => "storage.operation_reused",
            Self::NotFound { .. } => "storage.not_found",
            Self::ResourceNotFound(_) => "storage.not_found",
            Self::Unavailable(_) => "storage.unavailable",
            Self::TimedOut => "storage.timed_out",
            Self::AccessDenied => "storage.access_denied",
            Self::Backend(_) => "storage.failure",
            Self::Documents(_) => "storage.document_conflict",
            Self::DesignRejected(_) => "storage.design_rejected",
            Self::References(_) => "storage.reference_conflict",
            Self::Closed => "storage.closed",
            Self::CommitUncertain { .. } => "storage.commit_uncertain",
        }
    }
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.code())?;
        match self {
            Self::InvalidConfiguration(message) | Self::Validation(message) => write!(f, "{message}"),
            Self::Backend(_) => write!(f, "The project data operation failed"),
            Self::BackendUnavailable(kind) => write!(f, "The '{kind}' backend is not available in this build; project storage was not opened"),
            Self::IncompatibleSchema { found, supported } => write!(f, "Data schema {found} is newer than supported schema {supported}"),
            Self::Conflict(conflicts) => write!(f, "{}", serde_json::to_string(conflicts).map_err(|_| fmt::Error)?),
            Self::OperationReused => write!(f, "operationId was already used with different arguments or an older receipt format. Use a new operationId for a changed request; reuse it only for an identical retry."),
            Self::NotFound { kind, id } => write!(f, "Unknown {kind} id: {id}"),
            Self::ResourceNotFound(key) => write!(f, "Unknown {}: {}", key.kind, key.id),
            Self::Unavailable(message) => write!(f, "{message}"),
            Self::TimedOut => write!(f, "Storage operation timed out before commit"),
            Self::AccessDenied => write!(f, "Project storage access denied"),
            Self::Documents(items) => write!(f, "{}", serde_json::to_string(items).map_err(|_| fmt::Error)?),
            Self::DesignRejected(result) => write!(f, "{}", serde_json::to_string(result).map_err(|_| fmt::Error)?),
            Self::References(items) => write!(f, "{}", serde_json::to_string(items).map_err(|_| fmt::Error)?),
            Self::Closed => write!(f, "Project store is closed"),
            Self::CommitUncertain { operation_id } => write!(f, "Commit outcome unknown; retrieve receipt for {operation_id} before retrying"),
        }
    }
}

impl std::error::Error for StorageError {}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct ChangeCursor(pub(crate) String);

impl ChangeCursor {
    pub fn in_scope(self, scope: Option<&str>) -> Self {
        match scope {
            Some(scope) => Self(format!("selected:{scope}:{}", self.0)),
            None => self,
        }
    }
    /// Adapters supply an opaque token; clients compare or persist it without
    /// interpreting it as a resource version or mutation precondition.
    pub fn from_token(token: impl Into<String>) -> Self {
        Self(token.into())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectIdentity {
    pub id: String,
    pub name: String,
}
