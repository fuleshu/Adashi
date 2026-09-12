#![cfg_attr(not(feature = "desktop"), allow(dead_code))]

mod concurrency;
mod design;
mod fixed_hooks;
mod mcp;
mod memory;
mod mockups;
mod project;
mod projection;
mod prompt_hygiene;
mod qa;
mod rules;
mod schema;
mod seed;
mod settings;
mod state;
mod tasks;

#[cfg(test)]
pub(crate) use project::open_project_database;
#[cfg(feature = "desktop")]
#[path = "desktop_module.rs"]
mod desktop;
#[cfg(feature = "desktop")]
pub use desktop::run;

pub fn run_mcp() -> Result<(), Box<dyn std::error::Error>> {
    mcp::run_stdio_server()
}
