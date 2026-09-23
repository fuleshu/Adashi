use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct QaJob {
    pub id: i64,
    pub version: i64,
    pub number: i64,
    pub name: String,
    pub description: String,
    pub command: String,
    pub working_directory: String,
    pub shell: String,
    pub timeout_seconds: i64,
    pub enabled: bool,
    pub created_by: String,
    pub created_at: String,
    pub updated_at: String,
    pub derived_state: String,
    pub design_specification_links: Vec<QaJobDesignLink>,
    pub task_links: Vec<QaJobTaskLink>,
    pub tags: Vec<String>,
    pub latest_run: Option<QaJobRun>,
    pub run_history: Vec<QaJobRun>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct QaJobDesignLink {
    pub id: i64,
    pub qa_job_id: i64,
    pub sort_order: i64,
    pub target_type: String,
    pub design_external_id: String,
    pub title: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct QaJobTaskLink {
    pub id: i64,
    pub qa_job_id: i64,
    pub task_id: i64,
    pub sort_order: i64,
    pub title: String,
    pub number: i64,
    pub state: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct QaJobRun {
    pub id: i64,
    pub qa_run_id: i64,
    pub qa_job_id: i64,
    pub command_snapshot: String,
    pub status: String,
    pub exit_code: Option<i64>,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub duration_ms: Option<i64>,
    pub output: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct QaRun {
    pub id: i64,
    pub trigger_source: String,
    pub query_snapshot: String,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub summary: String,
    pub job_runs: Vec<QaJobRun>,
}

/// Bounded run facts embedded in a job listing: status and timing only.
/// Console output and command snapshots require get_job or get_run.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct QaJobRunSummary {
    pub id: i64,
    pub qa_run_id: i64,
    pub status: String,
    pub exit_code: Option<i64>,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub duration_ms: Option<i64>,
}

/// Bounded job projection for listings: metadata plus latest-run status and timing.
/// Deliberately excludes command, commandSnapshot, output and runHistory.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct QaJobSummary {
    pub id: i64,
    pub version: i64,
    pub number: i64,
    pub name: String,
    pub enabled: bool,
    pub derived_state: String,
    pub tags: Vec<String>,
    pub latest_run: Option<QaJobRunSummary>,
}

/// Per-job outcome inside a bounded run listing.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct QaRunJobStatus {
    pub qa_job_id: i64,
    pub status: String,
    pub exit_code: Option<i64>,
    pub duration_ms: Option<i64>,
}

/// Bounded run projection for listings: run metadata plus per-job outcomes.
/// Console output requires get_run.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct QaRunSummary {
    pub id: i64,
    pub trigger_source: String,
    pub query_snapshot: String,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub summary: String,
    pub jobs: Vec<QaRunJobStatus>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct QaJobQuery {
    pub job_ids: Option<Vec<i64>>,
    pub states: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
    pub task_ids: Option<Vec<i64>>,
    pub design_external_ids: Option<Vec<String>>,
    pub enabled: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct QaDesignLinkInput {
    pub target_type: Option<String>,
    pub design_external_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct NewQaJob {
    pub name: String,
    pub description: Option<String>,
    pub command: String,
    pub working_directory: Option<String>,
    pub shell: Option<String>,
    pub timeout_seconds: Option<i64>,
    pub enabled: Option<bool>,
    pub created_by: Option<String>,
    pub design_specification_links: Option<Vec<QaDesignLinkInput>>,
    pub task_ids: Option<Vec<i64>>,
    pub tags: Option<Vec<String>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct UpdateQaJob {
    pub qa_job_id: i64,
    pub name: Option<String>,
    pub description: Option<String>,
    pub command: Option<String>,
    pub working_directory: Option<String>,
    pub shell: Option<String>,
    pub timeout_seconds: Option<i64>,
    pub enabled: Option<bool>,
    pub design_specification_links: Option<Vec<QaDesignLinkInput>>,
    pub task_ids: Option<Vec<i64>>,
    pub tags: Option<Vec<String>>,
}
