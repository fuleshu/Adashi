//! Snapshot-local task communication numbers, shared by all transports/adapters.
//! Rank every retained task before filters or paging. Only identifying columns
//! enter the window query, so list retrieval never materializes task bodies.
pub(super) const CTE: &str = "WITH task_numbers AS (
    SELECT id, ROW_NUMBER() OVER (
        PARTITION BY project_id ORDER BY created_at, id
    ) AS display_number FROM agent_tasks
)";

pub(super) fn resolve(db: &rusqlite::Connection, project: i64, number: i64) -> Result<i64, String> {
    if number <= 0 {
        return Err("Task number must be positive".into());
    }
    db.query_row(
        &format!(
            "{CTE} SELECT t.id FROM agent_tasks t JOIN task_numbers n USING(id)
            WHERE t.project_id=?1 AND n.display_number=?2"
        ),
        rusqlite::params![project, number],
        |r| r.get(0),
    )
    .map_err(|_| format!("Unknown task number: {number}"))
}
