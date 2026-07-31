use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::{concurrency, state};

pub const DEFAULT_MEMORY_RULE: &str = r#"LONG-TERM MEMORY PROTOCOL:
- Before starting any task, silently call `adashi_get_memory` to understand the current project state, recent architectural decisions, and ongoing bugs.
- At the end of every successful task or major discussion, you MUST call `adashi_append_memory_note`.
- Append a concise idempotent handoff note keyed by the current run and optional task. Normal agents must not replace or compact canonical shared memory."#;

#[derive(Clone, Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryNote {
    pub note_id: String,
    pub operation_id: String,
    pub run_id: String,
    pub task_id: Option<i64>,
    pub body: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectMemory {
    pub rule: String,
    pub memory: String,
    pub memory_version: i64,
    pub protocol_version: i64,
    pub notes: Vec<MemoryNote>,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppendMemoryNote {
    pub note_id: String,
    pub operation_id: String,
    pub run_id: String,
    pub task_id: Option<i64>,
    pub body: String,
}

pub fn ensure_project_memory(db: &Connection, project_id: i64) -> Result<(), String> {
    db.execute(
        "INSERT OR IGNORE INTO project_memory(project_id, protocol_rule, memory_body)
         VALUES (?1, ?2, '')",
        params![project_id, DEFAULT_MEMORY_RULE],
    )
    .map_err(|error| error.to_string())?;
    ensure_memory_versions(db, project_id)?;
    Ok(())
}

pub fn load_memory(db: &Connection, project_id: i64) -> Result<ProjectMemory, String> {
    ensure_project_memory(db, project_id)?;
    let (rule, memory, updated_at) = db
        .query_row(
            "SELECT protocol_rule, memory_body, updated_at
             FROM project_memory
             WHERE project_id=?1",
            params![project_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .map_err(|error| error.to_string())?;
    Ok(ProjectMemory {
        rule,
        memory,
        memory_version: concurrency::load_version(db, project_id, "memory.canonical", "canonical")?,
        protocol_version: concurrency::load_version(db, project_id, "memory.protocol", "protocol")?,
        notes: load_notes(db, project_id)?,
        updated_at,
    })
}

pub fn append_note(
    db: &mut Connection,
    project_id: i64,
    input: AppendMemoryNote,
) -> Result<(MemoryNote, i64), String> {
    validate_note(&input)?;
    if let Some(existing) = load_note_by_operation(db, project_id, &input.operation_id)? {
        if existing.note_id == input.note_id
            && existing.run_id == input.run_id
            && existing.task_id == input.task_id
            && existing.body == input.body.trim()
        {
            return Ok((
                existing,
                state::load_project_revision(db, project_id)?.revision,
            ));
        }
        return Err(format!(
            "operationId '{}' already identifies a different memory note",
            input.operation_id
        ));
    }

    let tx = db.transaction().map_err(|error| error.to_string())?;
    tx.execute(
        "INSERT INTO project_memory_notes(
            project_id, note_id, operation_id, run_id, task_id, body
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            project_id,
            input.note_id.trim(),
            input.operation_id.trim(),
            input.run_id.trim(),
            input.task_id,
            input.body.trim()
        ],
    )
    .map_err(|error| error.to_string())?;
    let note = load_note_by_operation(&tx, project_id, &input.operation_id)?
        .ok_or_else(|| "Stored memory note could not be reloaded".to_string())?;
    let revision = state::bump_project_revision(&tx, project_id)?.revision;
    concurrency::record_no_op(&tx, project_id, &input.operation_id, &note)?;
    tx.commit().map_err(|error| error.to_string())?;
    Ok((note, revision))
}

pub fn compact_memory(
    db: &mut Connection,
    project_id: i64,
    expected_version: i64,
    operation_id: &str,
    memory: String,
) -> Result<(ProjectMemory, i64), String> {
    if operation_id.trim().is_empty() {
        return Err("operationId is required".to_string());
    }
    if let Some(replayed) =
        concurrency::load_operation::<ProjectMemory>(db, project_id, operation_id)?
    {
        return Ok((
            replayed,
            state::load_project_revision(db, project_id)?.revision,
        ));
    }
    let tx = db.transaction().map_err(|error| error.to_string())?;
    validate_expected(
        &tx,
        project_id,
        "memory.canonical",
        "canonical",
        expected_version,
    )?;
    let current = load_memory(&tx, project_id)?;
    if current.memory == memory {
        concurrency::record_no_op(&tx, project_id, operation_id, &current)?;
        let revision = state::load_project_revision(&tx, project_id)?.revision;
        tx.commit().map_err(|error| error.to_string())?;
        return Ok((current, revision));
    }
    tx.execute("UPDATE project_memory SET memory_body=?1, updated_at=CURRENT_TIMESTAMP WHERE project_id=?2", params![memory, project_id]).map_err(|error| error.to_string())?;
    concurrency::bump_version(&tx, project_id, "memory.canonical", "canonical")?;
    let revision = state::bump_project_revision(&tx, project_id)?.revision;
    let result = load_memory(&tx, project_id)?;
    concurrency::record_no_op(&tx, project_id, operation_id, &result)?;
    tx.commit().map_err(|error| error.to_string())?;
    Ok((result, revision))
}

pub fn update_memory_rule(
    db: &mut Connection,
    project_id: i64,
    expected_version: i64,
    operation_id: &str,
    rule: String,
) -> Result<(ProjectMemory, i64), String> {
    let rule = rule.trim();
    if rule.is_empty() {
        return Err("Memory rule is required".to_string());
    }
    if operation_id.trim().is_empty() {
        return Err("operationId is required".to_string());
    }
    if let Some(replayed) =
        concurrency::load_operation::<ProjectMemory>(db, project_id, operation_id)?
    {
        return Ok((
            replayed,
            state::load_project_revision(db, project_id)?.revision,
        ));
    }
    let tx = db.transaction().map_err(|error| error.to_string())?;
    validate_expected(
        &tx,
        project_id,
        "memory.protocol",
        "protocol",
        expected_version,
    )?;
    let current = load_memory(&tx, project_id)?;
    if current.rule == rule {
        concurrency::record_no_op(&tx, project_id, operation_id, &current)?;
        let revision = state::load_project_revision(&tx, project_id)?.revision;
        tx.commit().map_err(|error| error.to_string())?;
        return Ok((current, revision));
    }
    tx.execute("UPDATE project_memory SET protocol_rule=?1, updated_at=CURRENT_TIMESTAMP WHERE project_id=?2", params![rule, project_id]).map_err(|error| error.to_string())?;
    concurrency::bump_version(&tx, project_id, "memory.protocol", "protocol")?;
    let revision = state::bump_project_revision(&tx, project_id)?.revision;
    let result = load_memory(&tx, project_id)?;
    concurrency::record_no_op(&tx, project_id, operation_id, &result)?;
    tx.commit().map_err(|error| error.to_string())?;
    Ok((result, revision))
}

fn ensure_memory_versions(db: &Connection, project_id: i64) -> Result<(), String> {
    for (kind, id) in [
        ("memory.canonical", "canonical"),
        ("memory.protocol", "protocol"),
    ] {
        db.execute(
            "INSERT OR IGNORE INTO resource_versions(
                project_id, resource_kind, resource_id, version
             ) VALUES (?1, ?2, ?3, 1)",
            params![project_id, kind, id],
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn load_notes(db: &Connection, project_id: i64) -> Result<Vec<MemoryNote>, String> {
    let mut statement = db
        .prepare(
            "SELECT note_id, operation_id, run_id, task_id, body, created_at
             FROM project_memory_notes
             WHERE project_id=?1
             ORDER BY id",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![project_id], read_note)
        .map_err(|error| error.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| error.to_string())
}

fn load_note_by_operation(
    db: &Connection,
    project_id: i64,
    operation_id: &str,
) -> Result<Option<MemoryNote>, String> {
    db.query_row(
        "SELECT note_id, operation_id, run_id, task_id, body, created_at
         FROM project_memory_notes
         WHERE project_id=?1 AND operation_id=?2",
        params![project_id, operation_id.trim()],
        read_note,
    )
    .optional()
    .map_err(|error| error.to_string())
}

fn read_note(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryNote> {
    Ok(MemoryNote {
        note_id: row.get(0)?,
        operation_id: row.get(1)?,
        run_id: row.get(2)?,
        task_id: row.get(3)?,
        body: row.get(4)?,
        created_at: row.get(5)?,
    })
}

fn validate_note(input: &AppendMemoryNote) -> Result<(), String> {
    for (field, value) in [
        ("noteId", input.note_id.as_str()),
        ("operationId", input.operation_id.as_str()),
        ("runId", input.run_id.as_str()),
        ("body", input.body.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(format!("{field} is required"));
        }
    }
    Ok(())
}

fn validate_expected(
    db: &Connection,
    project_id: i64,
    kind: &str,
    id: &str,
    expected_version: i64,
) -> Result<(), String> {
    let current_version = concurrency::load_version(db, project_id, kind, id)?;
    if current_version == expected_version {
        Ok(())
    } else {
        Err(serde_json::json!({
            "code": "resource.conflict",
            "conflicts": [{
                "resourceKind": kind,
                "resourceId": id,
                "expectedVersion": expected_version,
                "currentVersion": current_version
            }]
        })
        .to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database() -> Connection {
        let mut db = Connection::open_in_memory().unwrap();
        crate::schema::migrate(&mut db).unwrap();
        db.execute(
            "INSERT INTO projects(name, slug) VALUES ('Test', 'test')",
            [],
        )
        .unwrap();
        crate::state::ensure_project_state(&db).unwrap();
        ensure_project_memory(&db, 1).unwrap();
        db
    }

    fn note(id: &str, operation: &str, body: &str) -> AppendMemoryNote {
        AppendMemoryNote {
            note_id: id.to_string(),
            operation_id: operation.to_string(),
            run_id: format!("run-{id}"),
            task_id: None,
            body: body.to_string(),
        }
    }

    #[test]
    fn note_retry_is_idempotent_and_parallel_notes_survive_compaction() {
        let mut db = database();
        let snapshot_version = load_memory(&db, 1).unwrap().memory_version;
        let (_, revision_one) =
            append_note(&mut db, 1, note("one", "note-op-one", "First")).unwrap();
        let (_, retry_revision) =
            append_note(&mut db, 1, note("one", "note-op-one", "First")).unwrap();
        assert_eq!(retry_revision, revision_one);
        append_note(&mut db, 1, note("two", "note-op-two", "Later note")).unwrap();

        let (compacted, revision_three) = compact_memory(
            &mut db,
            1,
            snapshot_version,
            "compact-op",
            "Canonical".into(),
        )
        .unwrap();
        assert_eq!(compacted.notes.len(), 2);
        assert_eq!(compacted.memory, "Canonical");
        assert_eq!(revision_three, 3);
        assert_eq!(compacted.memory_version, snapshot_version + 1);
    }

    #[test]
    fn stale_compaction_conflicts_and_semantic_no_op_does_not_bump() {
        let mut db = database();
        let initial = load_memory(&db, 1).unwrap();
        let (stored, changed_revision) = compact_memory(
            &mut db,
            1,
            initial.memory_version,
            "compact-a",
            "Canonical".into(),
        )
        .unwrap();
        let (same, same_revision) = compact_memory(
            &mut db,
            1,
            stored.memory_version,
            "compact-no-op",
            "Canonical".into(),
        )
        .unwrap();
        assert_eq!(same.memory_version, stored.memory_version);
        assert_eq!(same_revision, changed_revision);

        let error = compact_memory(
            &mut db,
            1,
            initial.memory_version,
            "compact-stale",
            "Stale".into(),
        )
        .unwrap_err();
        let conflict: serde_json::Value = serde_json::from_str(&error).unwrap();
        assert_eq!(conflict["conflicts"][0]["resourceKind"], "memory.canonical");
        assert_eq!(load_memory(&db, 1).unwrap().memory, "Canonical");
    }
}
