use serde::{Deserialize, Serialize};

pub const INTENDS: &[&str] = &["general", "design", "implementation"];
pub const HOOKS: &[&str] = &["run.start", "task.start", "task.end", "run.end"];

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct Rule {
    pub id: i64,
    pub version: i64,
    pub name: String,
    pub enabled: bool,
    pub intend: String,
    pub hook: String,
    pub prompt: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct InjectionRule {
    pub id: i64,
    pub version: i64,
    pub enabled: bool,
    pub intend: String,
    pub hook: String,
    pub prompt: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct NewRule {
    pub name: String,
    pub enabled: bool,
    pub intend: String,
    pub hook: String,
    pub prompt: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateRule {
    pub id: i64,
    pub name: String,
    pub enabled: bool,
    pub intend: String,
    pub hook: String,
    pub prompt: String,
}

/// Domain validation shared by legacy callers and every storage adapter.
pub fn validate_rule(name: &str, intend: &str, hook: &str) -> Result<(), String> {
    validate_intend(intend)?;
    validate_hook(hook)?;
    if name.trim().is_empty() {
        return Err("Rule name is required".to_string());
    }
    Ok(())
}

pub fn validate_intend(intend: &str) -> Result<(), String> {
    if INTENDS.contains(&intend) {
        Ok(())
    } else {
        Err(format!(
            "Invalid intend '{intend}'. Expected one of: {}",
            INTENDS.join(", ")
        ))
    }
}

pub fn validate_hook(hook: &str) -> Result<(), String> {
    if HOOKS.contains(&hook) {
        Ok(())
    } else {
        Err(format!(
            "Invalid hook '{hook}'. Expected one of: {}",
            HOOKS.join(", ")
        ))
    }
}
