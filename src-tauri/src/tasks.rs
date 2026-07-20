use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(rmcp::schemars::JsonSchema)]
pub struct Task {
    pub id: i64,
    pub number: i64,
    pub title: String,
    pub description: String,
    pub state: String,
    pub design_specification_links: Vec<TaskDesignSpecificationLink>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
    pub confirmed_at: Option<String>,
    pub completion_memo: String,
    pub created_files: Vec<String>,
    pub changed_files: Vec<String>,
    pub confirmation_commit_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(rmcp::schemars::JsonSchema)]
pub struct TaskDesignSpecificationLink {
    pub id: i64,
    pub task_id: i64,
    pub sort_order: i64,
    pub target_type: String,
    pub design_external_id: String,
    pub title: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(rmcp::schemars::JsonSchema)]
pub struct TaskDesignSpecificationLinkInput {
    pub target_type: Option<String>,
    pub design_external_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(rmcp::schemars::JsonSchema)]
pub struct NewTask {
    pub title: String,
    pub description: Option<String>,
    pub design_specification_links: Option<Vec<TaskDesignSpecificationLinkInput>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(rmcp::schemars::JsonSchema)]
pub struct UpdateTask {
    pub task_id: i64,
    pub title: Option<String>,
    pub description: Option<String>,
    pub state: Option<String>,
    pub design_specification_links: Option<Vec<TaskDesignSpecificationLinkInput>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(rmcp::schemars::JsonSchema)]
pub struct FinishTask {
    pub task_id: i64,
    pub completion_memo: String,
    pub created_files: Vec<String>,
    pub changed_files: Vec<String>,
}

pub fn load_tasks(
    db: &Connection,
    project_id: i64,
    states: Option<&[String]>,
) -> Result<Vec<Task>, String> {
    let tasks = load_task_rows(db, project_id)?;
    let allowed_states = states.map(|states| {
        states
            .iter()
            .map(|state| state.trim().to_ascii_lowercase())
            .collect::<Vec<_>>()
    });

    tasks
        .into_iter()
        .filter(|task| {
            allowed_states
                .as_ref()
                .map(|states| states.contains(&task.state))
                .unwrap_or(true)
        })
        .map(|task| hydrate_task(db, task))
        .collect()
}

pub fn load_task(db: &Connection, project_id: i64, task_id: i64) -> Result<Task, String> {
    let task = load_task_row(db, project_id, task_id)?;
    hydrate_task(db, task)
}

pub fn create_task(db: &Connection, project_id: i64, input: NewTask) -> Result<Task, String> {
    let title = input.title.trim();
    if title.is_empty() {
        return Err("Task title is required".to_string());
    }

    let number = next_task_number(db, project_id)?;
    db.execute(
        "INSERT INTO agent_tasks(project_id, number, title, description, state)
         VALUES (?1, ?2, ?3, ?4, 'open')",
        params![
            project_id,
            number,
            title,
            input.description.unwrap_or_default().trim()
        ],
    )
    .map_err(|err| err.to_string())?;

    let task_id = db.last_insert_rowid();
    replace_design_links(
        db,
        task_id,
        input.design_specification_links.unwrap_or_default(),
    )?;
    load_task(db, project_id, task_id)
}

pub fn update_task(db: &Connection, project_id: i64, input: UpdateTask) -> Result<Task, String> {
    let current = load_task(db, project_id, input.task_id)?;
    let title = input.title.unwrap_or(current.title).trim().to_string();
    if title.is_empty() {
        return Err("Task title is required".to_string());
    }

    let description = input
        .description
        .unwrap_or(current.description)
        .trim()
        .to_string();
    let state = input.state.unwrap_or(current.state);
    validate_state(&state)?;

    let affected = db
        .execute(
            "UPDATE agent_tasks
             SET title = ?1,
                 description = ?2,
                 state = ?3,
                 updated_at = CURRENT_TIMESTAMP
             WHERE id = ?4 AND project_id = ?5",
            params![title, description, state, input.task_id, project_id],
        )
        .map_err(|err| err.to_string())?;

    if affected == 0 {
        return Err(format!("Unknown task id: {}", input.task_id));
    }

    if let Some(links) = input.design_specification_links {
        replace_design_links(db, input.task_id, links)?;
    }

    load_task(db, project_id, input.task_id)
}

pub fn finish_task(db: &Connection, project_id: i64, input: FinishTask) -> Result<Task, String> {
    let memo = input.completion_memo.trim();
    if memo.is_empty() {
        return Err("Completion memo is required".to_string());
    }

    let affected = db
        .execute(
            "UPDATE agent_tasks
             SET state = 'finished',
                 completed_at = COALESCE(completed_at, CURRENT_TIMESTAMP),
                 completion_memo = ?1,
                 created_files = ?2,
                 changed_files = ?3,
                 updated_at = CURRENT_TIMESTAMP
             WHERE id = ?4 AND project_id = ?5",
            params![
                memo,
                encode_string_list(&input.created_files)?,
                encode_string_list(&input.changed_files)?,
                input.task_id,
                project_id
            ],
        )
        .map_err(|err| err.to_string())?;

    if affected == 0 {
        return Err(format!("Unknown task id: {}", input.task_id));
    }

    load_task(db, project_id, input.task_id)
}

pub fn confirm_task(db: &Connection, project_id: i64, task_id: i64) -> Result<Task, String> {
    let task = load_task(db, project_id, task_id)?;
    if task.state != "finished" && task.state != "confirmed" {
        return Err("Only finished tasks can be confirmed".to_string());
    }

    db.execute(
        "UPDATE agent_tasks
         SET state = 'confirmed',
             confirmed_at = COALESCE(confirmed_at, CURRENT_TIMESTAMP),
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?1 AND project_id = ?2",
        params![task_id, project_id],
    )
    .map_err(|err| err.to_string())?;

    load_task(db, project_id, task_id)
}

pub fn delete_task(db: &Connection, project_id: i64, task_id: i64) -> Result<(), String> {
    let affected = db
        .execute(
            "DELETE FROM agent_tasks WHERE id = ?1 AND project_id = ?2",
            params![task_id, project_id],
        )
        .map_err(|err| err.to_string())?;

    if affected == 0 {
        return Err(format!("Unknown task id: {task_id}"));
    }

    Ok(())
}

fn hydrate_task(db: &Connection, task: TaskRow) -> Result<Task, String> {
    let links = load_design_links(db, task.id)?;
    Ok(Task {
        id: task.id,
        number: task.number,
        title: task.title,
        description: task.description,
        state: task.state,
        design_specification_links: links,
        created_at: task.created_at,
        updated_at: task.updated_at,
        completed_at: task.completed_at,
        confirmed_at: task.confirmed_at,
        completion_memo: task.completion_memo,
        created_files: decode_string_list(&task.created_files)?,
        changed_files: decode_string_list(&task.changed_files)?,
        confirmation_commit_id: task.confirmation_commit_id,
    })
}

fn load_task_rows(db: &Connection, project_id: i64) -> Result<Vec<TaskRow>, String> {
    let mut statement = db
        .prepare(
            "SELECT id, number, title, description, state, created_at, updated_at,
                    completed_at, confirmed_at, completion_memo, created_files,
                    changed_files, confirmation_commit_id
             FROM agent_tasks
             WHERE project_id = ?1
             ORDER BY
                CASE state
                    WHEN 'open' THEN 0
                    WHEN 'finished' THEN 1
                    WHEN 'confirmed' THEN 2
                    ELSE 3
                END,
                number",
        )
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map(params![project_id], read_task_row)
        .map_err(|err| err.to_string())?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| err.to_string())
}

fn load_task_row(db: &Connection, project_id: i64, task_id: i64) -> Result<TaskRow, String> {
    db.query_row(
        "SELECT id, number, title, description, state, created_at, updated_at,
                completed_at, confirmed_at, completion_memo, created_files,
                changed_files, confirmation_commit_id
         FROM agent_tasks
         WHERE id = ?1 AND project_id = ?2",
        params![task_id, project_id],
        read_task_row,
    )
    .map_err(|err| err.to_string())
}

fn read_task_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskRow> {
    Ok(TaskRow {
        id: row.get(0)?,
        number: row.get(1)?,
        title: row.get(2)?,
        description: row.get(3)?,
        state: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        completed_at: row.get(7)?,
        confirmed_at: row.get(8)?,
        completion_memo: row.get(9)?,
        created_files: row.get(10)?,
        changed_files: row.get(11)?,
        confirmation_commit_id: row.get(12)?,
    })
}

fn load_design_links(
    db: &Connection,
    task_id: i64,
) -> Result<Vec<TaskDesignSpecificationLink>, String> {
    let mut statement = db
        .prepare(
            "SELECT
                l.id,
                l.task_id,
                l.sort_order,
                l.target_type,
                l.design_external_id,
                COALESCE(e.name, r.description, d.title, m.title, l.design_external_id) AS title
             FROM task_design_specification_links l
             LEFT JOIN c4_elements e ON e.external_id = l.design_external_id
             LEFT JOIN c4_relationships r ON r.external_id = l.design_external_id
             LEFT JOIN diagrams d ON d.key = l.design_external_id
             LEFT JOIN ui_mockups m ON m.external_id = l.design_external_id
             WHERE l.task_id = ?1
             ORDER BY l.sort_order, l.id",
        )
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map(params![task_id], |row| {
            Ok(TaskDesignSpecificationLink {
                id: row.get(0)?,
                task_id: row.get(1)?,
                sort_order: row.get(2)?,
                target_type: row.get(3)?,
                design_external_id: row.get(4)?,
                title: row.get(5)?,
            })
        })
        .map_err(|err| err.to_string())?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| err.to_string())
}

fn replace_design_links(
    db: &Connection,
    task_id: i64,
    links: Vec<TaskDesignSpecificationLinkInput>,
) -> Result<(), String> {
    db.execute(
        "DELETE FROM task_design_specification_links WHERE task_id = ?1",
        params![task_id],
    )
    .map_err(|err| err.to_string())?;

    for (index, link) in links.into_iter().enumerate() {
        let design_external_id = link.design_external_id.trim();
        if design_external_id.is_empty() {
            return Err("Design specification link id is required".to_string());
        }

        let target_type = match link.target_type {
            Some(target_type) if !target_type.trim().is_empty() => target_type.trim().to_string(),
            _ => infer_design_target_type(db, design_external_id)?,
        };
        validate_design_target_type(&target_type)?;

        db.execute(
            "INSERT INTO task_design_specification_links(
                task_id, sort_order, target_type, design_external_id
             )
             VALUES (?1, ?2, ?3, ?4)",
            params![task_id, index as i64, target_type, design_external_id],
        )
        .map_err(|err| err.to_string())?;
    }

    Ok(())
}

fn infer_design_target_type(db: &Connection, design_external_id: &str) -> Result<String, String> {
    let element_exists = db
        .query_row(
            "SELECT 1 FROM c4_elements WHERE external_id = ?1 LIMIT 1",
            params![design_external_id],
            |_| Ok(()),
        )
        .optional()
        .map_err(|err| err.to_string())?
        .is_some();
    if element_exists {
        return Ok("element".to_string());
    }

    let relationship_exists = db
        .query_row(
            "SELECT 1 FROM c4_relationships WHERE external_id = ?1 LIMIT 1",
            params![design_external_id],
            |_| Ok(()),
        )
        .optional()
        .map_err(|err| err.to_string())?
        .is_some();
    if relationship_exists {
        return Ok("relationship".to_string());
    }

    let diagram_exists = db
        .query_row(
            "SELECT 1 FROM diagrams WHERE key = ?1 LIMIT 1",
            params![design_external_id],
            |_| Ok(()),
        )
        .optional()
        .map_err(|err| err.to_string())?
        .is_some();
    if diagram_exists {
        return Ok("uml".to_string());
    }

    let mockup_exists = db
        .query_row(
            "SELECT 1 FROM ui_mockups WHERE external_id=?1 LIMIT 1",
            params![design_external_id],
            |_| Ok(()),
        )
        .optional()
        .map_err(|err| err.to_string())?
        .is_some();
    if mockup_exists {
        return Ok("mockup".to_string());
    }

    Err(format!(
        "Unknown design specification id: {design_external_id}"
    ))
}

fn next_task_number(db: &Connection, project_id: i64) -> Result<i64, String> {
    db.query_row(
        "SELECT COALESCE(MAX(number), 0) + 1 FROM agent_tasks WHERE project_id = ?1",
        params![project_id],
        |row| row.get(0),
    )
    .map_err(|err| err.to_string())
}

fn validate_state(state: &str) -> Result<(), String> {
    const STATES: &[&str] = &["open", "finished", "confirmed"];
    if STATES.contains(&state) {
        Ok(())
    } else {
        Err(format!(
            "Invalid task state '{state}'. Expected one of: {}",
            STATES.join(", ")
        ))
    }
}

fn validate_design_target_type(target_type: &str) -> Result<(), String> {
    const TARGET_TYPES: &[&str] = &["element", "relationship", "uml", "mockup"];
    if TARGET_TYPES.contains(&target_type) {
        Ok(())
    } else {
        Err(format!(
            "Invalid design link target type '{target_type}'. Expected one of: {}",
            TARGET_TYPES.join(", ")
        ))
    }
}

fn encode_string_list(values: &[String]) -> Result<String, String> {
    let cleaned = values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    serde_json::to_string(&cleaned).map_err(|err| err.to_string())
}

fn decode_string_list(source: &str) -> Result<Vec<String>, String> {
    if source.trim().is_empty() {
        return Ok(Vec::new());
    }

    serde_json::from_str(source).map_err(|err| err.to_string())
}

struct TaskRow {
    id: i64,
    number: i64,
    title: String,
    description: String,
    state: String,
    created_at: String,
    updated_at: String,
    completed_at: Option<String>,
    confirmed_at: Option<String>,
    completion_memo: String,
    created_files: String,
    changed_files: String,
    confirmation_commit_id: Option<String>,
}
