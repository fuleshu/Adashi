//! Backend-neutral models and shared domain helpers.
pub use adashi_storage_api::fixed_hooks::*;
pub const DESIGN_AUTHORING_HOOK_KEY: &str = "design.run.start.authoring";
pub const IMPLEMENTATION_GUIDANCE_HOOK_KEY: &str = "implementation.run.start.design-guide";
pub const DEFAULT_DESIGN_AUTHORING_PROMPT: &str = "";
pub const DEFAULT_IMPLEMENTATION_GUIDANCE_PROMPT: &str = "";
#[cfg(test)]
pub(crate) use crate::storage::sqlite::fixed_hooks::*;
