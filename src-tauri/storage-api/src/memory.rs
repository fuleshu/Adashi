use serde::{Deserialize, Serialize};

pub const MAX_MEMORY_CHARS: usize = 12_000;
pub const MAX_SUMMARY_CHARS: usize = 4_000;
pub const MAX_NOTE_CHARS: usize = 1_000;
pub const MAX_NOTES: usize = 20;

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryLimits {
    pub total_chars: usize,
    pub summary_chars: usize,
    pub note_chars: usize,
    pub notes: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryNote {
    pub note_id: String,
    pub operation_id: String,
    pub run_id: String,
    pub task_id: Option<i64>,
    pub body: String,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub superseded_by_version: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectMemory {
    pub limits: MemoryLimits,
    pub rule: String,
    pub memory: String,
    pub memory_version: i64,
    pub protocol_version: i64,
    pub notes: Vec<MemoryNote>,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppendMemoryNote {
    pub note_id: String,
    pub operation_id: String,
    pub run_id: String,
    pub task_id: Option<i64>,
    pub body: String,
}
