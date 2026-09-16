use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::{concurrency, state};

const LEGACY_APPEND_MEMORY_RULE: &str = r#"LONG-TERM MEMORY PROTOCOL:
- Before starting any task, silently call `adashi_get_memory` to understand the current project state, recent architectural decisions, and ongoing bugs.
- At the end of every successful task or major discussion, you MUST call `adashi_append_memory_note`.
- Append a concise idempotent handoff note keyed by the current run and optional task. Normal agents must not replace or compact canonical shared memory."#;

const LEGACY_UPDATE_MEMORY_RULE: &str = r#"LONG-TERM MEMORY PROTOCOL:
- Before starting any task, silently call `adashi_get_memory` to understand the current project state, recent architectural decisions, and ongoing bugs.
- At the end of every successful task or major discussion, you MUST call `adashi_update_memory`.
- Summarize what you just built, any API quirks you discovered, and what the next logical steps are. Do not ask for permission to do this, just update the memory through Adashi."#;

const LEGACY_BOUNDED_MEMORY_RULE: &str = r#"PROJECT MEMORY PROTOCOL:
- Read the memory supplied at run.start, or call `adashi_get_memory` if it was not supplied.
- Writing memory is optional. Only use `adashi_append_memory_note` for an IMPORTANT handover that will materially help a future run: a durable decision, a non-obvious constraint, or an unresolved blocker with a concrete next step.
- Skip routine task summaries, successful checks, tool-availability confirmations, repeated facts, and anything already captured in tasks, design, or QA. If nothing important changed, do not write a note.
- Keep each handover focused and within 1,000 characters, keyed by the current run and optional task. Normal agents must not replace the shared summary.
- Adashi retains at most 12,000 characters across the summary and up to 20 recent handovers, removing oldest notes first. Memory is a short handover aid, not a log or archive."#;

/// Previous default that referenced the pre-grouping tool names; migrated on load.
const LEGACY_V2_MEMORY_RULE: &str = r#"PROJECT MEMORY PROTOCOL:
- run.start supplies the current summary within a separate 2,000-character budget, never the handover log. For relevant prior decisions, constraints or blockers, call adashi_get_memory with query, runId or taskId. General requests may need memory too; operational requests can select memoryContext=protocolOnly.
- Historical handovers are dated evidence, not authoritative current state. Resolved notes are hidden by default; includeSuperseded=true retrieves their provenance. If summary is omitted, retrieve it before work requiring project constraints.
- Writing is optional: append only important durable decisions, non-obvious constraints, or unresolved blockers with a concrete next step. Skip routine reports, checks, tool confirmations and facts already in tasks, design or QA.
- Keep complete handovers within 1,000 characters. Normal agents must not replace the shared summary. Only an authorized coordinator may update the summary and resolve explicitly reviewed note ids under expectedVersion.
- Retention is separate: summary <=4,000 characters, <=20 handovers, <=12,000 total characters, oldest removed first. Oversized new writes fail; legacy omissions are explicit."#;

const LEGACY_V3_MEMORY_RULE: &str = r#"PROJECT MEMORY PROTOCOL:
- run.start supplies the current summary within a separate 2,000-character budget, never the handover log. For relevant prior decisions, constraints or blockers, call adashi_memory with operation get and query, runId or taskId. General requests may need memory too; operational requests can select memoryContext=protocolOnly.
- Historical handovers are dated evidence, not authoritative current state. Resolved notes are hidden by default; includeSuperseded=true retrieves their provenance. If summary is omitted, retrieve it before work requiring project constraints.
- Writing is optional: append only important durable decisions, non-obvious constraints, or unresolved blockers with a concrete next step. Skip routine reports, checks, tool confirmations and facts already in tasks, design or QA.
- Keep complete handovers within 1,000 characters. Normal agents must not replace the shared summary. Only an authorized coordinator may update the summary and resolve explicitly reviewed note ids under expectedVersion.
- Retention is separate: summary <=4,000 characters, <=20 handovers, <=12,000 total characters, oldest removed first. Oversized new writes fail; legacy omissions are explicit."#;

// Optional project-specific memory instructions. Shared workflow lives in agents_template.md.
pub const DEFAULT_MEMORY_RULE: &str = "";

pub const MAX_MEMORY_CHARS: usize = 12_000;
pub const MAX_SUMMARY_CHARS: usize = 4_000;
pub const MAX_NOTE_CHARS: usize = 1_000;
pub const MAX_NOTES: usize = 20;

#[derive(Clone, Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryLimits {
    pub total_chars: usize,
    pub summary_chars: usize,
    pub note_chars: usize,
    pub notes: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
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

#[derive(Clone, Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
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
    db.execute_batch("SAVEPOINT memory_maintenance")
        .map_err(|error| error.to_string())?;
    let result = maintain_memory(db, project_id);
    if result.is_err() {
        let _ = db.execute_batch("ROLLBACK TO memory_maintenance; RELEASE memory_maintenance");
        return result;
    }
    db.execute_batch("RELEASE memory_maintenance")
        .map_err(|error| error.to_string())
}

fn maintain_memory(db: &Connection, project_id: i64) -> Result<(), String> {
    db.execute(
        "INSERT OR IGNORE INTO project_memory(project_id, protocol_rule, memory_body)
         VALUES (?1, ?2, '')",
        params![project_id, DEFAULT_MEMORY_RULE],
    )
    .map_err(|error| error.to_string())?;
    ensure_memory_versions(db, project_id)?;
    let rule: String = db
        .query_row(
            "SELECT protocol_rule FROM project_memory WHERE project_id=?1",
            [project_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let legacy_rule = rule.replace("\r\n", "\n");
    let migrated = [
        LEGACY_APPEND_MEMORY_RULE,
        LEGACY_UPDATE_MEMORY_RULE,
        LEGACY_BOUNDED_MEMORY_RULE,
        LEGACY_V2_MEMORY_RULE,
        LEGACY_V3_MEMORY_RULE,
    ]
    .iter()
    .any(|legacy| {
        let normalize = |text: &str| {
            crate::prompt_hygiene::rewrite_removed_tool_references(text)
                .unwrap_or_else(|| text.to_string())
        };
        normalize(legacy) == normalize(legacy_rule.trim())
    });
    if migrated {
        db.execute(
            "UPDATE project_memory SET protocol_rule=?1 WHERE project_id=?2",
            params![DEFAULT_MEMORY_RULE, project_id],
        )
        .map_err(|error| error.to_string())?;
        concurrency::bump_version(db, project_id, "memory.protocol", "protocol")?;
    }
    let pruned = enforce_retention(db, project_id)?;
    if migrated || pruned {
        touch_memory(db, project_id)?;
        state::bump_project_revision(db, project_id)?;
    }
    // Old compaction receipts contained full memory snapshots, duplicating the log.
    // Keep the idempotency key, but replay memory mutations with the current bounded view.
    db.execute(
        "UPDATE mutation_operations SET result_json='{\"memoryMutation\":true}'
         WHERE project_id=?1 AND json_type(result_json, '$.memoryVersion')='integer'
           AND json_type(result_json, '$.protocolVersion')='integer'
           AND json_type(result_json, '$.notes')='array'",
        [project_id],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

const LEGACY_OMISSION: &str = "[Legacy content omitted at a complete boundary to meet retention limits; this is an excerpt, not a current summary. Review the source task or design before relying on it.]";

fn bounded_legacy_excerpt(body: &str, limit: usize) -> String {
    let budget = limit.saturating_sub(LEGACY_OMISSION.chars().count() + 2);
    let prefix: String = body.chars().take(budget).collect();
    let mut end = 0;
    for (index, ch) in prefix.char_indices() {
        let next = index + ch.len_utf8();
        if matches!(ch, '.' | '!' | '?' | '。' | '！' | '？')
            && body
                .get(next..)
                .and_then(|s| s.chars().next())
                .is_none_or(char::is_whitespace)
        {
            end = next;
        }
        if prefix[..next].ends_with("\n\n") {
            end = next;
        }
    }
    let complete = prefix[..end].trim();
    if complete.is_empty() {
        LEGACY_OMISSION.into()
    } else {
        format!("{complete}\n\n{LEGACY_OMISSION}")
    }
}

fn touch_memory(db: &Connection, project_id: i64) -> Result<(), String> {
    db.execute(
        "UPDATE project_memory SET updated_at=CURRENT_TIMESTAMP WHERE project_id=?1",
        [project_id],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn enforce_retention(db: &Connection, project_id: i64) -> Result<bool, String> {
    // Legacy repair is an explicit excerpt, not an invented summary. Preserve only
    // complete paragraphs/sentences and label omitted material; never cut a word.
    let summary: String = db
        .query_row(
            "SELECT memory_body FROM project_memory WHERE project_id=?1",
            [project_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let summary_changed = summary.chars().count() > MAX_SUMMARY_CHARS;
    if summary_changed {
        db.execute(
            "UPDATE project_memory SET memory_body=?1 WHERE project_id=?2",
            params![
                bounded_legacy_excerpt(&summary, MAX_SUMMARY_CHARS),
                project_id
            ],
        )
        .map_err(|e| e.to_string())?;
        concurrency::bump_version(db, project_id, "memory.canonical", "canonical")?;
    }
    let summary_len: usize = db
        .query_row(
            "SELECT length(memory_body) FROM project_memory WHERE project_id=?1",
            [project_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let mut statement = db
        .prepare(
            "SELECT id, body FROM project_memory_notes WHERE project_id=?1 AND length(body)>?2",
        )
        .map_err(|e| e.to_string())?;
    let oversized = statement
        .query_map(params![project_id, MAX_NOTE_CHARS], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())?;
    let notes_changed = !oversized.is_empty();
    for (id, body) in oversized {
        db.execute(
            "UPDATE project_memory_notes SET body=?1 WHERE id=?2",
            params![bounded_legacy_excerpt(&body, MAX_NOTE_CHARS), id],
        )
        .map_err(|e| e.to_string())?;
    }
    let first_retained: Option<i64> = db
        .query_row(
            "SELECT MIN(id) FROM (
            SELECT id, ROW_NUMBER() OVER (ORDER BY id DESC) AS position,
                   SUM(length(body)) OVER (ORDER BY id DESC) AS chars
            FROM project_memory_notes WHERE project_id=?1
         ) WHERE position<=?2 AND chars<=?3",
            params![project_id, MAX_NOTES, MAX_MEMORY_CHARS - summary_len],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    // Expired receipts keep only identity, so retries cannot resurrect deleted bodies.
    db.execute(
        "UPDATE mutation_operations SET result_json=json_object('expiredMemoryNote', 1)
         WHERE project_id=?1 AND operation_id IN (
            SELECT operation_id FROM project_memory_notes
            WHERE project_id=?1 AND (?2 IS NULL OR id<?2)
         )",
        params![project_id, first_retained],
    )
    .map_err(|error| error.to_string())?;
    let removed = db
        .execute(
            "DELETE FROM project_memory_notes WHERE project_id=?1 AND (?2 IS NULL OR id<?2)",
            params![project_id, first_retained],
        )
        .map_err(|error| error.to_string())?
        > 0;
    // Retained notes already provide the replay result; do not store a second body.
    db.execute(
        "UPDATE mutation_operations SET result_json='{\"retainedMemoryNote\":true}'
         WHERE project_id=?1 AND operation_id IN (
            SELECT operation_id FROM project_memory_notes WHERE project_id=?1
         ) AND json_type(result_json, '$.body')='text'",
        [project_id],
    )
    .map_err(|error| error.to_string())?;
    Ok(summary_changed || notes_changed || removed)
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
        limits: MemoryLimits {
            total_chars: MAX_MEMORY_CHARS,
            summary_chars: MAX_SUMMARY_CHARS,
            note_chars: MAX_NOTE_CHARS,
            notes: MAX_NOTES,
        },
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
    ensure_project_memory(db, project_id)?;
    if let Some(existing) = load_note_by_operation(db, project_id, &input.operation_id)? {
        if existing.note_id == input.note_id.trim()
            && existing.run_id == input.run_id.trim()
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
    if concurrency::load_operation::<serde_json::Value>(db, project_id, &input.operation_id)?
        .is_some()
    {
        return Err("memory.note_expired: operationId was already used; expired notes cannot be appended again".into());
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
    enforce_retention(&tx, project_id)?;
    touch_memory(&tx, project_id)?;
    let revision = state::bump_project_revision(&tx, project_id)?.revision;
    concurrency::record_no_op(
        &tx,
        project_id,
        &input.operation_id,
        &serde_json::json!({"retainedMemoryNote": true}),
    )?;
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
    compact_memory_review(db, project_id, expected_version, operation_id, memory, &[])
}

/// Only an authorized coordinator supplies the exact reviewed note ids.
pub fn compact_memory_review(
    db: &mut Connection,
    project_id: i64,
    expected_version: i64,
    operation_id: &str,
    memory: String,
    superseded_note_ids: &[String],
) -> Result<(ProjectMemory, i64), String> {
    if operation_id.trim().is_empty() {
        return Err("operationId is required".to_string());
    }
    ensure_project_memory(db, project_id)?;
    if replay_memory_mutation(db, project_id, operation_id)? {
        return Ok((
            load_memory(db, project_id)?,
            state::load_project_revision(db, project_id)?.revision,
        ));
    }
    if memory.chars().count() > MAX_SUMMARY_CHARS {
        return Err(format!("Memory summary must be at most {MAX_SUMMARY_CHARS} characters; keep only important context"));
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
    let unique = superseded_note_ids
        .iter()
        .collect::<std::collections::HashSet<_>>();
    if unique.len() != superseded_note_ids.len() {
        return Err("memory.duplicate_note: supply each reviewed note id once".into());
    }
    let mut unresolved = Vec::new();
    for id in superseded_note_ids {
        let resolution: Option<Option<i64>> = tx.query_row(
            "SELECT r.summary_version FROM project_memory_notes n LEFT JOIN project_memory_note_resolutions r
             ON r.project_id=n.project_id AND r.note_id=n.note_id WHERE n.project_id=?1 AND n.note_id=?2",
            params![project_id, id], |r| r.get(0),
        ).optional().map_err(|e| e.to_string())?;
        match resolution {
            None => return Err(format!("memory.unknown_note: {id}")),
            Some(None) => unresolved.push(id),
            Some(Some(_)) => {}
        }
    }
    if !unresolved.is_empty() && memory.trim().is_empty() {
        return Err(
            "memory.summary_required: record the reviewed current state before resolving notes"
                .into(),
        );
    }
    if current.memory == memory && unresolved.is_empty() {
        record_memory_mutation(&tx, project_id, operation_id)?;
        let revision = state::load_project_revision(&tx, project_id)?.revision;
        tx.commit().map_err(|error| error.to_string())?;
        return Ok((current, revision));
    }
    tx.execute("UPDATE project_memory SET memory_body=?1, updated_at=CURRENT_TIMESTAMP WHERE project_id=?2", params![memory, project_id]).map_err(|error| error.to_string())?;
    let summary_version =
        concurrency::bump_version(&tx, project_id, "memory.canonical", "canonical")?;
    for id in unresolved {
        tx.execute("INSERT INTO project_memory_note_resolutions(project_id,note_id,summary_version,operation_id) VALUES(?1,?2,?3,?4)",
            params![project_id, id, summary_version.version, operation_id]).map_err(|e| e.to_string())?;
    }
    enforce_retention(&tx, project_id)?;
    let revision = state::bump_project_revision(&tx, project_id)?.revision;
    let result = load_memory(&tx, project_id)?;
    record_memory_mutation(&tx, project_id, operation_id)?;
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
    if operation_id.trim().is_empty() {
        return Err("operationId is required".to_string());
    }
    ensure_project_memory(db, project_id)?;
    if replay_memory_mutation(db, project_id, operation_id)? {
        return Ok((
            load_memory(db, project_id)?,
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
        record_memory_mutation(&tx, project_id, operation_id)?;
        let revision = state::load_project_revision(&tx, project_id)?.revision;
        tx.commit().map_err(|error| error.to_string())?;
        return Ok((current, revision));
    }
    tx.execute("UPDATE project_memory SET protocol_rule=?1, updated_at=CURRENT_TIMESTAMP WHERE project_id=?2", params![rule, project_id]).map_err(|error| error.to_string())?;
    concurrency::bump_version(&tx, project_id, "memory.protocol", "protocol")?;
    let revision = state::bump_project_revision(&tx, project_id)?.revision;
    let result = load_memory(&tx, project_id)?;
    record_memory_mutation(&tx, project_id, operation_id)?;
    tx.commit().map_err(|error| error.to_string())?;
    Ok((result, revision))
}

fn replay_memory_mutation(
    db: &Connection,
    project_id: i64,
    operation_id: &str,
) -> Result<bool, String> {
    match concurrency::load_operation::<serde_json::Value>(db, project_id, operation_id)? {
        None => Ok(false),
        Some(value) if value["memoryMutation"] == true => Ok(true),
        Some(_) => Err("operationId already identifies a different mutation".into()),
    }
}

fn record_memory_mutation(
    db: &Connection,
    project_id: i64,
    operation_id: &str,
) -> Result<(), String> {
    concurrency::record_no_op(
        db,
        project_id,
        operation_id,
        &serde_json::json!({"memoryMutation": true}),
    )
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
    Ok(load_retained_notes(db, project_id)?
        .into_iter()
        .filter(|n| n.superseded_by_version.is_none())
        .collect())
}

pub fn load_retained_notes(db: &Connection, project_id: i64) -> Result<Vec<MemoryNote>, String> {
    let mut statement = db
        .prepare(
            "SELECT n.note_id, n.operation_id, n.run_id, n.task_id, n.body, n.created_at, r.summary_version
             FROM project_memory_notes n LEFT JOIN project_memory_note_resolutions r
               ON r.project_id=n.project_id AND r.note_id=n.note_id
             WHERE n.project_id=?1 ORDER BY n.id",
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
        "SELECT n.note_id, n.operation_id, n.run_id, n.task_id, n.body, n.created_at, r.summary_version
         FROM project_memory_notes n LEFT JOIN project_memory_note_resolutions r
           ON r.project_id=n.project_id AND r.note_id=n.note_id
         WHERE n.project_id=?1 AND n.operation_id=?2",
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
        superseded_by_version: row.get(6)?,
    })
}

fn validate_note(input: &AppendMemoryNote) -> Result<(), String> {
    if input.body.trim().chars().count() > MAX_NOTE_CHARS {
        return Err(format!("Memory handover must be at most {MAX_NOTE_CHARS} characters; omit routine logs and keep only important context"));
    }
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
    fn custom_memory_instructions_can_be_cleared_without_changing_the_summary() {
        let mut db = database();
        db.execute(
            "UPDATE project_memory SET memory_body='Current project constraints.'",
            [],
        )
        .unwrap();
        let version = load_memory(&db, 1).unwrap().protocol_version;
        let (custom, _) = update_memory_rule(
            &mut db,
            1,
            version,
            "set-custom",
            "Record audit migration constraints.".into(),
        )
        .unwrap();
        let (cleared, revision) = update_memory_rule(
            &mut db,
            1,
            custom.protocol_version,
            "clear-custom",
            "".into(),
        )
        .unwrap();
        assert_eq!(cleared.rule, "");
        assert_eq!(cleared.memory, "Current project constraints.");
        assert_eq!(cleared.protocol_version, custom.protocol_version + 1);
        ensure_project_memory(&db, 1).unwrap();
        assert_eq!(
            state::load_project_revision(&db, 1).unwrap().revision,
            revision
        );
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

    #[test]
    fn legacy_protocols_migrate_once_and_custom_rules_survive() {
        for rule in [LEGACY_APPEND_MEMORY_RULE, LEGACY_UPDATE_MEMORY_RULE] {
            let db = database();
            db.execute(
                "UPDATE project_memory SET protocol_rule=?1",
                [rule.replace('\n', "\r\n")],
            )
            .unwrap();
            let before = state::load_project_revision(&db, 1).unwrap().revision;
            let memory = load_memory(&db, 1).unwrap();
            assert_eq!(memory.rule, DEFAULT_MEMORY_RULE);
            assert_eq!(memory.protocol_version, 2);
            assert_eq!(
                state::load_project_revision(&db, 1).unwrap().revision,
                before + 1
            );
            load_memory(&db, 1).unwrap();
            assert_eq!(
                state::load_project_revision(&db, 1).unwrap().revision,
                before + 1
            );
        }
        let db = database();
        db.execute(
            "UPDATE project_memory SET protocol_rule='My custom rule'",
            [],
        )
        .unwrap();
        let memory = load_memory(&db, 1).unwrap();
        assert_eq!(memory.rule, "My custom rule");
        assert_eq!(memory.protocol_version, 1);
    }

    #[test]
    fn retention_keeps_newest_notes_within_combined_unicode_budget() {
        let mut db = database();
        compact_memory(&mut db, 1, 1, "summary", "界".repeat(MAX_SUMMARY_CHARS)).unwrap();
        for index in 0..30 {
            append_note(
                &mut db,
                1,
                note(
                    &index.to_string(),
                    &format!("op-{index}"),
                    &"🦀".repeat(MAX_NOTE_CHARS),
                ),
            )
            .unwrap();
        }
        let memory = load_memory(&db, 1).unwrap();
        assert_eq!(memory.notes.len(), 8);
        assert_eq!(memory.notes[0].note_id, "22");
        assert_eq!(memory.notes.last().unwrap().note_id, "29");
        assert_eq!(
            memory.memory.chars().count()
                + memory
                    .notes
                    .iter()
                    .map(|note| note.body.chars().count())
                    .sum::<usize>(),
            MAX_MEMORY_CHARS
        );
        assert_eq!(
            memory.memory_version, 2,
            "appending notes must not invalidate summary editors"
        );
        let revision = state::load_project_revision(&db, 1).unwrap().revision;
        append_note(
            &mut db,
            1,
            note("29", "op-29", &"🦀".repeat(MAX_NOTE_CHARS)),
        )
        .unwrap();
        assert_eq!(
            state::load_project_revision(&db, 1).unwrap().revision,
            revision
        );
        assert!(
            append_note(&mut db, 1, note("0", "op-0", &"🦀".repeat(MAX_NOTE_CHARS)))
                .unwrap_err()
                .contains("memory.note_expired")
        );
        let retained_rows: usize = db
            .query_row("SELECT COUNT(*) FROM project_memory_notes", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(retained_rows, 8);
        let cached_bodies: usize = db.query_row("SELECT COUNT(*) FROM mutation_operations WHERE json_type(result_json, '$.body') IS NOT NULL OR json_type(result_json, '$.notes') IS NOT NULL", [], |row| row.get(0)).unwrap();
        assert_eq!(cached_bodies, 0);
    }

    #[test]
    fn count_limit_and_oversized_write_validation_do_not_lose_newest_notes() {
        let mut db = database();
        for index in 0..30 {
            append_note(
                &mut db,
                1,
                note(&index.to_string(), &format!("op-{index}"), "Small handover"),
            )
            .unwrap();
        }
        let memory = load_memory(&db, 1).unwrap();
        assert_eq!(memory.notes.len(), MAX_NOTES);
        assert_eq!(memory.notes[0].note_id, "10");
        let revision = state::load_project_revision(&db, 1).unwrap().revision;
        assert!(append_note(
            &mut db,
            1,
            note("large", "large", &"x".repeat(MAX_NOTE_CHARS + 1))
        )
        .unwrap_err()
        .contains("1000"));
        assert!(compact_memory(
            &mut db,
            1,
            memory.memory_version,
            "large-summary",
            "x".repeat(MAX_SUMMARY_CHARS + 1)
        )
        .unwrap_err()
        .contains("4000"));
        assert_eq!(
            state::load_project_revision(&db, 1).unwrap().revision,
            revision
        );
        assert_eq!(load_memory(&db, 1).unwrap().notes.len(), MAX_NOTES);
    }

    #[test]
    fn legacy_growth_is_pruned_on_read_including_old_replay_snapshots() {
        let mut db = database();
        db.execute(
            "UPDATE project_memory SET memory_body=?1, updated_at='2000-01-01'",
            ["é".repeat(20_000)],
        )
        .unwrap();
        for index in 0..40 {
            db.execute("INSERT INTO project_memory_notes(project_id,note_id,operation_id,run_id,body) VALUES(1,?1,?1,'legacy',?2)", params![index.to_string(), "🦀".repeat(2_000)]).unwrap();
            concurrency::record_no_op(
                &db,
                1,
                &index.to_string(),
                &serde_json::json!({"body": "copied log"}),
            )
            .unwrap();
        }
        concurrency::record_no_op(&db, 1, "old-snapshot", &serde_json::json!({"memoryVersion":1,"protocolVersion":1,"notes":[{"body":"old log"}],"memory":"old summary"})).unwrap();
        let memory = load_memory(&db, 1).unwrap();
        assert_eq!(memory.memory, LEGACY_OMISSION);
        assert_eq!(memory.notes.len(), MAX_NOTES);
        assert_eq!(memory.notes[0].note_id, "20");
        assert_eq!(memory.notes[0].body, LEGACY_OMISSION);
        assert_eq!(memory.memory_version, 2);
        assert_ne!(memory.updated_at, "2000-01-01");
        let revision = state::load_project_revision(&db, 1).unwrap().revision;
        let (replay, replay_revision) =
            compact_memory(&mut db, 1, 1, "old-snapshot", "stale data".into()).unwrap();
        assert_eq!(replay.memory, memory.memory);
        assert_eq!(replay_revision, revision);
        let cached: String = db
            .query_row(
                "SELECT result_json FROM mutation_operations WHERE operation_id='old-snapshot'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(cached, "{\"memoryMutation\":true}");
        load_memory(&db, 1).unwrap();
        assert_eq!(
            state::load_project_revision(&db, 1).unwrap().revision,
            revision
        );
    }

    #[test]
    fn migration_rolls_back_protocol_and_truncation_on_failure() {
        let db = database();
        db.execute(
            "UPDATE project_memory SET protocol_rule=?1, memory_body=?2",
            params![LEGACY_APPEND_MEMORY_RULE, "x".repeat(20_000)],
        )
        .unwrap();
        db.execute("INSERT INTO project_memory_notes(project_id,note_id,operation_id,run_id,body) VALUES(1,'old','old','old',?1)", ["x".repeat(2_000)]).unwrap();
        db.execute_batch("CREATE TRIGGER reject_truncation BEFORE UPDATE ON project_memory_notes BEGIN SELECT RAISE(ABORT, 'simulated failure'); END;").unwrap();
        assert!(load_memory(&db, 1)
            .unwrap_err()
            .contains("simulated failure"));
        let (rule, summary_len): (String, usize) = db
            .query_row(
                "SELECT protocol_rule, length(memory_body) FROM project_memory",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(rule, LEGACY_APPEND_MEMORY_RULE);
        assert_eq!(summary_len, 20_000);
        assert_eq!(
            concurrency::load_version(&db, 1, "memory.protocol", "protocol").unwrap(),
            1
        );
        assert_eq!(
            concurrency::load_version(&db, 1, "memory.canonical", "canonical").unwrap(),
            1
        );
    }

    #[test]
    fn legacy_excerpts_end_at_complete_unicode_boundaries_and_label_omissions() {
        let body = format!(
            "A complete fact about 🦀.\n\n{}",
            "unfinished ".repeat(1000)
        );
        let excerpt = bounded_legacy_excerpt(&body, MAX_NOTE_CHARS);
        assert_eq!(
            excerpt,
            format!("A complete fact about 🦀.\n\n{LEGACY_OMISSION}")
        );
        assert!(excerpt.chars().count() <= MAX_NOTE_CHARS);
        assert_eq!(
            bounded_legacy_excerpt(&"x".repeat(2000), MAX_NOTE_CHARS),
            LEGACY_OMISSION
        );
    }

    #[test]
    fn coordinator_resolution_is_atomic_versioned_and_preserves_parallel_notes() {
        let mut db = database();
        append_note(&mut db, 1, note("old", "old-op", "Old finding.")).unwrap();
        let version = load_memory(&db, 1).unwrap().memory_version;
        append_note(
            &mut db,
            1,
            note("parallel", "parallel-op", "New unrelated blocker."),
        )
        .unwrap();
        assert!(compact_memory_review(
            &mut db,
            1,
            version,
            "bad",
            "Current state.".into(),
            &["old".into(), "missing".into()]
        )
        .is_err());
        assert_eq!(load_memory(&db, 1).unwrap().notes.len(), 2);
        let (current, revision) = compact_memory_review(
            &mut db,
            1,
            version,
            "review",
            "Current state.".into(),
            &["old".into()],
        )
        .unwrap();
        assert_eq!(current.memory_version, version + 1);
        assert_eq!(current.notes.len(), 1);
        assert_eq!(current.notes[0].note_id, "parallel");
        let retained = load_retained_notes(&db, 1).unwrap();
        assert_eq!(retained[0].body, "Old finding.");
        assert_eq!(
            retained[0].superseded_by_version,
            Some(current.memory_version)
        );
        let (_, replay_revision) = compact_memory_review(
            &mut db,
            1,
            version,
            "review",
            "Current state.".into(),
            &["old".into()],
        )
        .unwrap();
        assert_eq!(revision, replay_revision);
        assert!(compact_memory_review(
            &mut db,
            1,
            version,
            "stale",
            "Stale state.".into(),
            &["parallel".into()]
        )
        .unwrap_err()
        .contains("resource.conflict"));
    }
}
