use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use crate::concurrency::ResourceConflict;
use crate::rules::{NewRule, Rule};

pub use adashi_storage_api::{ChangeCursor, ProjectIdentity, StorageError, StorageResult};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleSnapshot {
    pub project: ProjectIdentity,
    pub cursor: ChangeCursor,
    pub rules: Vec<Rule>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RuleChange {
    Create {
        rule: NewRule,
    },
    Update {
        id: i64,
        expected_version: i64,
        rule: NewRule,
    },
    Delete {
        id: i64,
        expected_version: i64,
    },
}

impl RuleChange {
    pub(super) fn expectation(&self) -> Option<(i64, i64)> {
        match self {
            Self::Create { .. } => None,
            Self::Update {
                id,
                expected_version,
                ..
            }
            | Self::Delete {
                id,
                expected_version,
            } => Some((*id, *expected_version)),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuleMutation {
    pub operation_id: String,
    pub changes: Vec<RuleChange>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RuleOutcome {
    Saved { rule: Rule },
    Deleted { id: i64, version: i64 },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleMutationResult {
    pub cursor: ChangeCursor,
    pub outcomes: Vec<RuleOutcome>,
}

/// Backend-neutral foundation. Domain operations are added here as task 14 extracts them.
/// All methods finish their snapshot/transaction before returning; dropping the
/// project handle releases its backend resources. No driver object escapes here.
pub trait RuleStorage {
    fn rules_snapshot(&mut self) -> StorageResult<RuleSnapshot>;
    fn change_cursor(&mut self) -> StorageResult<ChangeCursor>;
    fn mutate_rules(&mut self, mutation: RuleMutation) -> StorageResult<RuleMutationResult>;
}

pub(super) struct PreparedRuleMutation {
    pub mutation: RuleMutation,
    pub fingerprint: String,
}

/// Shared input policy, invoked before backend dispatch. Backends receive only
/// validated requests and check the guards again inside their atomic unit.
pub(super) fn prepare(mut mutation: RuleMutation) -> StorageResult<PreparedRuleMutation> {
    mutation.operation_id = mutation.operation_id.trim().to_string();
    if mutation.operation_id.is_empty() || mutation.changes.is_empty() {
        return Err(StorageError::Validation(
            "operationId and at least one change are required".into(),
        ));
    }
    let mut targets = HashSet::new();
    for change in &mut mutation.changes {
        if let Some((id, expected_version)) = change.expectation() {
            if id <= 0 || expected_version <= 0 {
                return Err(StorageError::Validation(
                    "Rule id and expectedVersion must be positive".into(),
                ));
            }
            if !targets.insert(id) {
                return Err(StorageError::Validation(format!(
                    "Rule {id} occurs more than once in the change batch"
                )));
            }
        }
        if let RuleChange::Create { rule } | RuleChange::Update { rule, .. } = change {
            crate::rules::validate_rule(&rule.name, &rule.intend, &rule.hook)
                .map_err(StorageError::Validation)?;
            rule.name = rule.name.trim().to_string();
        }
    }
    let bytes = serde_json::to_vec(&mutation).map_err(StorageError::backend)?;
    let fingerprint = format!("{:x}", Sha256::digest(bytes));
    Ok(PreparedRuleMutation {
        mutation,
        fingerprint,
    })
}

pub(super) fn check_expectations(
    expectations: impl Iterator<Item = (i64, i64, i64)>,
) -> StorageResult<()> {
    let conflicts = expectations
        .filter(|(_, expected, current)| expected != current)
        .map(|(id, expected_version, current_version)| ResourceConflict {
            resource_kind: "rule".into(),
            resource_id: id.to_string(),
            expected_version,
            current_version,
        })
        .collect::<Vec<_>>();
    if conflicts.is_empty() {
        Ok(())
    } else {
        Err(StorageError::Conflict(conflicts))
    }
}

pub(super) fn same_rule(current: &Rule, replacement: &NewRule) -> bool {
    current.name == replacement.name
        && current.enabled == replacement.enabled
        && current.intend == replacement.intend
        && current.hook == replacement.hook
        && current.prompt == replacement.prompt
}
