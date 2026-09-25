//! Markdown discovery is bounded and one hop; editable bodies are explicit documents.
use super::*;
use adashi_storage_api::markdown::{MarkdownPage, MarkdownQuery};
use super::super::markdown;

pub(super) fn page(db: &Connection, project: i64, ids: Option<&HashSet<&str>>) -> Result<MarkdownPage, String> {
    if ids.is_some_and(HashSet::is_empty) {
        return Ok(MarkdownPage { documents: vec![], next_after_id: None, total_count: 0 });
    }
    markdown::list(db, project, &MarkdownQuery {
        ids: ids.map(|ids| ids.iter().map(|s|s.to_string()).collect()).unwrap_or_default(),
        limit: 100, ..Default::default()
    }).map_err(|e|e.to_string())
}

pub(super) fn associated(db: &Connection, project: i64, scope: &HashSet<String>) -> Result<MarkdownPage, String> {
    let scope = serde_json::to_string(scope).map_err(|e|e.to_string())?;
    let mut stmt = db.prepare("SELECT DISTINCT d.external_id FROM markdown_design_documents d JOIN markdown_design_links l ON l.document_id=d.id WHERE d.project_id=?1 AND l.design_external_id IN (SELECT value FROM json_each(?2)) ORDER BY d.external_id").map_err(|e|e.to_string())?;
    let ids = stmt.query_map(params![project,scope], |r|r.get::<_,String>(0)).map_err(|e|e.to_string())?.collect::<rusqlite::Result<Vec<_>>>().map_err(|e|e.to_string())?;
    page(db, project, Some(&ids.iter().map(String::as_str).collect()))
}

pub(super) fn scope(db: &Connection, project: i64, id: &str) -> Result<Option<DesignScopeResult>, String> {
    let Some(document) = markdown::get(db, project, id)? else { return Ok(None) };
    // Explicit outgoing targets are metadata only; never recursively follow Markdown links.
    let ids = document.design_links.iter().map(|l|l.design_external_id.clone()).collect::<Vec<_>>();
    let linked = load_by_ids(db, project, &ids)?;
    let bindings = load_bindings(db, load_workspace(db)?.id)?.into_iter().filter(|b|b.design_external_id == id).collect();
    Ok(Some(DesignScopeResult {
        revision: state::load_project_revision(db,project)?.revision,
        root_external_id:id.into(), uml_artifact_types:supported_uml_artifact_types(),
        ancestors:vec![], elements:linked.elements, relationships:linked.relationships,
        diagrams:linked.diagrams, mockups:linked.mockups, bindings,
        markdown:MarkdownPage { documents:vec![document.summary().map_err(|e|e.to_string())?],next_after_id:None,total_count:1 },
        backlinks:markdown::backlinks(db,project,id).map_err(|e|e.to_string())?,
        structurizr_dsl:None,
    }))
}

pub(super) fn search(db: &Connection, project: i64, terms: &[String]) -> Result<Vec<DesignSearchHit>, String> {
    let mut stmt = db.prepare("SELECT external_id,title,body FROM markdown_design_documents WHERE project_id=?1 ORDER BY external_id").map_err(|e|e.to_string())?;
    let rows = stmt.query_map([project], |r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?))).map_err(|e|e.to_string())?;
    let mut hits = Vec::new();
    for row in rows {
        let (id,title,body) = row.map_err(|e|e.to_string())?;
        if matches_terms(&format!("{id} {title} {body}").to_lowercase(), terms) {
            // Show a matching line, even when the match is deep inside a long document.
            let line = body.lines().find(|line|terms.iter().any(|t|line.to_lowercase().contains(t))).unwrap_or(&body);
            let chars = line.chars().collect::<Vec<_>>();
            let start = terms.iter().filter_map(|t|line.to_lowercase().find(t)).min().map(|byte|line.to_lowercase()[..byte].chars().count().saturating_sub(60)).unwrap_or(0);
            let summary = chars.into_iter().skip(start).take(200).collect::<String>();
            hits.push(DesignSearchHit { kind:"markdown".into(),id,title,summary });
        }
    }
    Ok(hits)
}
