use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::concurrency;

/// The task lifecycle, in order: `todo` is created and unclaimed, `active` is being worked on,
/// `finished` is reported complete and awaiting review, `closed` is reviewed and accepted.
///
/// `finished` and `closed` are deliberately different: an agent reporting its own work complete
/// is not a verdict, so the review step is a separate transition. A reviewer who disagrees sends
/// the task back to `active`, which clears the completion timestamp.
pub const TASK_STATE_NAMES: [&str; 4] = ["todo", "active", "finished", "closed"];

/// States a default listing shows: everything except closed work.

/// The error every unrecognised-state path returns, so the accepted values are stated from one
/// place and a caller never has to guess or retry.
pub fn invalid_task_state_error(value: &str) -> String {
    format!(
        "Invalid task state '{value}'. Expected one of: {}. Tasks start in 'todo'; a review that \
         rejects finished work sets it back to 'active'.",
        TASK_STATE_NAMES.join(", ")
    )
}

/// Whether moving `from` -> `to` drops the completion claim. `finished` and `closed` both mean
/// "the work is complete", so leaving either of them for real work clears `completed_at`.
fn clears_completed_at(from: TaskState, to: TaskState) -> bool {
    matches!(from, TaskState::Finished | TaskState::Closed)
        && !matches!(to, TaskState::Finished | TaskState::Closed)
}

#[derive(
    Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord,
    rmcp::schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum TaskState {
    Todo,
    Active,
    Finished,
    Closed,
}

impl TaskState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::Active => "active",
            Self::Finished => "finished",
            Self::Closed => "closed",
        }
    }

    /// Parses a caller-supplied state name, rejecting anything unrecognised with the full list.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "todo" => Ok(Self::Todo),
            "active" => Ok(Self::Active),
            "finished" => Ok(Self::Finished),
            "closed" => Ok(Self::Closed),
            other => Err(invalid_task_state_error(other)),
        }
    }
}

/// Whether `from` may become `to`. A state may always be re-set to itself, so a no-op update
/// stays idempotent. `closed` requires `finished`: a reviewer closes work they have seen, and the
/// only way to un-close it is to send it back to `active`.
pub fn task_transition_allowed(from: TaskState, to: TaskState) -> bool {
    if from == to {
        return true;
    }
    match to {
        TaskState::Todo => false,
        TaskState::Active => true,
        TaskState::Finished => from == TaskState::Active,
        TaskState::Closed => from == TaskState::Finished,
    }
}

/// Every state, for a consumer that applies its own visibility rules.
pub const ALL_TASK_STATES: [TaskState; 4] = [
    TaskState::Todo,
    TaskState::Active,
    TaskState::Finished,
    TaskState::Closed,
];

#[derive(Clone, Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskSummary {
    pub id: i64,
    pub number: i64,
    pub title: String,
    pub title_truncated: bool,
    pub state: TaskState,
    pub version: i64,
}

/// States a default listing shows: everything except closed work.
pub const DEFAULT_VISIBLE_STATES: [TaskState; 3] =
    [TaskState::Todo, TaskState::Active, TaskState::Finished];

/// The state filter to apply when a caller omits one: open work only, with closed tasks excluded
/// so accepted history cannot be mistaken for current work.
///
/// This is the *default view*, not "every state". A consumer that owns its own visibility control
/// — the dashboard, which offers a per-state checkbox — must ask for `ALL_TASK_STATES` instead,
/// or the control it shows can never reveal anything.
pub fn default_state_filter(states: Option<&[TaskState]>) -> Vec<TaskState> {
    match states {
        Some(states) => states.to_vec(),
        None => DEFAULT_VISIBLE_STATES.to_vec(),
    }
}

/// Counts the tasks a default listing withholds, so the omission can be stated rather than
/// silently applied.
pub fn count_closed_tasks(db: &Connection, project_id: i64) -> Result<i64, String> {
    db.query_row(
        "SELECT COUNT(*) FROM agent_tasks WHERE project_id=?1 AND state=?2",
        params![project_id, TaskState::Closed.as_str()],
        |row| row.get(0),
    )
    .map_err(|err| err.to_string())
}

/// Query only identifying columns. Detail hydration belongs to load_task.
///
/// `states` is the already-resolved filter: use `default_state_filter` for a listing where the
/// caller omitted the filter, so closed work is excluded by default rather than by accident.
pub fn load_task_summaries(
    db: &Connection,
    project_id: i64,
    states: &[TaskState],
    after_id: i64,
    limit: u32,
) -> Result<(Vec<TaskSummary>, i64), String> {
    let filter = serde_json::to_string(states).map_err(|e| e.to_string())?;
    let total = db
        .query_row(
            "SELECT COUNT(*) FROM agent_tasks WHERE project_id=?1
         AND state IN (SELECT value FROM json_each(?2))",
            params![project_id, filter],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    let mut statement = db.prepare(
        "SELECT t.id, t.number, substr(t.title,1,240), t.state, COALESCE(rv.version, 0), length(t.title)>240
         FROM agent_tasks t LEFT JOIN resource_versions rv
           ON rv.project_id=t.project_id AND rv.resource_kind='task' AND rv.resource_id=CAST(t.id AS TEXT)
         WHERE t.project_id=?1 AND t.state IN (SELECT value FROM json_each(?2))
           AND t.id>?3 ORDER BY t.id LIMIT ?4"
    ).map_err(|e| e.to_string())?;
    let rows = statement
        .query_map(params![project_id, filter, after_id, limit], |row| {
            let raw: String = row.get(3)?;
            // The column has a CHECK constraint over the same vocabulary; a value that reaches
            // here unrecognised means the stored data and the machine have diverged.
            let state = TaskState::parse(&raw).map_err(|_| rusqlite::Error::InvalidQuery)?;
            Ok(TaskSummary {
                id: row.get(0)?,
                number: row.get(1)?,
                title: row.get(2)?,
                title_truncated: row.get(5)?,
                state,
                version: row.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?;
    Ok((
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| e.to_string())?,
        total,
    ))
}

#[cfg(test)]
mod summary_tests {
    use super::*;

    #[test]
    fn summaries_bound_titles_and_never_hydrate_detail_columns() {
        let mut db = Connection::open_in_memory().unwrap();
        crate::schema::migrate(&mut db).unwrap();
        db.execute("INSERT INTO projects(name,slug) VALUES('P','p')", [])
            .unwrap();
        db.execute("INSERT INTO agent_tasks(project_id,number,title,description,state,created_files) VALUES(1,1,?1,?2,'active','invalid json')",
            params!["界".repeat(10_000), "large detail ".repeat(10_000)]).unwrap();
        let (tasks, total) = load_task_summaries(&db, 1, &[TaskState::Active], 0, 25).unwrap();
        assert_eq!(total, 1);
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title.chars().count(), 240);
        assert!(tasks[0].title_truncated);
        assert!(serde_json::to_vec(&tasks).unwrap().len() < 1024);
        assert!(load_task_summaries(&db, 1, &[], 0, 25).unwrap().0.is_empty());
        assert_eq!(
            load_task_summaries(&db, 1, &default_state_filter(None), tasks[0].id, 25)
                .unwrap()
                .1,
            1
        );
    }

    #[test]
    fn an_omitted_filter_hides_closed_work_and_an_explicit_filter_does_not() {
        let mut db = Connection::open_in_memory().unwrap();
        crate::schema::migrate(&mut db).unwrap();
        db.execute("INSERT INTO projects(name,slug) VALUES('P','p')", [])
            .unwrap();
        for (number, state) in [(1_i64, "todo"), (2, "active"), (3, "finished"), (4, "closed")] {
            db.execute(
                "INSERT INTO agent_tasks(project_id,number,title,state) VALUES(1,?1,?2,?3)",
                params![number, format!("Task {state}"), state],
            )
            .unwrap_or_else(|error| panic!("{state} must satisfy the stored constraint: {error}"));
        }

        let default_filter = default_state_filter(None);
        let (visible, total) = load_task_summaries(&db, 1, &default_filter, 0, 25).unwrap();
        assert_eq!(total, 3);
        assert_eq!(visible.len(), 3);
        assert!(
            visible.iter().all(|task| task.state != TaskState::Closed),
            "closed work must not appear in a default listing"
        );
        assert_eq!(count_closed_tasks(&db, 1).unwrap(), 1);

        let (closed, _) = load_task_summaries(&db, 1, &[TaskState::Closed], 0, 25).unwrap();
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].state, TaskState::Closed);

        let (everything, _) = load_task_summaries(
            &db,
            1,
            &default_state_filter(Some(&[
                TaskState::Todo,
                TaskState::Active,
                TaskState::Finished,
                TaskState::Closed,
            ])),
            0,
            25,
        )
        .unwrap();
        assert_eq!(everything.len(), 4);
    }

    #[test]
    fn the_lifecycle_is_todo_active_finished_closed_with_matching_timestamps() {
        fn in_memory() -> Connection {
            let mut db = Connection::open_in_memory().unwrap();
            crate::schema::migrate(&mut db).unwrap();
            db.execute("INSERT INTO projects(name,slug) VALUES('P','p')", [])
                .unwrap();
            db
        }
        fn new_task(db: &Connection, title: &str) -> Task {
            create_task(
                db,
                1,
                NewTask {
                    title: title.to_string(),
                    description: None,
                    design_specification_links: None,
                },
            )
            .unwrap()
        }
        fn set_state(db: &Connection, task: &Task, state: &str) -> Result<Task, String> {
            update_task(
                db,
                1,
                UpdateTask {
                    task_id: task.id,
                    title: None,
                    description: None,
                    state: Some(state.to_string()),
                    design_specification_links: None,
                },
            )
        }

        let db = in_memory();
        let task = new_task(&db, "Ship the thing");
        assert_eq!(task.state, "todo", "a new task is unclaimed");
        assert!(task.completed_at.is_none() && task.confirmed_at.is_none());

        // Work starts, then the agent reports it complete.
        let active = set_state(&db, &task, "active").unwrap();
        assert_eq!(active.state, "active");
        assert!(active.completed_at.is_none());

        finish_task(
            &db,
            1,
            FinishTask {
                task_id: task.id,
                completion_memo: "done the first time".to_string(),
                created_files: vec!["a.rs".to_string()],
                changed_files: vec![],
            },
        )
        .unwrap();
        let finished = load_task(&db, 1, task.id).unwrap();
        assert_eq!(finished.state, "finished");
        assert!(finished.completed_at.is_some(), "finishing stamps completion");

        // A review that rejects the work sends it back to active and drops the completion claim.
        let reopened = set_state(&db, &finished, "active").unwrap();
        assert_eq!(reopened.state, "active");
        assert!(reopened.completed_at.is_none(), "reopening clears completed_at");
        assert_eq!(
            reopened.completion_memo, "done the first time",
            "the evidence stays for the next attempt"
        );

        // Closing needs finished, and closed can only be re-opened to active.
        let err = set_state(&db, &reopened, "closed").unwrap_err();
        assert!(err.contains("active -> closed"), "{err}");
        assert_eq!(set_state(&db, &reopened, "finished").unwrap().state, "finished");
        let closed = close_task(&db, 1, task.id).unwrap();
        assert_eq!(closed.state, "closed");
        assert!(closed.confirmed_at.is_some(), "closing stamps acceptance");
        assert_eq!(
            close_task(&db, 1, task.id).unwrap().state,
            "closed",
            "closing twice is an idempotent retry, not an error"
        );
        let err = set_state(&db, &closed, "finished").unwrap_err();
        assert!(err.contains("closed -> finished"), "{err}");
        let reopened_from_closed = set_state(&db, &closed, "active").unwrap();
        assert_eq!(reopened_from_closed.state, "active");
        assert!(reopened_from_closed.completed_at.is_none());

        // todo cannot be re-entered, and an unknown state names the accepted values.
        let err = set_state(&db, &reopened_from_closed, "todo").unwrap_err();
        assert!(err.contains("active -> todo"), "{err}");
        let err = set_state(&db, &reopened_from_closed, "nonsense").unwrap_err();
        assert!(
            err.contains("Expected one of: todo, active, finished, closed"),
            "{err}"
        );
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(rmcp::schemars::JsonSchema)]
pub struct Task {
    pub id: i64,
    pub version: i64,
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(rmcp::schemars::JsonSchema)]
pub struct TaskDesignSpecificationLinkInput {
    pub target_type: Option<String>,
    pub design_external_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(rmcp::schemars::JsonSchema)]
pub struct NewTask {
    pub title: String,
    pub description: Option<String>,
    pub design_specification_links: Option<Vec<TaskDesignSpecificationLinkInput>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(rmcp::schemars::JsonSchema)]
pub struct UpdateTask {
    pub task_id: i64,
    pub title: Option<String>,
    pub description: Option<String>,
    pub state: Option<String>,
    pub design_specification_links: Option<Vec<TaskDesignSpecificationLinkInput>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(rmcp::schemars::JsonSchema)]
pub struct FinishTask {
    pub task_id: i64,
    pub completion_memo: String,
    pub created_files: Vec<String>,
    pub changed_files: Vec<String>,
}

/// Loads the tasks in `states`.
///
/// `states` is required rather than optional on purpose: the parameter decides whether closed
/// history is visible, and an omitted value silently meaning "the default view" is how the
/// dashboard's closed checkbox ended up filtering a list that never contained closed tasks. Use
/// `default_state_filter` for the default view or `ALL_TASK_STATES` for everything.
pub fn load_tasks(
    db: &Connection,
    project_id: i64,
    states: &[TaskState],
) -> Result<Vec<Task>, String> {
    let tasks = load_task_rows(db, project_id)?;

    tasks
        .into_iter()
        .filter(|task| states.iter().any(|state| state.as_str() == task.state))
        .map(|task| hydrate_task(db, project_id, task))
        .collect()
}

pub fn load_task(db: &Connection, project_id: i64, task_id: i64) -> Result<Task, String> {
    let task = load_task_row(db, project_id, task_id)?;
    hydrate_task(db, project_id, task)
}

pub fn create_task(db: &Connection, project_id: i64, input: NewTask) -> Result<Task, String> {
    let title = input.title.trim();
    if title.is_empty() {
        return Err("Task title is required".to_string());
    }

    let number = next_task_number(db, project_id)?;
    db.execute(
        "INSERT INTO agent_tasks(project_id, number, title, description, state)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            project_id,
            number,
            title,
            input.description.unwrap_or_default().trim(),
            TaskState::Todo.as_str()
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
    let from = TaskState::parse(&current.state)?;
    let to = match input.state {
        Some(state) => TaskState::parse(&state)?,
        None => from,
    };
    if !task_transition_allowed(from, to) {
        return Err(format!(
            "Invalid task transition {} -> {}. A task is worked on (active) and only a review \
             closes it (closed); to take a finished task back to work, set it to active.",
            from.as_str(),
            to.as_str()
        ));
    }

    // Leaving finished means the work is not complete after all, so the completion timestamp is
    // cleared rather than kept as a claim the task no longer makes. Closing implies finished, so
    // re-opening a closed task clears it too. The memo and file lists stay, because they are the
    // evidence the next attempt needs.
    let clear_completed_at = clears_completed_at(from, to);
    let set_completed_at = to == TaskState::Finished;

    let affected = db
        .execute(
            "UPDATE agent_tasks
             SET title = ?1,
                 description = ?2,
                 state = ?3,
                 completed_at = CASE
                    WHEN ?4 THEN NULL
                    WHEN ?5 THEN COALESCE(completed_at, CURRENT_TIMESTAMP)
                    ELSE completed_at
                 END,
                 updated_at = CURRENT_TIMESTAMP
             WHERE id = ?6 AND project_id = ?7",
            params![
                title,
                description,
                to.as_str(),
                clear_completed_at,
                set_completed_at,
                input.task_id,
                project_id
            ],
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
                 completed_at = CURRENT_TIMESTAMP,
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

/// Closes a task: the review accepted the finished work. Only `finished` may be closed, so the
/// only way out of `closed` is back to `active`, which is a deliberate re-open.
pub fn close_task(db: &Connection, project_id: i64, task_id: i64) -> Result<Task, String> {
    let task = load_task(db, project_id, task_id)?;
    let from = TaskState::parse(&task.state)?;
    if !task_transition_allowed(from, TaskState::Closed) {
        return Err(format!(
            "Invalid task transition {} -> closed. Only finished tasks can be closed.",
            from.as_str()
        ));
    }

    db.execute(
        "UPDATE agent_tasks
         SET state = 'closed',
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

fn hydrate_task(db: &Connection, project_id: i64, task: TaskRow) -> Result<Task, String> {
    let links = load_design_links(db, task.id)?;
    Ok(Task {
        id: task.id,
        version: concurrency::load_version(db, project_id, "task", &task.id.to_string())?,
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
                    WHEN 'todo' THEN 0
                    WHEN 'active' THEN 1
                    WHEN 'finished' THEN 2
                    WHEN 'closed' THEN 3
                    ELSE 4
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

/// Design link target kinds, from one list so the schema, the error text and the storage check
/// cannot disagree.
pub const DESIGN_TARGET_TYPES: [&str; 4] = ["element", "relationship", "uml", "mockup"];

pub fn invalid_design_target_type_error(value: &str) -> String {
    format!(
        "Invalid design link target type '{value}'. Expected one of: {}",
        DESIGN_TARGET_TYPES.join(", ")
    )
}

fn validate_design_target_type(target_type: &str) -> Result<(), String> {
    if DESIGN_TARGET_TYPES.contains(&target_type) {
        Ok(())
    } else {
        Err(invalid_design_target_type_error(target_type))
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
