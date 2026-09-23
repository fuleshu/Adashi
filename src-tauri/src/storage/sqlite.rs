use std::{fs, time::Duration};

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

use super::contract::{self, PreparedRuleMutation};
use super::{
    ChangeCursor, ProjectIdentity, RuleChange, RuleMutationResult, RuleOutcome, RuleSnapshot,
    StorageError, StorageResult,
};
use crate::settings::ProjectSettings;
use crate::{concurrency, fixed_hooks, rules, schema, seed, settings, state};

pub(super) struct SqliteStorage {
    db: Connection,
    project_id: i64,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuleReceipt {
    format: String,
    fingerprint: String,
    result: RuleMutationResult,
}

const RULE_RECEIPT_FORMAT: &str = "adashi.storage.rules.v1";

impl SqliteStorage {
    pub(super) fn open(project: &ProjectSettings, computer_id: &str) -> StorageResult<Self> {
        fs::create_dir_all(settings::project_data_dir(project)).map_err(StorageError::backend)?;
        let mut db = Connection::open(settings::project_database_path(project))
            .map_err(StorageError::backend)?;
        db.busy_timeout(Duration::from_secs(5))
            .map_err(StorageError::backend)?;
        let version = db
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .map_err(StorageError::backend)?;
        if version > schema::SCHEMA_VERSION {
            return Err(StorageError::IncompatibleSchema {
                found: version,
                supported: schema::SCHEMA_VERSION,
            });
        }
        schema::migrate(&mut db).map_err(StorageError::backend)?;
        seed::seed_initial_data(&mut db, project).map_err(StorageError::backend)?;
        fixed_hooks::ensure_fixed_hook_prompts(&db).map_err(StorageError::backend)?;
        // Preserve the current initialization and per-computer mapping contract.
        db.execute_batch(include_str!("../concurrency_schema.sql"))
            .map_err(StorageError::backend)?;
        db.execute(
            "INSERT INTO project_computers(project_id, computer_id, repository_path)
             SELECT id, ?1, ?2 FROM projects ORDER BY id LIMIT 1
             ON CONFLICT(project_id, computer_id) DO UPDATE
             SET repository_path = excluded.repository_path
             WHERE project_computers.repository_path != excluded.repository_path",
            params![computer_id, project.folder],
        )
        .map_err(StorageError::backend)?;
        let ids = db
            .prepare("SELECT id FROM projects ORDER BY id")
            .map_err(StorageError::backend)?
            .query_map([], |row| row.get::<_, i64>(0))
            .map_err(StorageError::backend)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(StorageError::backend)?;
        let [project_id] = ids.as_slice() else {
            return Err(StorageError::backend(
                "A project database must contain exactly one project",
            ));
        };
        Ok(Self {
            db,
            project_id: *project_id,
        })
    }

    pub(super) fn into_connection(self) -> Connection {
        self.db
    }

    pub(super) fn rules_snapshot(&mut self) -> StorageResult<RuleSnapshot> {
        let tx = self.db.transaction().map_err(StorageError::backend)?;
        let project = tx
            .query_row(
                "SELECT slug, name FROM projects WHERE id=?1",
                [self.project_id],
                |row| {
                    Ok(ProjectIdentity {
                        id: row.get(0)?,
                        name: row.get(1)?,
                    })
                },
            )
            .map_err(StorageError::backend)?;
        let cursor = cursor(&tx, self.project_id)?;
        let rules = rules::load_rules(&tx).map_err(StorageError::backend)?;
        tx.commit().map_err(StorageError::backend)?;
        Ok(RuleSnapshot {
            project,
            cursor,
            rules,
        })
    }

    pub(super) fn change_cursor(&mut self) -> StorageResult<ChangeCursor> {
        cursor(&self.db, self.project_id)
    }

    pub(super) fn mutate_rules(
        &mut self,
        prepared: PreparedRuleMutation,
    ) -> StorageResult<RuleMutationResult> {
        let PreparedRuleMutation {
            mutation,
            fingerprint,
        } = prepared;
        // Serialize only the physical SQLite write transaction, not a project-wide
        // optimistic edit lock. Every expectation is still resource-scoped.
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(StorageError::backend)?;
        let receipt = tx.query_row(
            "SELECT result_json FROM mutation_operations WHERE project_id=?1 AND operation_id=?2",
            params![self.project_id, mutation.operation_id], |row| row.get::<_, String>(0),
        ).optional().map_err(StorageError::backend)?;
        if let Some(receipt) = receipt {
            let receipt: RuleReceipt =
                serde_json::from_str(&receipt).map_err(|_| StorageError::OperationReused)?;
            if receipt.format != RULE_RECEIPT_FORMAT || receipt.fingerprint != fingerprint {
                return Err(StorageError::OperationReused);
            }
            tx.commit().map_err(StorageError::backend)?;
            return Ok(receipt.result);
        }

        let mut expectations = Vec::new();
        for change in &mutation.changes {
            if let Some((id, expected)) = change.expectation() {
                let current =
                    concurrency::load_version(&tx, self.project_id, "rule", &id.to_string())
                        .map_err(StorageError::backend)?;
                expectations.push((id, expected, current));
            }
        }
        contract::check_expectations(expectations.into_iter())?;

        let mut outcomes = Vec::new();
        let mut changed = false;
        for change in mutation.changes {
            match change {
                RuleChange::Create { rule } => {
                    let id = rules::create_rule(&tx, self.project_id, rule)
                        .map_err(StorageError::backend)?;
                    concurrency::bump_version(&tx, self.project_id, "rule", &id.to_string())
                        .map_err(StorageError::backend)?;
                    outcomes.push(RuleOutcome::Saved {
                        rule: rules::load_rule(&tx, id).map_err(StorageError::backend)?,
                    });
                    changed = true;
                }
                RuleChange::Update { id, rule, .. } => {
                    require_rule(&tx, self.project_id, id)?;
                    let current = rules::load_rule(&tx, id).map_err(StorageError::backend)?;
                    if !contract::same_rule(&current, &rule) {
                        rules::update_rule(
                            &tx,
                            rules::UpdateRule {
                                id,
                                name: rule.name,
                                enabled: rule.enabled,
                                intend: rule.intend,
                                hook: rule.hook,
                                prompt: rule.prompt,
                            },
                        )
                        .map_err(StorageError::backend)?;
                        concurrency::bump_version(&tx, self.project_id, "rule", &id.to_string())
                            .map_err(StorageError::backend)?;
                        changed = true;
                    }
                    outcomes.push(RuleOutcome::Saved {
                        rule: rules::load_rule(&tx, id).map_err(StorageError::backend)?,
                    });
                }
                RuleChange::Delete { id, .. } => {
                    require_rule(&tx, self.project_id, id)?;
                    rules::delete_rule(&tx, id).map_err(StorageError::backend)?;
                    let version = concurrency::tombstone_version(
                        &tx,
                        self.project_id,
                        "rule",
                        &id.to_string(),
                    )
                    .map_err(StorageError::backend)?
                    .version;
                    outcomes.push(RuleOutcome::Deleted { id, version });
                    changed = true;
                }
            }
        }
        if changed {
            state::bump_project_revision(&tx, self.project_id).map_err(StorageError::backend)?;
        }
        let result = RuleMutationResult {
            cursor: cursor(&tx, self.project_id)?,
            outcomes,
        };
        let receipt = RuleReceipt {
            format: RULE_RECEIPT_FORMAT.into(),
            fingerprint,
            result: result.clone(),
        };
        concurrency::record_no_op(&tx, self.project_id, &mutation.operation_id, &receipt)
            .map_err(StorageError::backend)?;
        tx.commit().map_err(StorageError::backend)?;
        Ok(result)
    }
}

fn cursor(db: &Connection, project_id: i64) -> StorageResult<ChangeCursor> {
    let revision = state::load_project_revision(db, project_id).map_err(StorageError::backend)?;
    Ok(ChangeCursor(format!("sqlite:{}", revision.revision)))
}

fn require_rule(db: &Connection, project_id: i64, id: i64) -> StorageResult<()> {
    let exists = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM rules WHERE project_id=?1 AND id=?2)",
            params![project_id, id],
            |row| row.get::<_, bool>(0),
        )
        .map_err(StorageError::backend)?;
    if exists {
        Ok(())
    } else {
        Err(StorageError::NotFound { kind: "rule", id })
    }
}
