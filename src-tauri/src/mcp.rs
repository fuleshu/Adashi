use crate::design::{
    self, DesignBindingsResult, DesignByIdsResult, DesignChange, DesignOverviewResult,
    DesignSaveResult, DesignScopeResult, DesignSearchResult,
};
use crate::fixed_hooks::{self, DESIGN_AUTHORING_HOOK_KEY, IMPLEMENTATION_GUIDANCE_HOOK_KEY};
use crate::memory::{self, ProjectMemory};
use crate::rules::{self, InjectionRule, NewRule, Rule, UpdateRule};
use crate::settings::{self, AppSettings, ProjectSettings};
use crate::state as project_state;
use crate::tasks::{
    self, FinishTask, NewTask, Task, TaskDesignSpecificationLink, TaskDesignSpecificationLinkInput,
    UpdateTask,
};
use crate::{open_project_database, resolve_project_from_settings};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::model::ErrorData;
use rmcp::transport::stdio;
use rmcp::{serve_server, tool, tool_router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::fmt::Write as _;
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
struct UpdateRuleParams {
    project_id: Option<String>,
    rule_id: i64,
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
    description: Option<String>,
    design_specification_links: Option<Vec<TaskDesignSpecificationLinkInput>>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct UpdateTaskParams {
    project_id: Option<String>,
    task_id: i64,
    title: Option<String>,
    description: Option<String>,
    state: Option<String>,
    design_specification_links: Option<Vec<TaskDesignSpecificationLinkInput>>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ListTasksParams {
    project_id: Option<String>,
    states: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct TaskIdParams {
    project_id: Option<String>,
    task_id: i64,
}

#[derive(Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct FinishTaskParams {
    project_id: Option<String>,
    task_id: i64,
    completion_memo: String,
    created_files: Option<Vec<String>>,
    changed_files: Option<Vec<String>>,
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
struct TaskReadResult {
    project_id: String,
    project_name: String,
    revision: i64,
    task: Task,
    design_specifications: Vec<TaskDesignSpecificationBranch>,
}

#[derive(Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct TaskDesignSpecificationBranch {
    link: TaskDesignSpecificationLink,
    scope: Option<DesignScopeResult>,
    note: Option<String>,
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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    generated_context: Vec<String>,
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
struct UpdateRuleResult {
    project_id: String,
    updated_rule_id: i64,
    revision: i64,
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
    design_specifications: Vec<TaskDesignSpecificationBranch>,
}

#[derive(Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct DeleteTaskResult {
    project_id: String,
    project_name: String,
    revision: i64,
    deleted_task_id: i64,
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
        description = "Get enabled optional rule prompts plus fixed generated lifecycle context for an intend and hook. run.start includes memory context, and design/implementation run.start includes Settings-managed fixed prompts plus compact formal-design context directly."
    )]
    fn get_rule_injections(
        &self,
        Parameters(params): Parameters<RuleInjectionParams>,
    ) -> Result<Json<RuleInjectionResult>, ErrorData> {
        let (project, db) = self.open_project(params.project_id.as_deref())?;
        let rules =
            rules::load_rule_injections(&db, &params.intend, &params.hook).map_err(tool_error)?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let mut injection_parts = rules
            .iter()
            .map(|rule| rule.prompt.clone())
            .collect::<Vec<_>>();
        let mut generated_context = Vec::new();

        let memory_rule = if params.hook == "run.start" {
            let memory = memory::load_memory(&db, project_row_id).map_err(tool_error)?;
            let memory_context = format_memory_run_start_context(&memory);
            if !memory_context.trim().is_empty() {
                injection_parts.push(memory_context.clone());
                generated_context.push(memory_context);
            }

            if matches!(params.intend.as_str(), "design" | "implementation") {
                if let Some(prompt) =
                    load_fixed_hook_prompt_for_injection(&db, project_row_id, &params.intend)?
                {
                    injection_parts.push(prompt);
                }

                let overview =
                    design::load_overview(&db, project_row_id, Some(3)).map_err(tool_error)?;
                let design_context = format_design_run_start_context(&params.intend, &overview);
                if !design_context.trim().is_empty() {
                    injection_parts.push(design_context.clone());
                    generated_context.push(design_context);
                }
            }

            Some(memory.rule)
        } else {
            None
        };

        let injection_prompt = injection_parts.join("\n\n");

        Ok(Json(RuleInjectionResult {
            project_id: project.id,
            project_name: project.name,
            intend: params.intend,
            hook: params.hook,
            rules,
            memory_rule,
            generated_context,
            injection_prompt,
        }))
    }

    #[tool(
        name = "adashi_list_tasks",
        description = "List project-local Adashi tasks. Pass states to filter to open, finished, and/or confirmed."
    )]
    fn list_tasks(
        &self,
        Parameters(params): Parameters<ListTasksParams>,
    ) -> Result<Json<TaskListResult>, ErrorData> {
        let (project, db) = self.open_project(params.project_id.as_deref())?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let revision =
            project_state::load_project_revision(&db, project_row_id).map_err(tool_error)?;
        let tasks =
            tasks::load_tasks(&db, project_row_id, params.states.as_deref()).map_err(tool_error)?;

        Ok(Json(TaskListResult {
            project_id: project.id,
            project_name: project.name,
            revision: revision.revision,
            tasks,
        }))
    }

    #[tool(
        name = "adashi_get_task",
        description = "Read one Adashi task and return all linked design specification branches when links are present."
    )]
    fn get_task(
        &self,
        Parameters(params): Parameters<TaskIdParams>,
    ) -> Result<Json<TaskReadResult>, ErrorData> {
        let (project, db) = self.open_project(params.project_id.as_deref())?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let revision =
            project_state::load_project_revision(&db, project_row_id).map_err(tool_error)?;
        let task = tasks::load_task(&db, project_row_id, params.task_id).map_err(tool_error)?;
        let design_specifications =
            load_task_design_specifications(&db, project_row_id, &task).map_err(tool_error)?;

        Ok(Json(TaskReadResult {
            project_id: project.id,
            project_name: project.name,
            revision: revision.revision,
            task,
            design_specifications,
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
        name = "adashi_update_rule",
        description = "Update an Adashi rule prompt by id. The write bumps the project revision so the desktop UI can merge the changed rule automatically."
    )]
    fn update_rule(
        &self,
        Parameters(params): Parameters<UpdateRuleParams>,
    ) -> Result<Json<UpdateRuleResult>, ErrorData> {
        let (project, db) = self.open_project(params.project_id.as_deref())?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        rules::update_rule(
            &db,
            UpdateRule {
                id: params.rule_id,
                name: params.name,
                enabled: params.enabled,
                intend: params.intend,
                hook: params.hook,
                prompt: params.prompt,
            },
        )
        .map_err(tool_error)?;
        let revision =
            project_state::bump_project_revision(&db, project_row_id).map_err(tool_error)?;
        Ok(Json(UpdateRuleResult {
            project_id: project.id,
            updated_rule_id: params.rule_id,
            revision: revision.revision,
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
        description = "Create a project-local Adashi task with optional ordered design specification links. The write bumps project revision."
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
                description: params.description,
                design_specification_links: params.design_specification_links,
            },
        )
        .map_err(tool_error)?;
        let revision =
            project_state::bump_project_revision(&db, project_row_id).map_err(tool_error)?;
        let design_specifications =
            load_task_design_specifications(&db, project_row_id, &task).map_err(tool_error)?;

        Ok(Json(TaskMutationResult {
            project_id: project.id,
            project_name: project.name,
            revision: revision.revision,
            task,
            design_specifications,
        }))
    }

    #[tool(
        name = "adashi_update_task",
        description = "Update selected fields of an Adashi task, including state and full ordered design specification link replacement. Omitted fields keep current values."
    )]
    fn update_task(
        &self,
        Parameters(params): Parameters<UpdateTaskParams>,
    ) -> Result<Json<TaskMutationResult>, ErrorData> {
        let (project, db) = self.open_project(params.project_id.as_deref())?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let task = tasks::update_task(
            &db,
            project_row_id,
            UpdateTask {
                task_id: params.task_id,
                title: params.title,
                description: params.description,
                state: params.state,
                design_specification_links: params.design_specification_links,
            },
        )
        .map_err(tool_error)?;
        let revision =
            project_state::bump_project_revision(&db, project_row_id).map_err(tool_error)?;
        let design_specifications =
            load_task_design_specifications(&db, project_row_id, &task).map_err(tool_error)?;

        Ok(Json(TaskMutationResult {
            project_id: project.id,
            project_name: project.name,
            revision: revision.revision,
            task,
            design_specifications,
        }))
    }

    #[tool(
        name = "adashi_finish_task",
        description = "Mark an Adashi task finished. This is AI-owned and records a completion memo plus created and changed files."
    )]
    fn finish_task(
        &self,
        Parameters(params): Parameters<FinishTaskParams>,
    ) -> Result<Json<TaskMutationResult>, ErrorData> {
        let (project, db) = self.open_project(params.project_id.as_deref())?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        let task = tasks::finish_task(
            &db,
            project_row_id,
            FinishTask {
                task_id: params.task_id,
                completion_memo: params.completion_memo,
                created_files: params.created_files.unwrap_or_default(),
                changed_files: params.changed_files.unwrap_or_default(),
            },
        )
        .map_err(tool_error)?;
        let revision =
            project_state::bump_project_revision(&db, project_row_id).map_err(tool_error)?;
        let design_specifications =
            load_task_design_specifications(&db, project_row_id, &task).map_err(tool_error)?;

        Ok(Json(TaskMutationResult {
            project_id: project.id,
            project_name: project.name,
            revision: revision.revision,
            task,
            design_specifications,
        }))
    }

    #[tool(
        name = "adashi_delete_task",
        description = "Delete a project-local Adashi task by id. The write bumps project revision."
    )]
    fn delete_task(
        &self,
        Parameters(params): Parameters<TaskIdParams>,
    ) -> Result<Json<DeleteTaskResult>, ErrorData> {
        let (project, db) = self.open_project(params.project_id.as_deref())?;
        let project_row_id = project_row_id(&db).map_err(tool_error)?;
        tasks::delete_task(&db, project_row_id, params.task_id).map_err(tool_error)?;
        let revision =
            project_state::bump_project_revision(&db, project_row_id).map_err(tool_error)?;

        Ok(Json(DeleteTaskResult {
            project_id: project.id,
            project_name: project.name,
            revision: revision.revision,
            deleted_task_id: params.task_id,
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
        description = "Read a compact top-down C4/UML design overview for the selected project, including supported typed UML artifact types and diagram metadata. This is deterministic retrieval; the agent chooses the relevant design scope and artifact type."
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
        description = "Read an explicit C4 branch by element id, optionally including ancestors, children, typed UML artifacts attached to elements or relationships, bindings, relationships, and canonical source."
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
        description = "Run deterministic text search over stored design elements, relationships, typed UML artifacts, and source. The agent must reason over the hits; the MCP does not infer task context."
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
        description = "Read explicit design elements, relationships, typed UML artifacts, supported artifact types, and bindings by stored design ids."
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
        description = "Read design artifacts explicitly bound to files or symbols, including typed UML artifact metadata when bindings target diagrams. This is stored traceability, not task interpretation."
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
        description = "Transactionally save a formal C4/UML design changeset. Use upsert_uml with diagramType and attachedToExternalId for typed Mermaid artifacts such as class/structure, sequence, flow, and state. The save validates revision, containment, relationships, UML syntax, attachments, and bindings; invalid input returns correction errors and is not stored."
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

fn format_memory_run_start_context(memory: &ProjectMemory) -> String {
    let mut output = String::new();
    let _ = writeln!(output, "# Adashi Memory Context (Generated)");
    let _ = writeln!(output, "Updated: {}", memory.updated_at);
    let _ = writeln!(output);
    let _ = writeln!(output, "Protocol:");
    let _ = writeln!(output, "{}", memory.rule.trim());
    let _ = writeln!(output);
    let _ = writeln!(output, "Current project memory:");

    let memory_body = memory.memory.trim();
    if memory_body.is_empty() {
        let _ = writeln!(output, "(No project memory has been recorded yet.)");
    } else {
        let _ = writeln!(output, "{}", trim_for_injection(memory_body, 16_000));
    }

    output.trim().to_string()
}

fn load_fixed_hook_prompt_for_injection(
    db: &rusqlite::Connection,
    project_row_id: i64,
    intend: &str,
) -> Result<Option<String>, ErrorData> {
    let key = match intend {
        "design" => DESIGN_AUTHORING_HOOK_KEY,
        "implementation" => IMPLEMENTATION_GUIDANCE_HOOK_KEY,
        _ => return Ok(None),
    };
    let prompt = fixed_hooks::load_prompt(db, project_row_id, key).map_err(tool_error)?;
    Ok(prompt
        .map(|prompt| prompt.trim().to_string())
        .filter(|prompt| !prompt.is_empty()))
}

fn format_design_run_start_context(intend: &str, overview: &DesignOverviewResult) -> String {
    let title = if intend == "implementation" {
        "Formal Design Implementation Guide"
    } else {
        "Formal Design Authoring Context"
    };
    let mut output = String::new();
    let _ = writeln!(output, "# {title} (Generated)");
    let _ = writeln!(
        output,
        "Workspace: {} (revision {})",
        overview.workspace_name, overview.revision
    );
    if !overview.workspace_description.trim().is_empty() {
        let _ = writeln!(
            output,
            "Purpose: {}",
            one_line(&overview.workspace_description, 220)
        );
    }
    let _ = writeln!(output);

    if intend == "implementation" {
        let _ = writeln!(
            output,
            "Use this formal design as implementation guidance. Align touched code with the bound design ids below; fetch a narrower scope only when the injected context is insufficient for the specific files, symbols, or component you are changing."
        );
    } else {
        let _ = writeln!(
            output,
            "Use this generated overview as the already-loaded design context. Store design conclusions with `adashi_design_save`; fetch a narrower scope only when the current design task needs more detail than the injected overview."
        );
    }
    let _ = writeln!(
        output,
        "The MCP remains deterministic: agents choose scopes and artifact types from explicit ids, bindings, and metadata."
    );
    let _ = writeln!(output);

    let _ = writeln!(output, "Supported UML artifact types:");
    for artifact_type in &overview.uml_artifact_types {
        let _ = writeln!(
            output,
            "- {} (`{}`): {}",
            artifact_type.artifact_label,
            artifact_type.diagram_type,
            one_line(&artifact_type.description, 160)
        );
    }
    let _ = writeln!(output);

    let _ = writeln!(output, "C4 design index:");
    for line in format_element_index(&overview.elements, 48) {
        let _ = writeln!(output, "{line}");
    }
    let _ = writeln!(output);

    let attached_diagrams = overview
        .diagrams
        .iter()
        .filter(|diagram| diagram.attached_to_external_id.is_some())
        .collect::<Vec<_>>();
    if !attached_diagrams.is_empty() {
        let _ = writeln!(output, "Attached UML artifacts:");
        for diagram in attached_diagrams.iter().take(24) {
            let attached_to = diagram.attached_to_external_id.as_deref().unwrap_or("");
            let target_type = diagram
                .attached_to_target_type
                .as_deref()
                .unwrap_or("design");
            let _ = writeln!(
                output,
                "- `{}` {} `{}` attached to {} `{}`",
                diagram.key, diagram.artifact_label, diagram.diagram_type, target_type, attached_to
            );
        }
        if attached_diagrams.len() > 24 {
            let _ = writeln!(
                output,
                "- ... {} more attached artifacts",
                attached_diagrams.len() - 24
            );
        }
        let _ = writeln!(output);
    }

    if !overview.bindings.is_empty() {
        let _ = writeln!(output, "Design bindings:");
        for binding in overview.bindings.iter().take(40) {
            let _ = writeln!(
                output,
                "- {} `{}` -> design `{}`",
                binding.target_type, binding.target, binding.design_external_id
            );
        }
        if overview.bindings.len() > 40 {
            let _ = writeln!(
                output,
                "- ... {} more bindings",
                overview.bindings.len() - 40
            );
        }
        let _ = writeln!(output);
    }

    let design_decisions = overview
        .relationships
        .iter()
        .filter(|relationship| relationship.tags.contains("Design Decision"))
        .collect::<Vec<_>>();
    if !design_decisions.is_empty() {
        let _ = writeln!(output, "Stored design-decision relationships:");
        for relationship in design_decisions.iter().take(16) {
            let _ = writeln!(
                output,
                "- `{}` -> `{}`: {}",
                relationship.source_external_id,
                relationship.destination_external_id,
                one_line(&relationship.description, 180)
            );
        }
    }

    output.trim().to_string()
}

fn format_element_index(elements: &[design::DesignElementRecord], limit: usize) -> Vec<String> {
    let element_by_id = elements
        .iter()
        .map(|element| (element.external_id.as_str(), element))
        .collect::<HashMap<_, _>>();
    let mut sorted = elements.iter().collect::<Vec<_>>();
    sorted.sort_by(|left, right| {
        element_depth_for_prompt(left, &element_by_id)
            .cmp(&element_depth_for_prompt(right, &element_by_id))
            .then_with(|| left.parent_external_id.cmp(&right.parent_external_id))
            .then_with(|| left.name.cmp(&right.name))
    });

    let mut lines = sorted
        .iter()
        .take(limit)
        .map(|element| {
            let depth = element_depth_for_prompt(element, &element_by_id).min(5);
            let indent = "  ".repeat(depth);
            format!(
                "- {indent}{} `{}` {} - {}",
                element.element_type,
                element.external_id,
                element.name,
                one_line(&element.description, 140)
            )
        })
        .collect::<Vec<_>>();

    if elements.len() > limit {
        lines.push(format!(
            "- ... {} more design elements",
            elements.len() - limit
        ));
    }

    lines
}

fn element_depth_for_prompt(
    element: &design::DesignElementRecord,
    element_by_id: &HashMap<&str, &design::DesignElementRecord>,
) -> usize {
    let mut depth = 0;
    let mut current = element.parent_external_id.as_deref();
    while let Some(parent_id) = current {
        depth += 1;
        current = element_by_id
            .get(parent_id)
            .and_then(|parent| parent.parent_external_id.as_deref());
    }
    depth
}

fn one_line(text: &str, limit: usize) -> String {
    trim_for_injection(
        &text.split_whitespace().collect::<Vec<_>>().join(" "),
        limit,
    )
}

fn trim_for_injection(text: &str, limit: usize) -> String {
    let text = text.trim();
    if text.len() <= limit {
        return text.to_string();
    }

    let mut end = 0;
    for (index, _) in text.char_indices() {
        if index > limit {
            break;
        }
        end = index;
    }

    format!(
        "{}\n\n[Truncated to {} bytes for lifecycle injection. Fetch the dedicated MCP resource if exact full context is required.]",
        text[..end].trim_end(),
        limit
    )
}

fn project_row_id(db: &rusqlite::Connection) -> Result<i64, String> {
    db.query_row("SELECT id FROM projects ORDER BY id LIMIT 1", [], |row| {
        row.get(0)
    })
    .map_err(|err| err.to_string())
}

fn load_task_design_specifications(
    db: &rusqlite::Connection,
    project_row_id: i64,
    task: &Task,
) -> Result<Vec<TaskDesignSpecificationBranch>, String> {
    task.design_specification_links
        .iter()
        .map(|link| {
            let root_id = match link.target_type.as_str() {
                "element" => Some(link.design_external_id.clone()),
                "uml" => db
                    .query_row(
                        "SELECT attached_to_external_id
                         FROM diagrams
                         WHERE key = ?1
                         LIMIT 1",
                        rusqlite::params![link.design_external_id],
                        |row| row.get::<_, Option<String>>(0),
                    )
                    .map_err(|err| err.to_string())?,
                "relationship" => None,
                _ => None,
            };

            let scope = match root_id {
                Some(root_id) => Some(design::load_scope(
                    db,
                    project_row_id,
                    &root_id,
                    true,
                    Some(2),
                    false,
                )?),
                None => None,
            };
            let note = if scope.is_some() {
                None
            } else {
                Some("This link does not resolve to an element-rooted design branch.".to_string())
            };

            Ok(TaskDesignSpecificationBranch {
                link: link.clone(),
                scope,
                note,
            })
        })
        .collect()
}

fn tool_error(message: String) -> ErrorData {
    let value = json!({ "message": message });
    ErrorData::invalid_params("Adashi MCP request failed", Some(value))
}

fn internal_error(err: impl std::fmt::Display) -> ErrorData {
    let value = json!({ "message": err.to_string() });
    ErrorData::internal_error("Adashi MCP server failed", Some(value))
}
