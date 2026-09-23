use super::api::*;
use adashi_storage_api::health::DesignHealthResult;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Disposable data belongs to this installation, outside the Git project.
pub(crate) struct FileDerivedCache {
    root: PathBuf,
}
impl FileDerivedCache {
    pub(crate) fn for_checkout(checkout: &Path) -> Self {
        let key = format!(
            "{:x}",
            Sha256::digest(checkout.to_string_lossy().as_bytes())
        );
        let root = crate::settings::settings_path()
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("cache")
            .join(key);
        Self { root }
    }
    fn path(&self, namespace: &str, key: &impl serde::Serialize) -> StorageResult<PathBuf> {
        let bytes = serde_json::to_vec(key).map_err(StorageError::backend)?;
        Ok(self
            .root
            .join(namespace)
            .join(format!("{:x}", Sha256::digest(bytes))))
    }
    fn read(path: &Path) -> StorageResult<Option<Vec<u8>>> {
        match fs::read(path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(StorageError::backend(e)),
        }
    }
    fn write(path: &Path, bytes: &[u8]) -> StorageResult<()> {
        fs::create_dir_all(path.parent().ok_or_else(|| {
            StorageError::InvalidConfiguration("Cache location needs a parent".into())
        })?)
        .map_err(StorageError::backend)?;
        let mut temp = tempfile::NamedTempFile::new_in(path.parent().unwrap())
            .map_err(StorageError::backend)?;
        use std::io::Write;
        temp.write_all(bytes).map_err(StorageError::backend)?;
        temp.persist(path).map_err(StorageError::backend)?;
        Ok(())
    }
}
impl LocalDerivedCache for FileDerivedCache {
    fn health(&self, key: &HealthCacheKey) -> StorageResult<Option<DesignHealthResult>> {
        Ok(Self::read(&self.path("health", key)?)?
            .and_then(|bytes| serde_json::from_slice(&bytes).ok()))
    }
    fn record_health(
        &mut self,
        key: &HealthCacheKey,
        result: &DesignHealthResult,
    ) -> StorageResult<()> {
        Self::write(
            &self.path("health", key)?,
            &serde_json::to_vec(result).map_err(StorageError::backend)?,
        )
    }
    fn preview(&self, key: &PreviewCacheKey) -> StorageResult<Option<Vec<u8>>> {
        Self::read(&self.path("preview", key)?)
    }
    fn record_preview(&mut self, key: &PreviewCacheKey, png: &[u8]) -> StorageResult<()> {
        Self::write(&self.path("preview", key)?, png)
    }
}
