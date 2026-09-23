use crate::{health::DesignHealthResult, ChangeCursor, StorageResult};
use serde::{Deserialize, Serialize};

/// Derived data belongs to the local checkout, outside canonical project data.
/// A cache miss is normal and does not initialize or mutate the shared store.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthCacheKey {
    pub project_id: String,
    pub computer_id: String,
    pub cursor: ChangeCursor,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewCacheKey {
    pub project_id: String,
    pub mockup_id: String,
    pub variant: String,
    /// Hash the actual SVG, dimensions and renderer version, not just revision.
    pub source_hash: String,
}
/// Separate optional infrastructure, never a prerequisite for an adapter.
/// Explicit scan/render actions may update it; ordinary reads remain read-only.
pub trait LocalDerivedCache {
    fn health(&self, key: &HealthCacheKey) -> StorageResult<Option<DesignHealthResult>>;
    fn record_health(
        &mut self,
        key: &HealthCacheKey,
        result: &DesignHealthResult,
    ) -> StorageResult<()>;
    fn preview(&self, key: &PreviewCacheKey) -> StorageResult<Option<Vec<u8>>>;
    fn record_preview(&mut self, key: &PreviewCacheKey, png: &[u8]) -> StorageResult<()>;
}
