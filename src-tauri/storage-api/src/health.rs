use serde::{Deserialize, Serialize};

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum ElementHealth {
    /// Attached to the model, but no binding: its code cannot be located.
    Unmapped,
    /// No parent and no relationship names it: floating in the hierarchy.
    Orphaned,
    /// Attached, with at least one binding that does not resolve.
    Broken,
    /// Attached, with bindings that all resolve.
    Resolved,
}

impl ElementHealth {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unmapped => "unmapped",
            Self::Orphaned => "orphaned",
            Self::Broken => "broken",
            Self::Resolved => "resolved",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "unmapped" => Some(Self::Unmapped),
            "orphaned" => Some(Self::Orphaned),
            "broken" => Some(Self::Broken),
            "resolved" => Some(Self::Resolved),
            _ => None,
        }
    }

    /// Whether the model's claim is currently unsupported by the code. These are the states a task
    /// close has to address or knowingly keep.
    #[allow(dead_code)]
    pub fn needs_attention(self) -> bool {
        !matches!(self, Self::Resolved)
    }

    /// Most actionable first, so a listing reads as a work queue.
    pub fn weight(self) -> u8 {
        match self {
            Self::Broken => 0,
            Self::Orphaned => 1,
            Self::Unmapped => 2,
            Self::Resolved => 3,
        }
    }

    /// The next action, in words, so the state does not need a legend.
    pub fn next_action(self) -> &'static str {
        match self {
            Self::Orphaned => "Attach it to the model: give it a parent or a relationship.",
            Self::Unmapped => "Bind it to the file that implements it.",
            Self::Broken => "Fix the binding: what it names is not there.",
            Self::Resolved => "Nothing to do.",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrokenBinding {
    pub design_external_id: String,
    pub target_type: String,
    pub target: String,
    pub detail: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ElementFinding {
    pub design_external_id: String,
    pub name: String,
    pub element_type: String,
    pub state: ElementHealth,
    /// What to do about it, in one line.
    pub next_action: String,
    /// Files the element's bindings resolve to.
    pub files: Vec<String>,
    /// Bindings that name something that is not there.
    pub broken: Vec<BrokenBinding>,
    /// Human-readable summary of why the element is in this state.
    pub detail: String,
    /// Findings reviewed and knowingly kept, with their reasons.
    pub waivers: Vec<HealthWaiver>,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HealthWaiver {
    pub id: i64,
    pub state: String,
    pub reason: String,
    pub task_id: Option<i64>,
    pub created_by: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HealthCounts {
    pub orphaned: u32,
    pub unmapped: u32,
    pub broken: u32,
    pub resolved: u32,
    /// Individual bindings that do not resolve, which can exceed the number of broken elements.
    pub broken_bindings: u32,
    pub waivers: u32,
    pub elements: u32,
}

impl HealthCounts {
    /// Elements whose claim is not currently supported by the code. The close gate reads this;
    /// until the gate lands, it is exercised by the tests.
    #[allow(dead_code)]
    pub fn needs_attention(&self) -> u32 {
        self.orphaned + self.unmapped + self.broken
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesignHealthResult {
    pub counts: HealthCounts,
    /// Every element, most actionable first, so a client can list or filter by state without a
    /// second call.
    pub elements: Vec<ElementFinding>,
    /// One line stating what the numbers mean, so a reader does not have to infer it.
    pub summary: String,
    /// What these checks do not establish, stated in the result itself.
    pub not_checked: String,
}
