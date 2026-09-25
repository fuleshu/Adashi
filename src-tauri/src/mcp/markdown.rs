//! Typed discovery and response contracts shared by published operation help.
use super::*;
use adashi_storage_api::{documents::DesignDocument, markdown::{MarkdownPage, MarkdownQuery}};

#[derive(Deserialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ListMarkdownParams {
    pub project_name: String,
    pub markdown_query: Option<MarkdownQuery>,
}

#[derive(rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct MarkdownList { markdown: MarkdownPage, guidance: String }
#[derive(rmcp::schemars::JsonSchema)]
struct Documents { documents: Vec<DesignDocument> }
#[derive(rmcp::schemars::JsonSchema)]
struct Scope { #[serde(flatten)] scope: DesignScopeResult, documents: Vec<DesignDocument> }
#[derive(rmcp::schemars::JsonSchema)]
struct ByIds { #[serde(flatten)] result: design::DesignByIdsResult, documents: Vec<DesignDocument> }
#[derive(rmcp::schemars::JsonSchema)]
struct Bindings { #[serde(flatten)] result: design::DesignBindingsResult, documents: Vec<DesignDocument> }
#[derive(rmcp::schemars::JsonSchema)]
struct Saved { #[serde(flatten)] result: DesignSaveResult, projection: Option<Value> }

pub(super) fn refresh(server:&AdashiMcpServer, project:&ProjectSettings, store:&mut ProjectStore)->Value {
    let mut refresh = || -> Result<Value,String> {
        let settings=server.load_settings().map_err(|e|e.to_string())?;
        let enabled=settings::architecture_projection_enabled(&settings,&project.id);
        if !enabled {return Ok(json!({"state":"disabled"}));}
        let snapshot=store.snapshot().map_err(|e|e.to_string())?;
        let directory=crate::projection::markdown_directory(&settings,&project.id);
        crate::projection::regenerate_configured(snapshot.as_ref(),std::path::Path::new(&project.folder),&settings::architecture_file_name(&settings,&project.id),true,&directory)?;
        Ok(json!({"state":"current"}))
    };
    match refresh() {Ok(status)=>status,Err(error)=>json!({"state":"error","error":error,"guidance":"Canonical save succeeded. Retry projection regeneration from desktop settings."})}
}

pub(super) fn response_schema(operation: &str) -> Option<Value> {
    fn schema<T: rmcp::schemars::JsonSchema>() -> Value { serde_json::to_value(rmcp::schemars::schema_for!(T)).expect("response schema") }
    Some(match operation {
        "list_markdown" => schema::<MarkdownList>(),
        "get_documents" => schema::<Documents>(),
        "get_scope" => schema::<Scope>(),
        "get_by_ids" => schema::<ByIds>(),
        "get_bindings" => schema::<Bindings>(),
        "get_overview" => schema::<DesignOverviewResult>(),
        "search" => schema::<DesignSearchResult>(),
        "save" => schema::<Saved>(),
        "set_element_descriptions" => schema::<DesignSaveResult>(),
        _ => return None,
    })
}
