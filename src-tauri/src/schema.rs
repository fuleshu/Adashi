use rusqlite::Connection;

pub fn migrate(db: &mut Connection) -> rusqlite::Result<()> {
    db.execute_batch(include_str!("schema.sql"))?;
    ensure_rules_name_column(db)?;
    ensure_diagram_attachment_columns(db)?;
    ensure_design_bindings_table(db)?;
    ensure_fixed_hook_prompts_table(db)?;
    ensure_task_system_tables(db)?;
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

fn ensure_diagram_attachment_columns(db: &Connection) -> rusqlite::Result<()> {
    let columns = table_columns(db, "diagrams")?;

    if !columns.iter().any(|column| column == "diagram_type") {
        db.execute(
            "ALTER TABLE diagrams ADD COLUMN diagram_type TEXT NOT NULL DEFAULT ''",
            [],
        )?;
    }

    if !columns
        .iter()
        .any(|column| column == "attached_to_external_id")
    {
        db.execute(
            "ALTER TABLE diagrams ADD COLUMN attached_to_external_id TEXT",
            [],
        )?;
    }

    db.execute(
        "UPDATE diagrams
         SET diagram_type = CASE WHEN diagram_type = '' THEN 'sequence' ELSE diagram_type END,
             attached_to_external_id = COALESCE(
                attached_to_external_id,
                (
                    SELECT external_id
                    FROM c4_elements
                    WHERE c4_elements.workspace_id = diagrams.workspace_id
                      AND element_type = 'Software System'
                    ORDER BY id
                    LIMIT 1
                )
             )
         WHERE kind = 'mermaid'",
        [],
    )?;

    db.execute(
        "INSERT OR IGNORE INTO schema_migrations(version) VALUES (6)",
        [],
    )?;
    Ok(())
}

fn ensure_design_bindings_table(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS design_bindings (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            workspace_id INTEGER NOT NULL REFERENCES design_workspaces(id) ON DELETE CASCADE,
            design_external_id TEXT NOT NULL,
            target_type TEXT NOT NULL CHECK(target_type IN ('file', 'symbol')),
            target TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(workspace_id, design_external_id, target_type, target)
        );

        CREATE INDEX IF NOT EXISTS idx_design_bindings_target
            ON design_bindings(workspace_id, target_type, target);",
    )?;
    Ok(())
}

fn ensure_fixed_hook_prompts_table(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS fixed_hook_prompts (
            project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            key TEXT NOT NULL,
            title TEXT NOT NULL,
            intend TEXT NOT NULL CHECK(intend IN ('general', 'design', 'implementation')),
            hook TEXT NOT NULL CHECK(hook IN ('run.start', 'task.start', 'task.end', 'run.end')),
            prompt TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY(project_id, key)
        );",
    )?;
    db.execute(
        "INSERT OR IGNORE INTO schema_migrations(version) VALUES (7)",
        [],
    )?;
    Ok(())
}

fn ensure_task_system_tables(db: &Connection) -> rusqlite::Result<()> {
    let columns = table_columns(db, "agent_tasks")?;

    if !columns.iter().any(|column| column == "number") {
        db.execute(
            "ALTER TABLE agent_tasks ADD COLUMN number INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }

    if !columns.iter().any(|column| column == "description") {
        db.execute(
            "ALTER TABLE agent_tasks ADD COLUMN description TEXT NOT NULL DEFAULT ''",
            [],
        )?;
        if columns.iter().any(|column| column == "body") {
            db.execute("UPDATE agent_tasks SET description = body", [])?;
        }
    }

    if !columns.iter().any(|column| column == "state") {
        db.execute(
            "ALTER TABLE agent_tasks ADD COLUMN state TEXT NOT NULL DEFAULT 'open'",
            [],
        )?;
        if columns.iter().any(|column| column == "status") {
            db.execute(
                "UPDATE agent_tasks
                 SET state = CASE status
                    WHEN 'done' THEN 'finished'
                    ELSE 'open'
                 END",
                [],
            )?;
        }
    }

    add_task_column_if_missing(db, "completed_at", "TEXT")?;
    add_task_column_if_missing(db, "confirmed_at", "TEXT")?;
    add_task_column_if_missing(db, "completion_memo", "TEXT NOT NULL DEFAULT ''")?;
    add_task_column_if_missing(db, "created_files", "TEXT NOT NULL DEFAULT '[]'")?;
    add_task_column_if_missing(db, "changed_files", "TEXT NOT NULL DEFAULT '[]'")?;
    add_task_column_if_missing(db, "confirmation_commit_id", "TEXT")?;

    db.execute(
        "UPDATE agent_tasks
         SET number = id
         WHERE number = 0",
        [],
    )?;
    db.execute(
        "UPDATE agent_tasks
         SET state = CASE
            WHEN state IN ('open', 'finished', 'confirmed') THEN state
            WHEN state = 'done' THEN 'finished'
            ELSE 'open'
         END",
        [],
    )?;

    db.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_tasks_project_number
            ON agent_tasks(project_id, number);

        CREATE TABLE IF NOT EXISTS task_design_specification_links (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            task_id INTEGER NOT NULL REFERENCES agent_tasks(id) ON DELETE CASCADE,
            sort_order INTEGER NOT NULL DEFAULT 0,
            target_type TEXT NOT NULL CHECK(target_type IN ('element', 'relationship', 'uml')),
            design_external_id TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(task_id, design_external_id)
        );

        CREATE INDEX IF NOT EXISTS idx_task_design_links_task_order
            ON task_design_specification_links(task_id, sort_order);

        CREATE TABLE IF NOT EXISTS task_qa_entries (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            task_id INTEGER NOT NULL REFERENCES agent_tasks(id) ON DELETE CASCADE,
            label TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            body TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );",
    )?;

    db.execute(
        "INSERT OR IGNORE INTO schema_migrations(version) VALUES (8)",
        [],
    )?;
    Ok(())
}

fn add_task_column_if_missing(
    db: &Connection,
    column_name: &str,
    column_sql: &str,
) -> rusqlite::Result<()> {
    let columns = table_columns(db, "agent_tasks")?;
    if columns.iter().any(|column| column == column_name) {
        return Ok(());
    }

    db.execute(
        &format!("ALTER TABLE agent_tasks ADD COLUMN {column_name} {column_sql}"),
        [],
    )?;
    Ok(())
}

fn table_columns(db: &Connection, table: &str) -> rusqlite::Result<Vec<String>> {
    let mut statement = db.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
    columns.collect()
}
