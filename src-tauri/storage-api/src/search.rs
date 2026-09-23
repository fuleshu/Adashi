use crate::tasks;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
const ALL_DOMAINS: [GrepDomain; 3] = [GrepDomain::Design, GrepDomain::Tasks, GrepDomain::Memory];

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum GrepDomain {
    Design,
    Tasks,
    Memory,
}

impl GrepDomain {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Design => "design",
            Self::Tasks => "tasks",
            Self::Memory => "memory",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GrepScope {
    /// Every project-content domain.
    All,
    Design,
    Tasks,
    Memory,
}

impl GrepScope {
    pub fn domains(self) -> &'static [GrepDomain] {
        match self {
            Self::All => &ALL_DOMAINS,
            Self::Design => &[GrepDomain::Design],
            Self::Tasks => &[GrepDomain::Tasks],
            Self::Memory => &[GrepDomain::Memory],
        }
    }

    pub fn from_word(word: &str) -> Option<Self> {
        match word.to_ascii_lowercase().as_str() {
            "all" => Some(Self::All),
            "design" => Some(Self::Design),
            "tasks" => Some(Self::Tasks),
            "memory" => Some(Self::Memory),
            _ => None,
        }
    }
}

/// Task state filter for a grep. The vocabulary is the task lifecycle's own, so a search and a
/// task listing can never disagree about what a state is called.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GrepTaskState {
    Todo,
    Active,
    Finished,
    Closed,
}

impl GrepTaskState {
    /// Task states are closed vocabulary, so an unrecognised state is reported as a typo with the
    /// accepted values rather than being silently searched as text.
    pub fn from_word(word: &str) -> Result<Self, String> {
        tasks::TaskState::parse(word).map(|state| match state {
            tasks::TaskState::Todo => Self::Todo,
            tasks::TaskState::Active => Self::Active,
            tasks::TaskState::Finished => Self::Finished,
            tasks::TaskState::Closed => Self::Closed,
        })
    }

    pub fn to_task_state(self) -> tasks::TaskState {
        match self {
            Self::Todo => tasks::TaskState::Todo,
            Self::Active => tasks::TaskState::Active,
            Self::Finished => tasks::TaskState::Finished,
            Self::Closed => tasks::TaskState::Closed,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GrepParams {
    /// Configured project name (case-insensitive) or project id.
    pub project_name: String,
    /// What to look for: a bare string, whitespace-separated terms (all must appear),
    /// `"quoted phrases"`, or `key:value` clauses for in/file/type/state/limit. Matching is
    /// always case-insensitive, and an unrecognised `key:value` clause is searched as text.
    /// An empty pattern returns the top-layer overview with counts.
    #[serde(default)]
    pub pattern: Option<String>,
    /// Domain scope: all (default), design, tasks, memory.
    #[serde(default)]
    pub r#in: Option<GrepScope>,
    /// Restrict design hits to elements bound to this file or symbol, and task hits to tasks
    /// whose created or changed file lists contain it.
    #[serde(default)]
    pub file: Option<String>,
    /// Restrict design hits to one C4 element type, for example Container.
    #[serde(default)]
    pub r#type: Option<String>,
    /// Restrict task hits to one state.
    #[serde(default)]
    pub state: Option<GrepTaskState>,
    /// Maximum matches returned (default 50, maximum 200); the response budget can lower it.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Rendered grep output plus the accounting an agent needs in order to narrow deliberately.
#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GrepResult {
    /// The complete grep-shaped output, bounded by the reply budget.
    pub output: String,
    /// True total over every selected domain, before the limit and the budget apply.
    pub total: usize,
    /// Per-domain split of `total`.
    pub counts: GrepCounts,
    /// Matches actually rendered.
    pub shown: usize,
    /// True when the limit or the reply budget left matches out.
    pub truncated: bool,
    /// Bytes of `output`; never larger than the reply budget.
    pub output_bytes: usize,
    /// Whether the pattern contained anything to search for.
    pub pattern_terms: usize,
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GrepCounts {
    pub design: usize,
    pub tasks: usize,
    pub memory: usize,
}

impl GrepCounts {
    pub fn of(counts: &BTreeMap<GrepDomain, usize>) -> Self {
        Self {
            design: counts.get(&GrepDomain::Design).copied().unwrap_or(0),
            tasks: counts.get(&GrepDomain::Tasks).copied().unwrap_or(0),
            memory: counts.get(&GrepDomain::Memory).copied().unwrap_or(0),
        }
    }
}
