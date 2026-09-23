//! Backend-neutral models and shared domain helpers.
pub use adashi_storage_api::qa::*;

pub fn state_rank(state: &str) -> i32 {
    match state {
        "running" => 0,
        "red" => 1,
        "needs-rerun" => 2,
        "green" => 3,
        _ => 4,
    }
}

pub(crate) fn platform_default_shell() -> &'static str {
    if cfg!(windows) {
        "powershell"
    } else {
        "bash"
    }
}
#[cfg(test)]
pub(crate) use crate::storage::sqlite::qa::*;
