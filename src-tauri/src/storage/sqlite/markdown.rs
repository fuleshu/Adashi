//! Project-scoped Markdown persistence. Caller owns the transaction and guards.
use adashi_storage_api::{markdown::*, StorageError, StorageResult};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::BTreeMap;

fn links(db: &Connection, id: i64) -> Result<Vec<DesignAssociation>, String> {
    Ok(links_for(db, &[id])?.remove(&id).unwrap_or_default())
}

/// Hydrate associations for a metadata page in one indexed query.
fn links_for(
    db: &Connection,
    ids: &[i64],
) -> Result<BTreeMap<i64, Vec<DesignAssociation>>, String> {
    let ids = serde_json::to_string(ids).map_err(|e| e.to_string())?;
    let mut stmt = db.prepare("SELECT document_id,target_type,design_external_id FROM markdown_design_links WHERE document_id IN (SELECT value FROM json_each(?1)) ORDER BY document_id,sort_order").map_err(|e|e.to_string())?;
    let rows = stmt
        .query_map([ids], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut result = BTreeMap::<i64, Vec<DesignAssociation>>::new();
    for row in rows {
        let (id, kind, design_external_id) = row.map_err(|e| e.to_string())?;
        result.entry(id).or_default().push(DesignAssociation {
            target_type: serde_json::from_value(serde_json::Value::String(kind))
                .map_err(|e| e.to_string())?,
            design_external_id,
        });
    }
    Ok(result)
}

pub fn get(
    db: &Connection,
    project: i64,
    external: &str,
) -> Result<Option<MarkdownDesignDocument>, String> {
    let row = db.query_row("SELECT id,title,body FROM markdown_design_documents WHERE project_id=?1 AND external_id=?2", params![project,external], |r| Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?))).optional().map_err(|e|e.to_string())?;
    row.map(|(id, title, body)| {
        Ok(MarkdownDesignDocument {
            external_id: external.into(),
            title,
            body,
            design_links: links(db, id)?,
        })
    })
    .transpose()
}

pub fn upsert(
    db: &Connection,
    project: i64,
    document: &MarkdownDesignDocument,
) -> Result<bool, String> {
    document.validate().map_err(|e| e.to_string())?;
    let current = get(db, project, &document.external_id)?;
    if current.as_ref() == Some(document) {
        return Ok(false);
    }
    // UPDATE existing rows explicitly: semantic no-ops never consume identities.
    if current.is_some() {
        db.execute("UPDATE markdown_design_documents SET title=?3,body=?4 WHERE project_id=?1 AND external_id=?2",params![project,document.external_id,document.title,document.body]).map_err(|e|e.to_string())?;
    } else {
        let collision:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM c4_elements e JOIN design_workspaces w ON w.id=e.workspace_id WHERE w.project_id=?1 AND e.external_id=?2 UNION ALL SELECT 1 FROM c4_relationships r JOIN design_workspaces w ON w.id=r.workspace_id WHERE w.project_id=?1 AND r.external_id=?2 UNION ALL SELECT 1 FROM diagrams d JOIN design_workspaces w ON w.id=d.workspace_id WHERE w.project_id=?1 AND d.key=?2 UNION ALL SELECT 1 FROM ui_mockups WHERE project_id=?1 AND external_id=?2)",params![project,document.external_id],|r|r.get(0)).map_err(|e|e.to_string())?;
        if collision {
            return Err("Markdown identity is already used by another design artefact".into());
        }
        db.execute("INSERT INTO markdown_design_documents(project_id,external_id,title,body) VALUES(?1,?2,?3,?4)",params![project,document.external_id,document.title,document.body]).map_err(|e|e.to_string())?;
    }
    let id: i64 = db
        .query_row(
            "SELECT id FROM markdown_design_documents WHERE project_id=?1 AND external_id=?2",
            params![project, document.external_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if current.as_ref().map(|d| &d.design_links) != Some(&document.design_links) {
        db.execute(
            "DELETE FROM markdown_design_links WHERE document_id=?1",
            [id],
        )
        .map_err(|e| e.to_string())?;
        let mut insert=db.prepare("INSERT INTO markdown_design_links(document_id,sort_order,target_type,design_external_id) VALUES(?1,?2,?3,?4)").map_err(|e|e.to_string())?;
        for (order, link) in document.design_links.iter().enumerate() {
            let kind = serde_json::to_value(link.target_type).map_err(|e| e.to_string())?;
            insert
                .execute(params![
                    id,
                    order as i64,
                    kind.as_str(),
                    link.design_external_id
                ])
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(true)
}

/// Incoming links are checked against the final graph after the entire batch.
pub fn delete(db: &Connection, project: i64, id: &str) -> Result<bool, String> {
    let changed = db
        .execute(
            "DELETE FROM markdown_design_documents WHERE project_id=?1 AND external_id=?2",
            params![project, id],
        )
        .map_err(|e| e.to_string())?;
    if changed == 0 {
        return Err(format!("Unknown Markdown document: {id}"));
    }
    Ok(true)
}

pub fn list(db: &Connection, project: i64, query: &MarkdownQuery) -> StorageResult<MarkdownPage> {
    query.validate()?;
    let kind = query
        .linked_to
        .as_ref()
        .map(|l| serde_json::to_value(l.target_type).unwrap());
    let ids = serde_json::to_string(&query.ids).map_err(StorageError::backend)?;
    let mut stmt=db.prepare("SELECT d.id,d.external_id,d.title,d.body,COUNT(*) OVER()
        FROM markdown_design_documents d WHERE d.project_id=?1
        AND (?2 IS NULL OR d.external_id>?2)
        AND (?3='[]' OR d.external_id IN (SELECT value FROM json_each(?3)))
        AND (?4 IS NULL OR instr(lower(d.title),lower(?4))>0 OR instr(lower(d.body),lower(?4))>0)
        AND (?5 IS NULL OR EXISTS(SELECT 1 FROM markdown_design_links l WHERE l.document_id=d.id AND l.target_type=?5 AND l.design_external_id=?6))
        AND (?7 IS NULL OR EXISTS(SELECT 1 FROM design_bindings b JOIN design_workspaces w ON w.id=b.workspace_id WHERE w.project_id=d.project_id AND b.design_external_id=d.external_id AND b.target_type='file' AND b.target=?7))
        AND (?8 IS NULL OR EXISTS(SELECT 1 FROM design_bindings b JOIN design_workspaces w ON w.id=b.workspace_id WHERE w.project_id=d.project_id AND b.design_external_id=d.external_id AND b.target_type='symbol' AND b.target=?8))
        ORDER BY d.external_id LIMIT ?9").map_err(StorageError::backend)?;
    let rows = stmt
        .query_map(
            params![
                project,
                query.after_id,
                ids,
                query.query,
                kind.as_ref().and_then(|k| k.as_str()),
                query.linked_to.as_ref().map(|l| &l.design_external_id),
                query.file,
                query.symbol,
                query.limit + 1
            ],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, usize>(4)?,
                ))
            },
        )
        .map_err(StorageError::backend)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(StorageError::backend)?;
    let more = rows.len() > query.limit as usize;
    let total_count = rows.first().map(|r| r.4).unwrap_or(0);
    let mut associations = links_for(
        db,
        &rows
            .iter()
            .take(query.limit as usize)
            .map(|r| r.0)
            .collect::<Vec<_>>(),
    )
    .map_err(StorageError::backend)?;
    let documents = rows
        .into_iter()
        .take(query.limit as usize)
        .map(|(id, external_id, title, body, _)| {
            MarkdownDesignDocument {
                external_id,
                title,
                body,
                design_links: associations.remove(&id).unwrap_or_default(),
            }
            .summary()
        })
        .collect::<StorageResult<Vec<_>>>()?;
    let next_after_id = if more {
        documents.last().map(|d| d.external_id.clone())
    } else {
        None
    };
    Ok(MarkdownPage {
        total_count,
        documents,
        next_after_id,
    })
}

/// Internal inventory traversal; public discovery always uses bounded pages.
pub fn all_summaries(db: &Connection, project: i64) -> Result<Vec<MarkdownSummary>, String> {
    let mut query = MarkdownQuery { limit: 100, ..Default::default() };
    let mut result = Vec::new();
    loop {
        let page = list(db, project, &query).map_err(|e| e.to_string())?;
        result.extend(page.documents);
        query.after_id = page.next_after_id;
        if query.after_id.is_none() { return Ok(result); }
    }
}

/// Batch complete-document reads keep explicit retrieval and grep free of N+1 queries.
pub fn get_many(db: &Connection, project: i64, external_ids: &[String]) -> Result<Vec<MarkdownDesignDocument>, String> {
    let ids = serde_json::to_string(external_ids).map_err(|e| e.to_string())?;
    let mut stmt = db.prepare("SELECT id,external_id,title,body FROM markdown_design_documents WHERE project_id=?1 AND external_id IN (SELECT value FROM json_each(?2)) ORDER BY external_id").map_err(|e| e.to_string())?;
    let rows = stmt.query_map(params![project,ids], |r| Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?))).map_err(|e|e.to_string())?.collect::<rusqlite::Result<Vec<_>>>().map_err(|e|e.to_string())?;
    let mut associations = links_for(db, &rows.iter().map(|r|r.0).collect::<Vec<_>>())?;
    Ok(rows.into_iter().map(|(id,external_id,title,body)| MarkdownDesignDocument { external_id,title,body,design_links: associations.remove(&id).unwrap_or_default() }).collect())
}

pub fn backlinks(db: &Connection, project: i64, id: &str) -> StorageResult<Vec<MarkdownBacklink>> {
    let mut stmt=db.prepare("SELECT 'markdown',d.external_id,d.title FROM markdown_design_links l JOIN markdown_design_documents d ON d.id=l.document_id WHERE d.project_id=?1 AND l.target_type='markdown' AND l.design_external_id=?2 UNION ALL SELECT 'task',CAST(t.id AS TEXT),t.title FROM task_design_specification_links l JOIN agent_tasks t ON t.id=l.task_id WHERE t.project_id=?1 AND l.target_type='markdown' AND l.design_external_id=?2 UNION ALL SELECT 'qa.job',CAST(j.id AS TEXT),j.name FROM qa_job_design_links l JOIN qa_jobs j ON j.id=l.qa_job_id WHERE j.project_id=?1 AND l.target_type='markdown' AND l.design_external_id=?2 UNION ALL SELECT 'binding',b.target_type||':'||b.target,b.target FROM design_bindings b JOIN design_workspaces w ON w.id=b.workspace_id WHERE w.project_id=?1 AND b.design_external_id=?2 ORDER BY 1,2").map_err(StorageError::backend)?;
    let result = stmt
        .query_map(params![project, id], |r| {
            Ok(MarkdownBacklink {
                source_kind: r.get(0)?,
                source_id: r.get(1)?,
                title: r.get(2)?,
            })
        })
        .map_err(StorageError::backend)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(StorageError::backend);
    result
}
