use serde::{Deserialize, Serialize};

/// Accepted QA job kinds. A kind states what class of check the job performs and
/// therefore how long it may run and whether it may produce a deliverable.
pub const QA_JOB_KINDS: &[&str] = &[
    "lint",
    "unit",
    "integration",
    "e2e",
    "smoke",
    "build",
    "release",
];

/// Per-kind wall-clock ceiling in seconds. A job that needs longer than its kind
/// allows is not one check; it must be split or run outside Adashi.
pub fn qa_kind_timeout_ceiling(kind: &str) -> i64 {
    match kind {
        "lint" => 120,
        "smoke" => 300,
        "unit" => 600,
        "integration" => 900,
        "e2e" => 1_800,
        "build" => 1_800,
        "release" => 3_600,
        _ => 600,
    }
}

/// Absolute ceiling for any QA job, regardless of kind.
pub const QA_JOB_MAX_TIMEOUT_SECONDS: i64 = 3_600;
pub const QA_JOB_DEFAULT_TIMEOUT_SECONDS: i64 = 120;
pub const QA_JOB_SCOPE_MAX_LENGTH: usize = 240;
/// Hard cap on jobs reserved by one run, broad or explicit.
pub const QA_RUN_MAX_JOBS: usize = 12;
pub const QA_RUN_DEFAULT_BUDGET_SECONDS: i64 = 900;
pub const QA_RUN_MAX_BUDGET_SECONDS: i64 = 3_600;
/// Lease slack added to a job timeout so a crashed worker's run can be reclaimed.
pub const QA_JOB_LEASE_SLACK_SECONDS: i64 = 60;

/// Operations that produce a distributable artifact rather than verifying
/// behavior. Only a `release` job may package; a test job that assembles an
/// installer is doing work unrelated to the behavior it claims to verify.
const PACKAGING_TOKENS: &[&str] = &[
    "assemble-release-bundle",
    "cargo bundle",
    "electron-builder",
    "makensis",
    "iscc",
    "candle.exe",
    "light.exe",
    "pkgbuild",
    "productbuild",
    "dpkg-deb",
    "appimagetool",
    "wix build",
    ".wxs",
    ".msi",
    ".dmg",
    ".appimage",
];

/// Normalizes a requested kind, or returns None when it is not a known kind.
pub fn qa_normalize_kind(kind: &str) -> Option<&'static str> {
    let trimmed = kind.trim();
    QA_JOB_KINDS
        .iter()
        .copied()
        .find(|candidate| candidate.eq_ignore_ascii_case(trimmed))
}

/// Returns the packaging operation a command performs, if any.
pub fn qa_packaging_operation(command: &str) -> Option<String> {
    let normalized = command.to_ascii_lowercase();
    let normalized = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.contains("tauri build") && !normalized.contains("--no-bundle") {
        return Some("tauri build (bundling)".to_string());
    }
    PACKAGING_TOKENS
        .iter()
        .find(|token| normalized.contains(**token))
        .map(|token| (*token).to_string())
}

/// Validates the declared kind and scope. Returns the normalized kind.
pub fn validate_qa_identity(kind: &str, scope: &str) -> Result<&'static str, String> {
    let Some(normalized) = qa_normalize_kind(kind) else {
        let label = if kind.trim().is_empty() {
            "QA kind is required".to_string()
        } else {
            format!("QA kind '{}' is not supported", kind.trim())
        };
        return Err(format!(
            "{label}. Expected one of: {}",
            QA_JOB_KINDS.join(", ")
        ));
    };
    let scope = scope.trim();
    if scope.is_empty() {
        return Err(
            "QA scope is required: state the one behavior this job verifies".to_string(),
        );
    }
    if scope.chars().count() > QA_JOB_SCOPE_MAX_LENGTH {
        return Err(format!(
            "QA scope must be at most {QA_JOB_SCOPE_MAX_LENGTH} characters"
        ));
    }
    Ok(normalized)
}

/// Validates a timeout against the kind ceiling. Returns the accepted timeout.
pub fn validate_qa_timeout(kind: &str, timeout_seconds: i64) -> Result<i64, String> {
    if timeout_seconds <= 0 {
        return Err("QA timeout must be positive".to_string());
    }
    let ceiling = qa_kind_timeout_ceiling(kind);
    if timeout_seconds > ceiling {
        return Err(format!(
            "QA kind '{kind}' allows at most {ceiling} seconds, got {timeout_seconds}. \
             Split the job or run the heavy step outside Adashi instead of raising the timeout."
        ));
    }
    Ok(timeout_seconds)
}

/// Rejects packaging work outside `release`.
pub fn validate_qa_packaging(kind: &str, command: &str) -> Result<(), String> {
    if kind == "release" {
        return Ok(());
    }
    if let Some(operation) = qa_packaging_operation(command) {
        return Err(format!(
            "QA kind '{kind}' may not package or bundle (found '{operation}'). \
             A QA job verifies behavior; declare kind=release only when the deliverable is what you verify."
        ));
    }
    Ok(())
}

/// A QA job must name the design artefact or task it verifies.
pub fn validate_qa_verify_target(design_links: usize, task_ids: usize) -> Result<(), String> {
    if design_links == 0 && task_ids == 0 {
        return Err(
            "QA job must link at least one design specification or task that it verifies"
                .to_string(),
        );
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct QaJob {
    pub id: i64,
    pub version: i64,
    pub number: i64,
    pub name: String,
    pub description: String,
    /// Declared check class; see `QA_JOB_KINDS`.
    pub kind: String,
    /// The one behavior this job verifies.
    pub scope: String,
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
    #[schemars(with = "crate::markdown::DesignTargetKind")]
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

impl QaJobQuery {
    /// True when the caller named what to run. An all-default query is a broad
    /// run over every enabled job and must be requested explicitly.
    pub fn requests_explicit_selection(&self) -> bool {
        fn named(values: &Option<Vec<String>>) -> bool {
            values
                .as_ref()
                .is_some_and(|items| items.iter().any(|item| !item.trim().is_empty()))
        }
        fn named_ids(values: &Option<Vec<i64>>) -> bool {
            values.as_ref().is_some_and(|items| !items.is_empty())
        }
        named_ids(&self.job_ids)
            || named(&self.states)
            || named(&self.tags)
            || named_ids(&self.task_ids)
            || named(&self.design_external_ids)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct QaDesignLinkInput {
    #[schemars(with = "Option<crate::markdown::DesignTargetKind>")]
    pub target_type: Option<String>,
    pub design_external_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct NewQaJob {
    pub name: String,
    pub description: Option<String>,
    pub kind: Option<String>,
    pub scope: Option<String>,
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
    pub kind: Option<String>,
    pub scope: Option<String>,
    pub command: Option<String>,
    pub working_directory: Option<String>,
    pub shell: Option<String>,
    pub timeout_seconds: Option<i64>,
    pub enabled: Option<bool>,
    pub design_specification_links: Option<Vec<QaDesignLinkInput>>,
    pub task_ids: Option<Vec<i64>>,
    pub tags: Option<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_ceilings_are_ordered_and_bounded() {
        assert_eq!(qa_kind_timeout_ceiling("lint"), 120);
        assert_eq!(qa_kind_timeout_ceiling("release"), QA_JOB_MAX_TIMEOUT_SECONDS);
        for kind in QA_JOB_KINDS {
            let ceiling = qa_kind_timeout_ceiling(kind);
            assert!(ceiling > 0 && ceiling <= QA_JOB_MAX_TIMEOUT_SECONDS, "{kind}");
        }
    }

    #[test]
    fn packaging_detection_is_specific_and_respects_no_bundle() {
        assert_eq!(
            qa_packaging_operation("tauri build").as_deref(),
            Some("tauri build (bundling)")
        );
        assert!(qa_packaging_operation("tauri build --no-bundle").is_none());
        assert!(qa_packaging_operation("scripts/assemble-release-bundle.sh out").is_some());
        assert!(qa_packaging_operation("cargo test --workspace").is_none());
        assert!(qa_packaging_operation("vite build && tsc --noEmit").is_none());
    }

    #[test]
    fn empty_query_is_not_an_explicit_selection() {
        assert!(!QaJobQuery::default().requests_explicit_selection());
        let mut query = QaJobQuery::default();
        query.job_ids = Some(vec![]);
        assert!(!query.requests_explicit_selection());
        query.job_ids = Some(vec![4]);
        assert!(query.requests_explicit_selection());
        let tags = QaJobQuery {
            tags: Some(vec!["  ".into()]),
            ..Default::default()
        };
        assert!(!tags.requests_explicit_selection());
    }

    #[test]
    fn timeout_validation_names_the_ceiling() {
        let error = validate_qa_timeout("unit", 900).unwrap_err();
        assert!(error.contains("at most 600"), "{error}");
        assert_eq!(validate_qa_timeout("unit", 600).unwrap(), 600);
    }
}
