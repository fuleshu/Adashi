use super::*;
use adashi_storage_api::{check_references, MissingReference};
use std::collections::BTreeSet;

/// Validate polymorphic links as well as SQL foreign keys against the final
/// staged graph. SQLite foreign keys alone cannot enforce design identity kinds.
pub(crate) fn validate(db: &Connection, project: i64) -> StorageResult<()> {
    let mut resources = BTreeSet::new();
    let mut add=db.prepare("SELECT 'element',e.external_id FROM c4_elements e JOIN design_workspaces w ON w.id=e.workspace_id WHERE w.project_id=?1 UNION ALL SELECT 'relationship',r.external_id FROM c4_relationships r JOIN design_workspaces w ON w.id=r.workspace_id WHERE w.project_id=?1 UNION ALL SELECT 'uml',d.key FROM diagrams d JOIN design_workspaces w ON w.id=d.workspace_id WHERE w.project_id=?1 AND d.kind='mermaid' UNION ALL SELECT 'mockup',external_id FROM ui_mockups WHERE project_id=?1 UNION ALL SELECT 'task',CAST(id AS TEXT) FROM agent_tasks WHERE project_id=?1").map_err(StorageError::backend)?;
    for item in add
        .query_map([project], |r| {
            Ok(ResourceKey {
                kind: r.get(0)?,
                id: r.get(1)?,
            })
        })
        .map_err(StorageError::backend)?
    {
        resources.insert(item.map_err(StorageError::backend)?);
    }
    let mut edges = Vec::new();
    for sql in [
        "SELECT 'task',CAST(t.id AS TEXT),l.target_type,l.design_external_id FROM task_design_specification_links l JOIN agent_tasks t ON t.id=l.task_id WHERE t.project_id=?1",
        "SELECT 'qa.job',CAST(j.id AS TEXT),l.target_type,l.design_external_id FROM qa_job_design_links l JOIN qa_jobs j ON j.id=l.qa_job_id WHERE j.project_id=?1",
        "SELECT 'qa.job',CAST(j.id AS TEXT),'task',CAST(l.task_id AS TEXT) FROM qa_job_task_links l JOIN qa_jobs j ON j.id=l.qa_job_id WHERE j.project_id=?1",
        "SELECT 'mockup',external_id,'element',attached_to_external_id FROM ui_mockups WHERE project_id=?1",
    ] {
        let mut stmt=db.prepare(sql).map_err(StorageError::backend)?;
        for row in stmt.query_map([project],|r|Ok(MissingReference {source:ResourceKey{kind:r.get(0)?,id:r.get(1)?},target:ResourceKey{kind:r.get(2)?,id:r.get(3)?}})).map_err(StorageError::backend)? { edges.push(row.map_err(StorageError::backend)?); }
    }
    check_references(&edges, &resources)
}
