use crate::design_health::*;
use rusqlite::{params, Connection};
#[cfg(test)]
use std::{
    collections::{BTreeSet, HashMap},
    path::Path,
};

#[cfg(test)]
fn collect_model(db: &Connection) -> Result<Model, String> {
    let mut element_statement = db
        .prepare(
            "SELECT e.external_id, e.name, e.element_type, e.parent_external_id
             FROM c4_elements e
             JOIN design_workspaces w ON w.id = e.workspace_id
             ORDER BY e.id",
        )
        .map_err(|err| err.to_string())?;
    let elements = element_statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(|err| err.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| err.to_string())?;

    let mut binding_statement = db
        .prepare(
            "SELECT b.design_external_id, b.target_type, b.target
             FROM design_bindings b
             JOIN design_workspaces w ON w.id = b.workspace_id
             ORDER BY b.target_type, b.target",
        )
        .map_err(|err| err.to_string())?;
    let bindings = binding_statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|err| err.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| err.to_string())?;

    let mut connected = BTreeSet::new();
    let mut relationship_statement = db
        .prepare(
            "SELECT r.source_external_id, r.destination_external_id
             FROM c4_relationships r
             JOIN design_workspaces w ON w.id = r.workspace_id",
        )
        .map_err(|err| err.to_string())?;
    let relationships = relationship_statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|err| err.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| err.to_string())?;
    for (source, destination) in relationships {
        connected.insert(source);
        connected.insert(destination);
    }
    // An element that holds children is attached: it is reached from above by descent, which is
    // how a top-level Software System is placed when no relationship names it.
    for (_, _, _, parent) in &elements {
        if let Some(parent) = parent {
            connected.insert(parent.clone());
        }
    }

    Ok(Model {
        elements,
        bindings,
        connected,
    })
}

#[cfg(test)]
pub fn scan_and_record(
    db: &Connection,
    project_id: i64,
    project_folder: &Path,
) -> Result<DesignHealthResult, String> {
    let result = scan(project_folder, collect_model(db)?)?;
    record(db, project_id, &result)?;
    Ok(result)
}

#[cfg(test)]
pub(crate) fn record(
    db: &Connection,
    project_id: i64,
    result: &DesignHealthResult,
) -> Result<(), String> {
    let tx = db.unchecked_transaction().map_err(|err| err.to_string())?;
    tx.execute(
        "DELETE FROM design_binding_checks WHERE project_id = ?1",
        params![project_id],
    )
    .map_err(|err| err.to_string())?;
    tx.execute(
        "DELETE FROM design_element_checks WHERE project_id = ?1",
        params![project_id],
    )
    .map_err(|err| err.to_string())?;

    for finding in &result.elements {
        for binding in &finding.broken {
            tx.execute(
                "INSERT INTO design_binding_checks(project_id, design_external_id, target_type, target, state, detail)
                 VALUES(?1, ?2, ?3, ?4, 'broken', ?5)",
                params![
                    project_id,
                    binding.design_external_id,
                    binding.target_type,
                    binding.target,
                    binding.detail
                ],
            )
            .map_err(|err| err.to_string())?;
        }
        tx.execute(
            "INSERT INTO design_element_checks(project_id, design_external_id, state, detail, files, checked_at)
             VALUES(?1, ?2, ?3, ?4, ?5, CURRENT_TIMESTAMP)",
            params![
                project_id,
                finding.design_external_id,
                finding.state.as_str(),
                finding.detail,
                serde_json::to_string(&finding.files).unwrap_or_else(|_| "[]".to_string())
            ],
        )
        .map_err(|err| err.to_string())?;
    }
    tx.commit().map_err(|err| err.to_string())
}

#[cfg(test)]
pub fn recorded_states(
    db: &Connection,
    project_id: i64,
) -> Result<HashMap<String, (ElementHealth, Vec<String>)>, String> {
    let mut statement = db
        .prepare(
            "SELECT design_external_id, state, files
             FROM design_element_checks WHERE project_id=?1",
        )
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map(params![project_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|err| err.to_string())?;
    let mut states = HashMap::new();
    for row in rows {
        let (external_id, state, files) = row.map_err(|err| err.to_string())?;
        if let Some(state) = ElementHealth::parse(&state) {
            let files = serde_json::from_str::<Vec<String>>(&files).unwrap_or_default();
            states.insert(external_id, (state, files));
        }
    }
    Ok(states)
}

#[cfg(test)]
pub fn recorded_counts(db: &Connection, project_id: i64) -> Result<HealthCounts, String> {
    let mut statement = db
        .prepare(
            "SELECT state, COUNT(*) FROM design_element_checks WHERE project_id=?1 GROUP BY state",
        )
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map(params![project_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|err| err.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| err.to_string())?;

    let mut counts = HealthCounts::default();
    for (state, count) in rows {
        let count = count as u32;
        match state.as_str() {
            "orphaned" => counts.orphaned = count,
            "unmapped" => counts.unmapped = count,
            "broken" => counts.broken = count,
            "resolved" => counts.resolved = count,
            _ => {}
        }
        counts.elements += count;
    }
    counts.broken_bindings = db
        .query_row(
            "SELECT COUNT(*) FROM design_binding_checks WHERE project_id=?1 AND state='broken'",
            params![project_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|err| err.to_string())? as u32;
    counts.waivers = db
        .query_row(
            "SELECT COUNT(*) FROM design_health_waivers WHERE project_id=?1",
            params![project_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|err| err.to_string())? as u32;
    Ok(counts)
}

#[cfg(test)]
pub fn recorded_state(
    db: &Connection,
    project_id: i64,
    external_id: &str,
) -> Result<Option<ElementHealth>, String> {
    let state: Option<String> = db
        .query_row(
            "SELECT state FROM design_element_checks WHERE project_id=?1 AND design_external_id=?2",
            params![project_id, external_id],
            |row| row.get(0),
        )
        .ok();
    Ok(state.and_then(|state| ElementHealth::parse(&state)))
}

#[cfg(test)]
pub fn recorded_state_with_files(
    db: &Connection,
    project_id: i64,
    external_id: &str,
) -> Result<Option<(ElementHealth, Vec<String>)>, String> {
    Ok(recorded_states(db, project_id)?.remove(external_id))
}

pub fn load_waivers(
    db: &Connection,
    project_id: i64,
    external_id: &str,
) -> Result<Vec<HealthWaiver>, String> {
    let mut statement = db
        .prepare(
            "SELECT id, state, reason, task_id, created_by, created_at
             FROM design_health_waivers
             WHERE project_id=?1 AND design_external_id=?2
             ORDER BY id DESC",
        )
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map(params![project_id, external_id], |row| {
            Ok(HealthWaiver {
                id: row.get(0)?,
                state: row.get(1)?,
                reason: row.get(2)?,
                task_id: row.get(3)?,
                created_by: row.get(4)?,
                created_at: row.get(5)?,
            })
        })
        .map_err(|err| err.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| err.to_string())
}

pub fn record_waiver(
    db: &Connection,
    project_id: i64,
    external_id: &str,
    state: ElementHealth,
    reason: &str,
    task_id: Option<i64>,
) -> Result<i64, String> {
    let reason = reason.trim();
    if reason.is_empty() {
        return Err("A kept finding needs a reason. Without one it is a silence.".to_string());
    }
    db.execute(
        "INSERT INTO design_health_waivers(project_id, design_external_id, state, reason, task_id)
         VALUES(?1, ?2, ?3, ?4, ?5)",
        params![project_id, external_id, state.as_str(), reason, task_id],
    )
    .map_err(|err| err.to_string())?;
    Ok(db.last_insert_rowid())
}
