use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(rmcp::schemars::JsonSchema)]
pub struct Task {
    pub id: i64,
    pub title: String,
    pub body: String,
    pub status: String,
    pub priority: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(rmcp::schemars::JsonSchema)]
pub struct NewTask {
    pub title: String,
    pub body: Option<String>,
    pub status: Option<String>,
    pub priority: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(rmcp::schemars::JsonSchema)]
pub struct UpdateTask {
    pub task_id: i64,
    pub title: Option<String>,
    pub body: Option<String>,
    pub status: Option<String>,
    pub priority: Option<i64>,
}

pub fn load_tasks(db: &Connection) -> Result<Vec<Task>, String> {
    let mut statement = db
        .prepare("SELECT id, title, body, status, priority FROM agent_tasks ORDER BY priority, id")
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(Task {
                id: row.get(0)?,
                title: row.get(1)?,
                body: row.get(2)?,
                status: row.get(3)?,
                priority: row.get(4)?,
            })
        })
        .map_err(|err| err.to_string())?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| err.to_string())
}

pub fn create_task(db: &Connection, project_id: i64, input: NewTask) -> Result<Task, String> {
    let title = input.title.trim();
    if title.is_empty() {
        return Err("Task title is required".to_string());
    }

    let status = input.status.unwrap_or_else(|| "planned".to_string());
    validate_status(&status)?;

    let priority = input.priority.unwrap_or(3);
    validate_priority(priority)?;

    db.execute(
        "INSERT INTO agent_tasks(project_id, title, body, status, priority)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            project_id,
            title,
            input.body.unwrap_or_default(),
            status,
            priority
        ],
    )
    .map_err(|err| err.to_string())?;

    load_task(db, db.last_insert_rowid())
}

pub fn update_task(db: &Connection, input: UpdateTask) -> Result<Task, String> {
    let current = load_task(db, input.task_id)?;
    let title = input.title.unwrap_or(current.title).trim().to_string();
    if title.is_empty() {
        return Err("Task title is required".to_string());
    }

    let body = input.body.unwrap_or(current.body);
    let status = input.status.unwrap_or(current.status);
    validate_status(&status)?;

    let priority = input.priority.unwrap_or(current.priority);
    validate_priority(priority)?;

    let affected = db
        .execute(
            "UPDATE agent_tasks
             SET title = ?1,
                 body = ?2,
                 status = ?3,
                 priority = ?4,
                 updated_at = CURRENT_TIMESTAMP
             WHERE id = ?5",
            params![title, body, status, priority, input.task_id],
        )
        .map_err(|err| err.to_string())?;

    if affected == 0 {
        return Err(format!("Unknown task id: {}", input.task_id));
    }

    load_task(db, input.task_id)
}

fn load_task(db: &Connection, task_id: i64) -> Result<Task, String> {
    db.query_row(
        "SELECT id, title, body, status, priority FROM agent_tasks WHERE id = ?1",
        params![task_id],
        |row| {
            Ok(Task {
                id: row.get(0)?,
                title: row.get(1)?,
                body: row.get(2)?,
                status: row.get(3)?,
                priority: row.get(4)?,
            })
        },
    )
    .map_err(|err| err.to_string())
}

fn validate_status(status: &str) -> Result<(), String> {
    const STATUSES: &[&str] = &["planned", "ready", "active", "blocked", "done"];
    if STATUSES.contains(&status) {
        Ok(())
    } else {
        Err(format!(
            "Invalid task status '{status}'. Expected one of: {}",
            STATUSES.join(", ")
        ))
    }
}

fn validate_priority(priority: i64) -> Result<(), String> {
    if (1..=5).contains(&priority) {
        Ok(())
    } else {
        Err("Task priority must be between 1 and 5".to_string())
    }
}
