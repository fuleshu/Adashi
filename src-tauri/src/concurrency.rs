use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceExpectation {
    pub resource_kind: String,
    pub resource_id: String,
    pub expected_version: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MutationGuard {
    pub operation_id: String,
    #[serde(default)]
    pub read_set: Vec<ResourceExpectation>,
    #[serde(default)]
    pub write_set: Vec<ResourceExpectation>,
}

#[derive(Clone, Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResourceVersion {
    pub resource_kind: String,
    pub resource_id: String,
    pub version: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResourceConflict {
    pub resource_kind: String,
    pub resource_id: String,
    pub expected_version: i64,
    pub current_version: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResourceIntent {
    pub agent_run_id: String,
    pub resource_kind: String,
    pub resource_id: String,
    pub expires_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConflictEnvelope<'a> {
    code: &'static str,
    conflicts: &'a [ResourceConflict],
}

pub fn validate_guard(
    db: &Connection,
    project_id: i64,
    guard: &MutationGuard,
) -> Result<(), String> {
    if guard.operation_id.trim().is_empty() {
        return Err("operationId is required".to_string());
    }

    let mut identities = std::collections::HashSet::new();
    for expectation in guard.read_set.iter().chain(&guard.write_set) {
        validate_identity(&expectation.resource_kind, &expectation.resource_id)?;
        let identity = (
            expectation.resource_kind.trim().to_string(),
            expectation.resource_id.trim().to_string(),
        );
        if !identities.insert(identity) {
            return Err(format!(
                "Resource '{}:{}' occurs more than once in the mutation guard",
                expectation.resource_kind, expectation.resource_id
            ));
        }
    }

    let conflicts = guard
        .read_set
        .iter()
        .chain(&guard.write_set)
        .filter_map(|expected| {
            let current = load_version(
                db,
                project_id,
                expected.resource_kind.trim(),
                expected.resource_id.trim(),
            );
            match current {
                Ok(current_version) if current_version != expected.expected_version => {
                    Some(Ok(ResourceConflict {
                        resource_kind: expected.resource_kind.trim().to_string(),
                        resource_id: expected.resource_id.trim().to_string(),
                        expected_version: expected.expected_version,
                        current_version,
                    }))
                }
                Ok(_) => None,
                Err(error) => Some(Err(error)),
            }
        })
        .collect::<Result<Vec<_>, _>>()?;

    if conflicts.is_empty() {
        Ok(())
    } else {
        Err(serde_json::to_string(&ConflictEnvelope {
            code: "resource.conflict",
            conflicts: &conflicts,
        })
        .map_err(|error| error.to_string())?)
    }
}

pub fn require_write_target(
    guard: &MutationGuard,
    resource_kind: &str,
    resource_id: &str,
) -> Result<(), String> {
    if guard.write_set.iter().any(|resource| {
        resource.resource_kind.trim() == resource_kind && resource.resource_id.trim() == resource_id
    }) {
        Ok(())
    } else {
        Err(format!(
            "writeSet must contain resource '{resource_kind}:{resource_id}'"
        ))
    }
}

pub fn load_version(
    db: &Connection,
    project_id: i64,
    resource_kind: &str,
    resource_id: &str,
) -> Result<i64, String> {
    validate_identity(resource_kind, resource_id)?;
    db.query_row(
        "SELECT version
         FROM resource_versions
         WHERE project_id=?1 AND resource_kind=?2 AND resource_id=?3",
        params![project_id, resource_kind.trim(), resource_id.trim()],
        |row| row.get(0),
    )
    .optional()
    .map_err(|error| error.to_string())
    .map(|version| version.unwrap_or(0))
}

pub fn bump_version(
    db: &Connection,
    project_id: i64,
    resource_kind: &str,
    resource_id: &str,
) -> Result<ResourceVersion, String> {
    validate_identity(resource_kind, resource_id)?;
    db.execute(
        "INSERT INTO resource_versions(project_id, resource_kind, resource_id, version)
         VALUES (?1, ?2, ?3, 1)
         ON CONFLICT(project_id, resource_kind, resource_id)
         DO UPDATE SET version=resource_versions.version+1, updated_at=CURRENT_TIMESTAMP",
        params![project_id, resource_kind.trim(), resource_id.trim()],
    )
    .map_err(|error| error.to_string())?;
    Ok(ResourceVersion {
        resource_kind: resource_kind.trim().to_string(),
        resource_id: resource_id.trim().to_string(),
        version: load_version(db, project_id, resource_kind, resource_id)?,
    })
}

pub fn tombstone_version(
    db: &Connection,
    project_id: i64,
    resource_kind: &str,
    resource_id: &str,
) -> Result<ResourceVersion, String> {
    bump_version(db, project_id, resource_kind, resource_id)
}

pub fn record_no_op<T: Serialize>(
    db: &Connection,
    project_id: i64,
    operation_id: &str,
    result: &T,
) -> Result<(), String> {
    record_operation(db, project_id, operation_id, result)
}

pub fn load_operation<T: DeserializeOwned>(
    db: &Connection,
    project_id: i64,
    operation_id: &str,
) -> Result<Option<T>, String> {
    if operation_id.trim().is_empty() {
        return Err("operationId is required".to_string());
    }
    let value = db
        .query_row(
            "SELECT result_json
             FROM mutation_operations
             WHERE project_id=?1 AND operation_id=?2",
            params![project_id, operation_id.trim()],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    value
        .map(|json| serde_json::from_str(&json).map_err(|error| error.to_string()))
        .transpose()
}

pub fn publish_intent(
    db: &Connection,
    project_id: i64,
    agent_run_id: &str,
    resource_kind: &str,
    resource_id: &str,
    ttl_seconds: i64,
) -> Result<ResourceIntent, String> {
    validate_identity(resource_kind, resource_id)?;
    if agent_run_id.trim().is_empty() {
        return Err("agentRunId is required".to_string());
    }
    if !(1..=86_400).contains(&ttl_seconds) {
        return Err("ttlSeconds must be between 1 and 86400".to_string());
    }
    prune_expired_intents(db, project_id)?;
    let modifier = format!("+{ttl_seconds} seconds");
    db.execute(
        "INSERT INTO resource_intents(
            project_id, agent_run_id, resource_kind, resource_id, expires_at
         )
         VALUES (?1, ?2, ?3, ?4, datetime('now', ?5))
         ON CONFLICT(project_id, agent_run_id, resource_kind, resource_id)
         DO UPDATE SET expires_at=excluded.expires_at, updated_at=CURRENT_TIMESTAMP",
        params![
            project_id,
            agent_run_id.trim(),
            resource_kind.trim(),
            resource_id.trim(),
            modifier
        ],
    )
    .map_err(|error| error.to_string())?;
    db.query_row(
        "SELECT agent_run_id, resource_kind, resource_id, expires_at
         FROM resource_intents
         WHERE project_id=?1 AND agent_run_id=?2 AND resource_kind=?3 AND resource_id=?4",
        params![
            project_id,
            agent_run_id.trim(),
            resource_kind.trim(),
            resource_id.trim()
        ],
        |row| {
            Ok(ResourceIntent {
                agent_run_id: row.get(0)?,
                resource_kind: row.get(1)?,
                resource_id: row.get(2)?,
                expires_at: row.get(3)?,
            })
        },
    )
    .map_err(|error| error.to_string())
}

pub fn load_live_intents(db: &Connection, project_id: i64) -> Result<Vec<ResourceIntent>, String> {
    prune_expired_intents(db, project_id)?;
    let mut statement = db
        .prepare(
            "SELECT agent_run_id, resource_kind, resource_id, expires_at
             FROM resource_intents
             WHERE project_id=?1 AND expires_at > CURRENT_TIMESTAMP
             ORDER BY expires_at, agent_run_id, resource_kind, resource_id",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![project_id], |row| {
            Ok(ResourceIntent {
                agent_run_id: row.get(0)?,
                resource_kind: row.get(1)?,
                resource_id: row.get(2)?,
                expires_at: row.get(3)?,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| error.to_string())
}

fn prune_expired_intents(db: &Connection, project_id: i64) -> Result<(), String> {
    db.execute(
        "DELETE FROM resource_intents
         WHERE project_id=?1 AND expires_at <= CURRENT_TIMESTAMP",
        params![project_id],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn record_operation<T: Serialize>(
    db: &Connection,
    project_id: i64,
    operation_id: &str,
    result: &T,
) -> Result<(), String> {
    if operation_id.trim().is_empty() {
        return Err("operationId is required".to_string());
    }
    let result_json = serde_json::to_string(result).map_err(|error| error.to_string())?;
    db.execute(
        "INSERT INTO mutation_operations(project_id, operation_id, result_json)
         VALUES (?1, ?2, ?3)",
        params![project_id, operation_id.trim(), result_json],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn validate_identity(resource_kind: &str, resource_id: &str) -> Result<(), String> {
    if resource_kind.trim().is_empty() {
        return Err("resourceKind is required".to_string());
    }
    if resource_id.trim().is_empty() {
        return Err("resourceId is required".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state;

    fn database() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "PRAGMA foreign_keys=ON;
             CREATE TABLE projects(id INTEGER PRIMARY KEY);
             INSERT INTO projects(id) VALUES (1);
             CREATE TABLE project_state(
                project_id INTEGER PRIMARY KEY,
                revision INTEGER NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );
             INSERT INTO project_state(project_id) VALUES (1);
             CREATE TABLE resource_versions(
                project_id INTEGER NOT NULL,
                resource_kind TEXT NOT NULL,
                resource_id TEXT NOT NULL,
                version INTEGER NOT NULL DEFAULT 1,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY(project_id, resource_kind, resource_id)
             );
             CREATE TABLE mutation_operations(
                project_id INTEGER NOT NULL,
                operation_id TEXT NOT NULL,
                result_json TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY(project_id, operation_id)
             );
             CREATE TABLE resource_intents(
                project_id INTEGER NOT NULL,
                agent_run_id TEXT NOT NULL,
                resource_kind TEXT NOT NULL,
                resource_id TEXT NOT NULL,
                expires_at TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY(project_id, agent_run_id, resource_kind, resource_id)
             );",
        )
        .unwrap();
        db
    }

    #[test]
    fn disjoint_resources_do_not_conflict() {
        let db = database();
        bump_version(&db, 1, "task", "1").unwrap();
        bump_version(&db, 1, "task", "2").unwrap();
        let guard = MutationGuard {
            operation_id: "op-a".into(),
            read_set: vec![],
            write_set: vec![ResourceExpectation {
                resource_kind: "task".into(),
                resource_id: "1".into(),
                expected_version: 1,
            }],
        };
        validate_guard(&db, 1, &guard).unwrap();
        bump_version(&db, 1, "task", "2").unwrap();
        validate_guard(&db, 1, &guard).unwrap();
    }

    #[test]
    fn same_resource_returns_precise_conflict() {
        let db = database();
        bump_version(&db, 1, "task", "1").unwrap();
        let guard = MutationGuard {
            operation_id: "op-a".into(),
            read_set: vec![],
            write_set: vec![ResourceExpectation {
                resource_kind: "task".into(),
                resource_id: "1".into(),
                expected_version: 0,
            }],
        };
        let error = validate_guard(&db, 1, &guard).unwrap_err();
        let value: serde_json::Value = serde_json::from_str(&error).unwrap();
        assert_eq!(value["code"], "resource.conflict");
        assert_eq!(value["conflicts"][0]["resourceKind"], "task");
        assert_eq!(value["conflicts"][0]["resourceId"], "1");
        assert_eq!(value["conflicts"][0]["expectedVersion"], 0);
        assert_eq!(value["conflicts"][0]["currentVersion"], 1);
    }

    #[test]
    fn idempotency_result_round_trips() {
        let db = database();
        record_no_op(&db, 1, "op-a", &serde_json::json!({"ok": true})).unwrap();
        let result: serde_json::Value = load_operation(&db, 1, "op-a").unwrap().unwrap();
        assert_eq!(result["ok"], true);
    }

    #[test]
    fn live_intents_are_advisory_and_expired_rows_are_pruned() {
        let db = database();
        publish_intent(&db, 1, "run-a", "task", "1", 30).unwrap();
        db.execute(
            "INSERT INTO resource_intents(
                project_id, agent_run_id, resource_kind, resource_id, expires_at
             ) VALUES (1, 'expired', 'task', '2', datetime('now', '-1 second'))",
            [],
        )
        .unwrap();
        assert_eq!(load_live_intents(&db, 1).unwrap().len(), 1);
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM resource_intents WHERE agent_run_id='expired'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
    }
    #[test]
    fn every_mutable_family_allows_disjoint_progress() {
        for kind in [
            "task",
            "qa.job",
            "rule",
            "design.element",
            "design.relationship",
            "design.uml",
            "design.binding",
            "mockup.accepted",
            "mockup.working",
        ] {
            let db = database();
            bump_version(&db, 1, kind, "a").unwrap();
            bump_version(&db, 1, kind, "b").unwrap();
            let guard = MutationGuard {
                operation_id: format!("{kind}-a"),
                read_set: vec![],
                write_set: vec![ResourceExpectation {
                    resource_kind: kind.into(),
                    resource_id: "a".into(),
                    expected_version: 1,
                }],
            };
            bump_version(&db, 1, kind, "b").unwrap();
            validate_guard(&db, 1, &guard).unwrap();
        }
    }

    #[test]
    fn unique_creates_are_independent_and_duplicate_identity_conflicts() {
        let db = database();
        let create = |id: &str, operation: &str| MutationGuard {
            operation_id: operation.into(),
            read_set: vec![],
            write_set: vec![ResourceExpectation {
                resource_kind: "task".into(),
                resource_id: id.into(),
                expected_version: 0,
            }],
        };
        validate_guard(&db, 1, &create("a", "create-a")).unwrap();
        bump_version(&db, 1, "task", "a").unwrap();
        validate_guard(&db, 1, &create("b", "create-b")).unwrap();
        assert!(validate_guard(&db, 1, &create("a", "duplicate-a")).is_err());
        let tombstone = tombstone_version(&db, 1, "task", "a").unwrap();
        assert_eq!(tombstone.version, 2);
        assert!(validate_guard(&db, 1, &create("a", "recreate-a")).is_err());
        assert_eq!(state::load_project_revision(&db, 1).unwrap().revision, 0);
    }
}
