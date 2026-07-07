use crate::design::{
    self, DesignBindingsResult, DesignByIdsResult, DesignChange, DesignOverviewResult,
    DesignSaveResult, DesignScopeResult, DesignSearchResult,
};
use crate::memory::{self, ProjectMemory};
use crate::rules::{self, InjectionRule, NewRule, Rule};
use crate::settings::{self, AppSettings, ProjectSettings};
use crate::state as project_state;
use crate::tasks::{self, NewTask, Task, UpdateTask};
use crate::{open_project_database, resolve_project_from_settings};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::model::ErrorData;
use rmcp::transport::stdio;
use rmcp::{serve_server, tool, tool_router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;

#[derive(Clone)]
pub struct AdashiMcpServer {
    settings_path: PathBuf,
}

impl AdashiMcpServer {
    pub fn new(settings_path: PathBuf) -> Self {
        Self { settings_path }
    }

    fn load_settings(&self) -> Result<AppSettings, ErrorData> {
        settings::load_or_init(&self.settings_path).map_err(internal_error)
    }

    fn open_project(
        &self,
        project_id: Option<&str>,
    ) -> Result<(ProjectSettings, rusqlite::Connection), ErrorData> {
        let settings = self.load_settings()?;
        let project = resolve_project_from_settings(&settings, project_id)
            .map_err(|err| ErrorData::invalid_params(err, None))?;
        let db = open_project_database(&project).map_err(internal_error)?;
        Ok((project, db))
    }
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ProjectParams {
    project_id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct RuleInjectionParams {
    project_id: Option<String>,
    intend: String,
    hook: String,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct CreateRuleParams {
    project_id: Option<String>,
    name: String,
    enabled: bool,
    intend: String,
    hook: String,
    prompt: String,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct DeleteRuleParams {
    project_id: Option<String>,
    rule_id: i64,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct CreateTaskParams {
    project_id: Option<String>,
    title: String,
    body: Option<String>,
    status: Option<String>,
    priority: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct UpdateTaskParams {
    project_id: Option<String>,
    task_id: i64,
    title: Option<String>,
    body: Option<String>,
    status: Option<String>,
    priority: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct UpdateMemoryParams {
    project_id: Option<String>,
    memory: String,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct UpdateMemoryRuleParams {
    project_id: Option<String>,
    rule: String,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct DesignOverviewParams {
    project_id: Option<String>,
    max_depth: Option<usize>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct DesignScopeParams {
    project_id: Option<String>,
    element_id: String,
    include_ancestors: Option<bool>,
    children_depth: Option<usize>,
    include_source: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct DesignSearchParams {
    project_id: Option<String>,
    query: String,
    kinds: Option<Vec<String>>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct DesignByIdsParams {
    project_id: Option<String>,
    ids: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct DesignBindingsParams {
    project_id: Option<String>,
    files: Option<Vec<String>>,
    symbols: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct DesignSaveParams {
    project_id: Option<String>,
    expected_revision: i64,
    change_intent: String,
    changes: Vec<DesignChange>,
}

#[derive(Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct RuleListResult {
    project_id: String,
    project_name: String,
    rules: Vec<Rule>,
}

#[derive(Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct TaskListResult {
    project_id: String,
    project_name: String,
    revision: i64,
    tasks: Vec<Task>,
}

#[derive(Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct RuleInjectionResult {
    project_id: String,
    project_name: String,
    intend: String,
    hook: String,
    rules: Vec<InjectionRule>,
    #[serde(skip_serializing_if = "Option::is_none")]
    memory_rule: Option<String>,
    injection_prompt: String,
}

#[derive(Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct CreateRuleResult {
    project_id: String,
    rule_id: i64,
}

#[derive(Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct DeleteRuleResult {
    project_id: String,
    deleted_rule_id: i64,
    revision: i64,
}

#[derive(Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct TaskMutationResult {
    project_id: String,
    project_name: String,
    revision: i64,
    task: Task,
}

#[derive(Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct MemoryResult {
    project_id: String,
    project_name: String,
    revision: i64,
    memory: ProjectMemory,
}

#[tool_router(server_handler)]
impl AdashiMcpServer {
    #[tool(
        name = "adashi_list_rules",
        description = "List all Adashi rule prompts for a project."
    )]
    fn list_rules(
        &self,
        Parameters(params): Parameters<ProjectParams>,
    ) -> Result<Json<RuleListResult>, ErrorData> {
        let (project, db) = self.open_project(params.project_id.as_deref())?;
        let rules = rules::load_rules(&db).map_err(tool_error)?;
        Ok(Json(RuleListResult {
            project_id: project.id,
            project_name: project.name,
            rules,
        }))
    }

    #[tool(
        name = "adashi_get_rule_injections",
        description = "Get enabled Adashi rule prompts for an intend and lifecycle hook. Agents should call this at run.start, task.start, task.end, and run.end when this MCP server is present."
    )]
    fn get_rule_injections(
        &self,
        Parameters(params): Parameters<RuleInjectionParams>,
    ) -> Result<Json<RuleInjectionResult>, ErrorData> {
        let (project, db) = self.open_project(params.project_id.as_deref())?;
        let rules =
            rules::load_rule_injections(&db, &params.intend, &params.hook).map_err(tool_error)?;
        let memory_rule = if params.hook == "run.start" {
            let project_row_id = project_row_id(&db).map_err(tool_error)?;
            Some(
                memory::load_memory(&db, project_row_id)
                    .map_err(tool_error)?
                    .rule,
            )
        } else {
            None
        };
        let mut injection_parts = rules
            .iter()
            .map(|rule| rule.prompt.clone())
            .collect::<Vec<_>>();

        if let Some(rule) = memory_rule.as_deref() {
            if !rule.trim().is_empty() {
                injection_parts.push(rule.to_string());
            }
        }

        let injection_prompt = injection_parts.join("\n\n");

        Ok(Json(RuleInjectionResult {
            project_id: project.id,
            project_name: project.name,
            intend: params.intend,
            hook: params.hook,
            rules,
            memory_rule,
            injection_prompt,
        }))
    }

    #[tool(
        name = "adashi_list_tasks",
        description = "List Adashi tasks for a project, including task ids needed for update calls."
    )]
    fn list_tasks(
        &self,
        Parameters(params): Parameters<ProjectParams>,
    ) -> Result<Json<TaskListResult>, ErrorData> {
        let (project, db) = self.open_project(params.project_id.as_deref())?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let revision =
            project_state::load_project_revision(&db, project_row_id).map_err(tool_error)?;
        let tasks = tasks::load_tasks(&db).map_err(tool_error)?;

        Ok(Json(TaskListResult {
            project_id: project.id,
            project_name: project.name,
            revision: revision.revision,
            tasks,
        }))
    }

    #[tool(
        name = "adashi_get_memory",
        description = "Read the SQL-backed Adashi long-term memory protocol and current project memory. Agents should call this before starting any task when this MCP server is present."
    )]
    fn get_memory(
        &self,
        Parameters(params): Parameters<ProjectParams>,
    ) -> Result<Json<MemoryResult>, ErrorData> {
        let (project, db) = self.open_project(params.project_id.as_deref())?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let revision =
            project_state::load_project_revision(&db, project_row_id).map_err(tool_error)?;
        let memory = memory::load_memory(&db, project_row_id).map_err(tool_error)?;

        Ok(Json(MemoryResult {
            project_id: project.id,
            project_name: project.name,
            revision: revision.revision,
            memory,
        }))
    }

    #[tool(
        name = "adashi_create_rule",
        description = "Create an Adashi rule prompt for a project."
    )]
    fn create_rule(
        &self,
        Parameters(params): Parameters<CreateRuleParams>,
    ) -> Result<Json<CreateRuleResult>, ErrorData> {
        let (project, db) = self.open_project(params.project_id.as_deref())?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let rule_id = rules::create_rule(
            &db,
            project_row_id,
            NewRule {
                name: params.name,
                enabled: params.enabled,
                intend: params.intend,
                hook: params.hook,
                prompt: params.prompt,
            },
        )
        .map_err(tool_error)?;
        project_state::bump_project_revision(&db, project_row_id).map_err(tool_error)?;
        Ok(Json(CreateRuleResult {
            project_id: project.id,
            rule_id,
        }))
    }

    #[tool(
        name = "adashi_delete_rule",
        description = "Delete an Adashi rule prompt by id."
    )]
    fn delete_rule(
        &self,
        Parameters(params): Parameters<DeleteRuleParams>,
    ) -> Result<Json<DeleteRuleResult>, ErrorData> {
        let (project, db) = self.open_project(params.project_id.as_deref())?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        rules::delete_rule(&db, params.rule_id).map_err(tool_error)?;
        let revision =
            project_state::bump_project_revision(&db, project_row_id).map_err(tool_error)?;
        Ok(Json(DeleteRuleResult {
            project_id: project.id,
            deleted_rule_id: params.rule_id,
            revision: revision.revision,
        }))
    }

    #[tool(
        name = "adashi_create_task",
        description = "Create an Adashi task for the selected project. The write bumps the project revision so the desktop UI can merge the changed task list automatically."
    )]
    fn create_task(
        &self,
        Parameters(params): Parameters<CreateTaskParams>,
    ) -> Result<Json<TaskMutationResult>, ErrorData> {
        let (project, db) = self.open_project(params.project_id.as_deref())?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let task = tasks::create_task(
            &db,
            project_row_id,
            NewTask {
                title: params.title,
                body: params.body,
                status: params.status,
                priority: params.priority,
            },
        )
        .map_err(tool_error)?;
        let revision =
            project_state::bump_project_revision(&db, project_row_id).map_err(tool_error)?;

        Ok(Json(TaskMutationResult {
            project_id: project.id,
            project_name: project.name,
            revision: revision.revision,
            task,
        }))
    }

    #[tool(
        name = "adashi_update_task",
        description = "Update selected fields of an Adashi task. Omitted fields keep their current values. The write bumps the project revision so the desktop UI can merge changed fields automatically."
    )]
    fn update_task(
        &self,
        Parameters(params): Parameters<UpdateTaskParams>,
    ) -> Result<Json<TaskMutationResult>, ErrorData> {
        let (project, db) = self.open_project(params.project_id.as_deref())?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let task = tasks::update_task(
            &db,
            UpdateTask {
                task_id: params.task_id,
                title: params.title,
                body: params.body,
                status: params.status,
                priority: params.priority,
            },
        )
        .map_err(tool_error)?;
        let revision =
            project_state::bump_project_revision(&db, project_row_id).map_err(tool_error)?;

        Ok(Json(TaskMutationResult {
            project_id: project.id,
            project_name: project.name,
            revision: revision.revision,
            task,
        }))
    }

    #[tool(
        name = "adashi_update_memory",
        description = "Replace the current SQL-backed Adashi project memory after a successful task or major discussion. The write bumps the project revision so the desktop UI can merge the changed memory automatically."
    )]
    fn update_memory(
        &self,
        Parameters(params): Parameters<UpdateMemoryParams>,
    ) -> Result<Json<MemoryResult>, ErrorData> {
        let (project, db) = self.open_project(params.project_id.as_deref())?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let memory =
            memory::update_memory(&db, project_row_id, params.memory).map_err(tool_error)?;
        let revision =
            project_state::bump_project_revision(&db, project_row_id).map_err(tool_error)?;

        Ok(Json(MemoryResult {
            project_id: project.id,
            project_name: project.name,
            revision: revision.revision,
            memory,
        }))
    }

    #[tool(
        name = "adashi_update_memory_rule",
        description = "Replace the SQL-backed Adashi long-term memory protocol rule for a project. The write bumps the project revision so the desktop UI can merge the changed rule automatically."
    )]
    fn update_memory_rule(
        &self,
        Parameters(params): Parameters<UpdateMemoryRuleParams>,
    ) -> Result<Json<MemoryResult>, ErrorData> {
        let (project, db) = self.open_project(params.project_id.as_deref())?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let memory =
            memory::update_memory_rule(&db, project_row_id, params.rule).map_err(tool_error)?;
        let revision =
            project_state::bump_project_revision(&db, project_row_id).map_err(tool_error)?;

        Ok(Json(MemoryResult {
            project_id: project.id,
            project_name: project.name,
            revision: revision.revision,
            memory,
        }))
    }

    #[tool(
        name = "adashi_design_get_overview",
        description = "Read a compact top-down C4/UML design overview for the selected project. This is deterministic retrieval; the agent chooses the relevant design scope."
    )]
    fn design_get_overview(
        &self,
        Parameters(params): Parameters<DesignOverviewParams>,
    ) -> Result<Json<DesignOverviewResult>, ErrorData> {
        let (_project, db) = self.open_project(params.project_id.as_deref())?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let overview =
            design::load_overview(&db, project_row_id, params.max_depth).map_err(tool_error)?;
        Ok(Json(overview))
    }

    #[tool(
        name = "adashi_design_get_scope",
        description = "Read an explicit C4 branch by element id, optionally including ancestors, children, attached UML, bindings, relationships, and canonical source."
    )]
    fn design_get_scope(
        &self,
        Parameters(params): Parameters<DesignScopeParams>,
    ) -> Result<Json<DesignScopeResult>, ErrorData> {
        let (_project, db) = self.open_project(params.project_id.as_deref())?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let scope = design::load_scope(
            &db,
            project_row_id,
            &params.element_id,
            params.include_ancestors.unwrap_or(true),
            params.children_depth,
            params.include_source.unwrap_or(false),
        )
        .map_err(tool_error)?;
        Ok(Json(scope))
    }

    #[tool(
        name = "adashi_design_search",
        description = "Run deterministic text search over stored design elements, relationships, UML, and source. The agent must reason over the hits; the MCP does not infer task context."
    )]
    fn design_search(
        &self,
        Parameters(params): Parameters<DesignSearchParams>,
    ) -> Result<Json<DesignSearchResult>, ErrorData> {
        let (_project, db) = self.open_project(params.project_id.as_deref())?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let result = design::search(
            &db,
            project_row_id,
            &params.query,
            &params.kinds.unwrap_or_default(),
            params.limit.unwrap_or(20),
        )
        .map_err(tool_error)?;
        Ok(Json(result))
    }

    #[tool(
        name = "adashi_design_get_by_ids",
        description = "Read explicit design elements, relationships, UML artifacts, and bindings by stored design ids."
    )]
    fn design_get_by_ids(
        &self,
        Parameters(params): Parameters<DesignByIdsParams>,
    ) -> Result<Json<DesignByIdsResult>, ErrorData> {
        let (_project, db) = self.open_project(params.project_id.as_deref())?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let result = design::load_by_ids(&db, project_row_id, &params.ids).map_err(tool_error)?;
        Ok(Json(result))
    }

    #[tool(
        name = "adashi_design_get_bindings",
        description = "Read design artifacts explicitly bound to files or symbols. This is stored traceability, not task interpretation."
    )]
    fn design_get_bindings(
        &self,
        Parameters(params): Parameters<DesignBindingsParams>,
    ) -> Result<Json<DesignBindingsResult>, ErrorData> {
        let (_project, db) = self.open_project(params.project_id.as_deref())?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let result = design::load_by_bindings(
            &db,
            project_row_id,
            &params.files.unwrap_or_default(),
            &params.symbols.unwrap_or_default(),
        )
        .map_err(tool_error)?;
        Ok(Json(result))
    }

    #[tool(
        name = "adashi_design_save",
        description = "Transactionally save a formal C4/UML design changeset. The save validates revision, containment, relationships, UML syntax, attachments, and bindings; invalid input returns correction errors and is not stored."
    )]
    fn design_save(
        &self,
        Parameters(params): Parameters<DesignSaveParams>,
    ) -> Result<Json<DesignSaveResult>, ErrorData> {
        let (_project, mut db) = self.open_project(params.project_id.as_deref())?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let result = design::save_changes(
            &mut db,
            project_row_id,
            params.expected_revision,
            &params.change_intent,
            &params.changes,
        )
        .map_err(tool_error)?;
        Ok(Json(result))
    }
}

pub fn run_stdio_server() -> Result<(), Box<dyn std::error::Error>> {
    let settings_path = settings::settings_path();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    runtime.block_on(async move {
        let service = serve_server(AdashiMcpServer::new(settings_path), stdio()).await?;
        service.waiting().await?;
        Ok::<(), Box<dyn std::error::Error>>(())
    })
}

fn project_row_id(db: &rusqlite::Connection) -> Result<i64, String> {
    db.query_row("SELECT id FROM projects ORDER BY id LIMIT 1", [], |row| {
        row.get(0)
    })
    .map_err(|err| err.to_string())
}

fn tool_error(message: String) -> ErrorData {
    let value = json!({ "message": message });
    ErrorData::invalid_params("Adashi MCP request failed", Some(value))
}

fn internal_error(err: impl std::fmt::Display) -> ErrorData {
    let value = json!({ "message": err.to_string() });
    ErrorData::internal_error("Adashi MCP server failed", Some(value))
}
