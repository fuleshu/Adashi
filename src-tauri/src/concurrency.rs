//! Backend-neutral models and shared domain helpers.
#[cfg(test)]
pub(crate) use crate::storage::sqlite::concurrency::*;
pub use adashi_storage_api::coordination::*;
