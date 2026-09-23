use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct FixedHookPrompt {
    pub version: i64,
    pub key: String,
    pub title: String,
    pub intend: String,
    pub hook: String,
    pub prompt: String,
    pub updated_at: String,
}
