//! Complete portable interchange for adapter-to-adapter migration. Its record
//! vocabulary is the versioned text contract, not executable SQL or a live cache.
use super::{StorageError, StorageResult};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProjectImage {
    pub schema_version: u32,
    pub files: BTreeMap<String, Vec<u8>>,
}

pub(crate) trait SourceSnapshot {
    fn image(&self) -> &ProjectImage;
    /// Check outside the pinned source view immediately before activation.
    fn ensure_unchanged(&self) -> StorageResult<()>;
    /// Store a usable, consistent source backup; all local provenance is retained.
    fn backup(&self, project_folder: &Path) -> StorageResult<()>;
}

pub(crate) trait MigrationAdapter {
    // Every adapter, including future server SQL, transfers the closed canonical
    // vocabulary: Markdown documents/ordered links, bindings, task/QA references
    // and retained deletion identities. Generated files/manifests are excluded.
    fn capture(
        &self,
        project: &super::ProjectRegistration,
        freeze_writes: bool,
    ) -> StorageResult<Box<dyn SourceSnapshot>>;
    fn stage(
        &self,
        image: &ProjectImage,
        project_folder: &Path,
        generation: &str,
    ) -> StorageResult<()>;
    fn inspect_staged(&self, project_folder: &Path) -> StorageResult<ProjectImage>;
}

impl ProjectImage {
    pub fn validate(&self) -> StorageResult<()> {
        if self.schema_version != 1 {
            return Err(StorageError::Validation(
                "Unsupported migration image version".into(),
            ));
        }
        super::text::transfer::validate_image(self)
    }
    pub fn fingerprint(&self) -> StorageResult<String> {
        super::text::transfer::fingerprint(self)
    }
    pub fn semantic_fingerprint(&self) -> StorageResult<String> {
        super::text::transfer::semantic_fingerprint(self)
    }
    pub fn counts(&self) -> StorageResult<BTreeMap<String, usize>> {
        super::text::transfer::counts(self)
    }
    pub fn warnings(&self) -> StorageResult<Vec<String>> {
        super::text::transfer::warnings(self)
    }
}
