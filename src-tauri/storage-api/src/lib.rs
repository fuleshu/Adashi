//! Complete, driver-independent project storage API.
//!
//! Adapter authors implement [`StorageBackend`]; callers use [`ProjectStorage`].
//! [`StorageClient`] supplies validation and delegates to a complete backend. No
//! database connection, SQL row, desktop runtime or MCP transport belongs here.
//! See docs/storage-api-migration.md for the operation-to-interface inventory.

mod api;
mod cache;
mod changes;
pub mod coordination;
pub mod design;
pub mod documents;
mod error;
pub mod fixed_hooks;
pub mod health;
pub mod memory;
pub mod mockups;
pub mod qa;
pub mod rules;
pub mod search;
pub mod tasks;
mod validation;

pub use api::*;
pub use cache::*;
pub use changes::*;
pub use error::*;
pub use validation::*;
