use serde::{Deserialize, Serialize};

/// The task lifecycle, in order: `todo` is created and unclaimed, `active` is being worked on,
/// `finished` is reported complete and awaiting review, `closed` is reviewed and accepted.
///
/// `finished` and `closed` are deliberately different: an agent reporting its own work complete
/// is not a verdict, so the review step is a separate transition. A reviewer who disagrees sends
/// the task back to `active`, which clears the completion timestamp.
pub const TASK_STATE_NAMES: [&str; 4] = ["todo", "active", "finished", "closed"];

/// The error every unrecognised-state path returns, so the accepted values are stated from one
/// place and a caller never has to guess or retry.
pub fn invalid_task_state_error(value: &str) -> String {
    format!(
        "Invalid task state '{value}'. Expected one of: {}. Tasks start in 'todo'; a review that \
         rejects finished work sets it back to 'active'.",
        TASK_STATE_NAMES.join(", ")
    )
}

/// Whether moving `from` -> `to` drops the completion claim. `finished` and `closed` both mean
/// "the work is complete", so leaving either of them for real work clears `completed_at`.
pub fn clears_completed_at(from: TaskState, to: TaskState) -> bool {
    matches!(from, TaskState::Finished | TaskState::Closed)
        && !matches!(to, TaskState::Finished | TaskState::Closed)
}

#[derive(
    Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum TaskState {
    Todo,
    Active,
    Finished,
    Closed,
}

impl TaskState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::Active => "active",
            Self::Finished => "finished",
            Self::Closed => "closed",
        }
    }

    /// Parses a caller-supplied state name, rejecting anything unrecognised with the full list.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "todo" => Ok(Self::Todo),
            "active" => Ok(Self::Active),
            "finished" => Ok(Self::Finished),
            "closed" => Ok(Self::Closed),
            other => Err(invalid_task_state_error(other)),
        }
    }
}

/// Whether `from` may become `to`. A state may always be re-set to itself, so a no-op update
/// stays idempotent. `closed` requires `finished`: a reviewer closes work they have seen, and the
/// only way to un-close it is to send it back to `active`.
pub fn task_transition_allowed(from: TaskState, to: TaskState) -> bool {
    if from == to {
        return true;
    }
    match to {
        TaskState::Todo => false,
        TaskState::Active => true,
        TaskState::Finished => from == TaskState::Active,
        TaskState::Closed => from == TaskState::Finished,
    }
}

/// Every state, for a consumer that applies its own visibility rules.
pub const ALL_TASK_STATES: [TaskState; 4] = [
    TaskState::Todo,
    TaskState::Active,
    TaskState::Finished,
    TaskState::Closed,
];

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskSummary {
    pub id: i64,
    pub number: i64,
    pub title: String,
    pub title_truncated: bool,
    pub state: TaskState,
    pub version: i64,
}

/// States a default listing shows: everything except closed work.
pub const DEFAULT_VISIBLE_STATES: [TaskState; 3] =
    [TaskState::Todo, TaskState::Active, TaskState::Finished];

/// The state filter to apply when a caller omits one: open work only, with closed tasks excluded
/// so accepted history cannot be mistaken for current work.
///
/// This is the *default view*, not "every state". A consumer that owns its own visibility control
/// — the dashboard, which offers a per-state checkbox — must ask for `ALL_TASK_STATES` instead,
/// or the control it shows can never reveal anything.
pub fn default_state_filter(states: Option<&[TaskState]>) -> Vec<TaskState> {
    match states {
        Some(states) => states.to_vec(),
        None => DEFAULT_VISIBLE_STATES.to_vec(),
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct Task {
    pub id: i64,
    pub version: i64,
    pub number: i64,
    pub title: String,
    pub description: String,
    pub state: String,
    pub design_specification_links: Vec<TaskDesignSpecificationLink>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
    pub confirmed_at: Option<String>,
    pub completion_memo: String,
    pub created_files: Vec<String>,
    pub changed_files: Vec<String>,
    pub confirmation_commit_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct TaskDesignSpecificationLink {
    pub id: i64,
    pub task_id: i64,
    pub sort_order: i64,
    pub target_type: String,
    pub design_external_id: String,
    pub title: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct TaskDesignSpecificationLinkInput {
    pub target_type: Option<String>,
    pub design_external_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct NewTask {
    pub title: String,
    pub description: Option<String>,
    pub design_specification_links: Option<Vec<TaskDesignSpecificationLinkInput>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct UpdateTask {
    pub task_id: i64,
    pub title: Option<String>,
    pub description: Option<String>,
    pub state: Option<String>,
    pub design_specification_links: Option<Vec<TaskDesignSpecificationLinkInput>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(schemars::JsonSchema)]
pub struct FinishTask {
    pub task_id: i64,
    pub completion_memo: String,
    pub created_files: Vec<String>,
    pub changed_files: Vec<String>,
}
