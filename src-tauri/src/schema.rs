use rusqlite::Connection;

pub fn migrate(db: &mut Connection) -> rusqlite::Result<()> {
    db.execute_batch(include_str!("schema.sql"))?;
    ensure_rules_name_column(db)?;
    crate::state::ensure_project_state(db)?;
    ensure_project_memory_rows(db)?;
    Ok(())
}

fn ensure_rules_name_column(db: &Connection) -> rusqlite::Result<()> {
    let mut statement = db.prepare("PRAGMA table_info(rules)")?;
    let columns = statement.query_map([], |row| row.get::<_, String>(1))?;

    for column in columns {
        if column? == "name" {
            return Ok(());
        }
    }

    db.execute(
        "ALTER TABLE rules ADD COLUMN name TEXT NOT NULL DEFAULT ''",
        [],
    )?;
    db.execute(
        "INSERT OR IGNORE INTO schema_migrations(version) VALUES (3)",
        [],
    )?;
    Ok(())
}

fn ensure_project_memory_rows(db: &Connection) -> rusqlite::Result<()> {
    db.execute(
        "INSERT OR IGNORE INTO project_memory(project_id, protocol_rule, memory_body)
         SELECT id, ?1, ''
         FROM projects",
        [crate::memory::DEFAULT_MEMORY_RULE],
    )?;
    Ok(())
}
