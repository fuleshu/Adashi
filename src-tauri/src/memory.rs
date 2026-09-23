//! Backend-neutral models and shared domain helpers.
pub use adashi_storage_api::memory::*;
pub const DEFAULT_MEMORY_RULE: &str = "";
#[cfg(test)]
pub(crate) use crate::storage::sqlite::memory::*;
