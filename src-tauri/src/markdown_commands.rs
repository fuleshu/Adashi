//! Narrow desktop Markdown API. Both clients use the same guarded storage mutations.
use super::{resolve_project, AppState};
use adashi_storage_api::{*, design::{DesignChange,DesignSaveResult}, documents::{DesignDocument,DocumentReadToken}, markdown::*};
use crate::{projection, settings::{self,AppSettings,ProjectSettings},storage::ProjectStore};
use serde::{Deserialize,Serialize};
use tauri::State;

#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub(super) struct DocumentView { document:DesignDocument, backlinks:Vec<MarkdownBacklink> }

#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(super) struct SaveInput { project_id:String, operation_id:String, read_token:Option<String>, document:MarkdownDesignDocument, import_source:Option<super::markdown_import::ImportSource> }
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(super) struct DeleteInput { project_id:String,operation_id:String,external_id:String,read_token:String }

#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub(super) struct SavedDocument {
    saved:DesignSaveResult,
    document:Option<DesignDocument>,
    refresh_error:Option<String>,
    projection_error:Option<String>,
    source_path:Option<String>,
}

#[tauri::command]
pub(super) async fn list_markdown_documents(state:State<'_,AppState>,project_id:String,query:Option<MarkdownQuery>)->Result<MarkdownPage,String> {
    let project=resolve_project(&state,Some(&project_id))?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut store=ProjectStore::open(&project).map_err(|e|e.to_string())?;
        let s=store.snapshot().map_err(|e|e.to_string())?;
        s.markdown_documents(&query.unwrap_or_default()).map_err(|e|e.to_string())
    }).await.map_err(|e|e.to_string())?
}

#[tauri::command]
pub(super) async fn get_markdown_document(state:State<'_,AppState>,project_id:String,external_id:String)->Result<DocumentView,String> {
    let project=resolve_project(&state,Some(&project_id))?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut store=ProjectStore::open(&project).map_err(|e|e.to_string())?;
        let s=store.snapshot().map_err(|e|e.to_string())?;
        let document=s.design_documents(&[format!("markdown:{external_id}")]).map_err(|e|e.to_string())?.remove(0);
        Ok(DocumentView {document,backlinks:s.markdown_backlinks(&external_id).map_err(|e|e.to_string())?})
    }).await.map_err(|e|e.to_string())?
}

/// Errors after commit are returned alongside success, never as an ambiguous failed write.
fn commit(project:ProjectSettings,settings:AppSettings,operation_id:String,id:String,changes:Vec<DesignChange>,tokens:Vec<DocumentReadToken>)->Result<SavedDocument,String> {
    let mut store=ProjectStore::open(&project).map_err(|e|e.to_string())?;
    let result=store.commit(Mutation {operation_id,changes:vec![Change::Design(DesignWrite::Save {change_intent:"Edit Markdown design from desktop".into(),changes,read_tokens:tokens})]}).map_err(|e|e.to_string())?;
    let Some(ChangeOutcome::Design(saved))=result.outcomes.into_iter().next() else {return Err("Unexpected Markdown save result".into())};
    let mut response=SavedDocument {saved,document:None,refresh_error:None,projection_error:None,source_path:None};
    match store.snapshot() {
        Err(error)=>response.refresh_error=Some(error.to_string()),
        Ok(s)=> {
            match s.design_documents(&[format!("markdown:{id}")]) {Ok(mut docs)=>response.document=docs.pop(),Err(e)=>response.refresh_error=Some(e.to_string())}
            if settings::architecture_projection_enabled(&settings,&project.id) {
                response.projection_error=projection::regenerate_configured(s.as_ref(),std::path::Path::new(&project.folder),&settings::architecture_file_name(&settings,&project.id),true,&projection::markdown_directory(&settings,&project.id)).err();
            }
        }
    }
    Ok(response)
}

#[tauri::command]
pub(super) async fn save_markdown_document(state:State<'_,AppState>,input:SaveInput)->Result<SavedDocument,String> {
    let project=resolve_project(&state,Some(&input.project_id))?;
    let settings=state.settings.lock().map_err(|_|"Settings lock poisoned")?.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let d=input.document;
        if let Some(source)=&input.import_source {
            if input.read_token.is_some() { return Err("Import creates a new document; edit an existing document through its normal editor.".into()); }
            super::markdown_import::verify(source)?;
        }
        let tokens=input.read_token.map(|read_token|vec![DocumentReadToken {document_id:format!("markdown:{}",d.external_id),read_token}]).unwrap_or_default();
        let mut result=commit(project,settings,input.operation_id,d.external_id.clone(),vec![DesignChange::UpsertMarkdown {external_id:d.external_id,title:d.title,body:d.body,design_links:d.design_links}],tokens)?;
        result.source_path=input.import_source.map(|s|s.source_path);
        Ok(result)
    }).await.map_err(|e|e.to_string())?
}

#[tauri::command]
pub(super) async fn delete_markdown_document(state:State<'_,AppState>,input:DeleteInput)->Result<SavedDocument,String> {
    let project=resolve_project(&state,Some(&input.project_id))?;
    let settings=state.settings.lock().map_err(|_|"Settings lock poisoned")?.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let token=DocumentReadToken {document_id:format!("markdown:{}",input.external_id),read_token:input.read_token};
        commit(project,settings,input.operation_id,input.external_id.clone(),vec![DesignChange::DeleteMarkdown {external_id:input.external_id}],vec![token])
    }).await.map_err(|e|e.to_string())?
}
