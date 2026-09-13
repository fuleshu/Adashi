use rusqlite::Connection;

pub fn migrate(db: &mut Connection) -> rusqlite::Result<()> {
    db.execute_batch(include_str!("schema.sql"))?;
    db.execute_batch(include_str!("concurrency_schema.sql"))?;
    ensure_rules_name_column(db)?;
    ensure_diagram_attachment_columns(db)?;
    ensure_design_bindings_table(db)?;
    ensure_fixed_hook_prompts_table(db)?;
    ensure_task_system_tables(db)?;
    ensure_qa_system_tables(db)?;
    ensure_ui_mockup_tables(db)?;
    ensure_mockup_design_link_targets(db)?;
    crate::state::ensure_project_state(db)?;
    ensure_project_memory_rows(db)?;
    Ok(())
}

fn ensure_mockup_design_link_targets(db: &Connection) -> rusqlite::Result<()> {
    let task_sql: String = db.query_row(
        "SELECT sql FROM sqlite_master WHERE type='table' AND name='task_design_specification_links'",
        [], |row| row.get(0),
    )?;
    if !task_sql.contains("'mockup'") {
        db.execute_batch(
            "ALTER TABLE task_design_specification_links RENAME TO task_design_specification_links_legacy;
             CREATE TABLE task_design_specification_links (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id INTEGER NOT NULL REFERENCES agent_tasks(id) ON DELETE CASCADE,
                sort_order INTEGER NOT NULL DEFAULT 0,
                target_type TEXT NOT NULL CHECK(target_type IN ('element', 'relationship', 'uml', 'mockup')),
                design_external_id TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(task_id, design_external_id)
             );
             INSERT INTO task_design_specification_links(id, task_id, sort_order, target_type, design_external_id, created_at)
                SELECT id, task_id, sort_order, target_type, design_external_id, created_at FROM task_design_specification_links_legacy;
             DROP TABLE task_design_specification_links_legacy;
             CREATE INDEX idx_task_design_links_task_order ON task_design_specification_links(task_id, sort_order);",
        )?;
    }

    let qa_sql: String = db.query_row(
        "SELECT sql FROM sqlite_master WHERE type='table' AND name='qa_job_design_links'",
        [],
        |row| row.get(0),
    )?;
    if !qa_sql.contains("'mockup'") {
        db.execute_batch(
            "ALTER TABLE qa_job_design_links RENAME TO qa_job_design_links_legacy;
             CREATE TABLE qa_job_design_links (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                qa_job_id INTEGER NOT NULL REFERENCES qa_jobs(id) ON DELETE CASCADE,
                sort_order INTEGER NOT NULL DEFAULT 0,
                target_type TEXT NOT NULL CHECK(target_type IN ('element', 'relationship', 'uml', 'mockup')),
                design_external_id TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(qa_job_id, design_external_id)
             );
             INSERT INTO qa_job_design_links(id, qa_job_id, sort_order, target_type, design_external_id, created_at)
                SELECT id, qa_job_id, sort_order, target_type, design_external_id, created_at FROM qa_job_design_links_legacy;
             DROP TABLE qa_job_design_links_legacy;
             CREATE INDEX idx_qa_job_design_links_job_order ON qa_job_design_links(qa_job_id, sort_order);
             CREATE INDEX idx_qa_job_design_links_target ON qa_job_design_links(design_external_id);",
        )?;
    }
    Ok(())
}

fn ensure_ui_mockup_tables(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS ui_mockups (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            external_id TEXT NOT NULL,
            title TEXT NOT NULL,
            attached_to_external_id TEXT NOT NULL,
            viewport_width INTEGER NOT NULL,
            viewport_height INTEGER NOT NULL,
            screen TEXT NOT NULL DEFAULT '',
            state TEXT NOT NULL DEFAULT '',
            fidelity TEXT NOT NULL DEFAULT '',
            schema_version INTEGER NOT NULL DEFAULT 1,
            accepted_svg TEXT NOT NULL,
            accepted_revision INTEGER NOT NULL DEFAULT 1,
            working_svg TEXT,
            base_revision INTEGER,
            status TEXT NOT NULL DEFAULT 'accepted' CHECK(status IN ('accepted', 'workingDraft', 'pendingAgent', 'proposed')),
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(project_id, external_id)
        );

        CREATE INDEX IF NOT EXISTS idx_ui_mockups_attachment
            ON ui_mockups(project_id, attached_to_external_id);

        CREATE TABLE IF NOT EXISTS ui_mockup_edit_operations (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            mockup_id INTEGER NOT NULL REFERENCES ui_mockups(id) ON DELETE CASCADE,
            sequence INTEGER NOT NULL,
            kind TEXT NOT NULL,
            target_element_id TEXT,
            payload_json TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(mockup_id, sequence)
        );

        CREATE TABLE IF NOT EXISTS ui_mockup_annotations (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            mockup_id INTEGER NOT NULL REFERENCES ui_mockups(id) ON DELETE CASCADE,
            external_id TEXT NOT NULL,
            svg_path TEXT NOT NULL,
            optional_text TEXT NOT NULL DEFAULT '',
            sort_order INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(mockup_id, external_id)
        );

        CREATE TABLE IF NOT EXISTS ui_mockup_proposals (
            mockup_id INTEGER PRIMARY KEY REFERENCES ui_mockups(id) ON DELETE CASCADE,
            base_revision INTEGER NOT NULL,
            proposed_svg TEXT NOT NULL,
            proposed_manifest_json TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS ui_mockup_preview_cache (
            mockup_id INTEGER NOT NULL REFERENCES ui_mockups(id) ON DELETE CASCADE,
            source_revision INTEGER NOT NULL,
            variant TEXT NOT NULL,
            png BLOB NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY(mockup_id, source_revision, variant)
        );

        CREATE INDEX IF NOT EXISTS idx_ui_mockups_status
            ON ui_mockups(project_id, status);

        INSERT OR IGNORE INTO schema_migrations(version) VALUES (10);",
    )?;
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
    ensure_task_state_vocabulary(db)?;

    db.execute(
        "UPDATE agent_tasks
         SET state = CASE
            WHEN state IN ('todo', 'active', 'finished', 'closed') THEN state
            WHEN state = 'done' THEN 'finished'
            ELSE 'todo'
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

/// Rewrites the task state vocabulary from `open`/`finished`/`confirmed` to
/// `todo`/`active`/`finished`/`closed`.
///
/// SQLite cannot alter a CHECK constraint in place, so the table is rebuilt. Foreign keys from
/// the task link tables must be off while the table is renamed and recreated, otherwise the
/// rename would repoint them at the legacy name that is then dropped.
///
/// Mapping: `open` becomes `todo` because every existing task started unclaimed and no stored
/// task recorded that work had begun; `confirmed` becomes `closed` because both mean "reviewed
/// and accepted". The timestamps keep their meaning, so `completed_at` and `confirmed_at` are
/// carried across unchanged.
fn ensure_task_state_vocabulary(db: &Connection) -> rusqlite::Result<()> {
    let table_sql: String = db.query_row(
        "SELECT sql FROM sqlite_master WHERE type='table' AND name='agent_tasks'",
        [],
        |row| row.get(0),
    )?;
    if !table_sql.contains("'confirmed'") && !table_sql.contains("'open'") {
        return Ok(());
    }

    db.execute_batch(
        "PRAGMA foreign_keys=OFF;
         BEGIN IMMEDIATE;

         CREATE TABLE agent_tasks_rebuilt (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            number INTEGER NOT NULL DEFAULT 0,
            title TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            state TEXT NOT NULL DEFAULT 'todo' CHECK(state IN ('todo', 'active', 'finished', 'closed')),
            completed_at TEXT,
            confirmed_at TEXT,
            completion_memo TEXT NOT NULL DEFAULT '',
            created_files TEXT NOT NULL DEFAULT '[]',
            changed_files TEXT NOT NULL DEFAULT '[]',
            confirmation_commit_id TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(project_id, number)
         );

         INSERT INTO agent_tasks_rebuilt(
            id, project_id, number, title, description, state, completed_at, confirmed_at,
            completion_memo, created_files, changed_files, confirmation_commit_id,
            created_at, updated_at
         )
         SELECT
            id, project_id, number, title, description,
            CASE state
                WHEN 'open' THEN 'todo'
                WHEN 'confirmed' THEN 'closed'
                WHEN 'finished' THEN 'finished'
                WHEN 'done' THEN 'finished'
                ELSE 'todo'
            END,
            completed_at, confirmed_at, completion_memo, created_files, changed_files,
            confirmation_commit_id, created_at, updated_at
         FROM agent_tasks;

         DROP TABLE agent_tasks;
         ALTER TABLE agent_tasks_rebuilt RENAME TO agent_tasks;

         CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_tasks_project_number
            ON agent_tasks(project_id, number);

         COMMIT;
         PRAGMA foreign_keys=ON;",
    )?;
    Ok(())
}

fn ensure_qa_system_tables(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS qa_jobs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            number INTEGER NOT NULL DEFAULT 0,
            name TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            command TEXT NOT NULL,
            working_directory TEXT NOT NULL DEFAULT '',
            shell TEXT NOT NULL DEFAULT 'powershell',
            timeout_seconds INTEGER NOT NULL DEFAULT 120,
            enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0, 1)),
            created_by TEXT NOT NULL DEFAULT 'user',
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(project_id, number)
        );

        CREATE TABLE IF NOT EXISTS qa_job_design_links (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            qa_job_id INTEGER NOT NULL REFERENCES qa_jobs(id) ON DELETE CASCADE,
            sort_order INTEGER NOT NULL DEFAULT 0,
            target_type TEXT NOT NULL CHECK(target_type IN ('element', 'relationship', 'uml')),
            design_external_id TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(qa_job_id, design_external_id)
        );

        CREATE INDEX IF NOT EXISTS idx_qa_job_design_links_job_order
            ON qa_job_design_links(qa_job_id, sort_order);

        CREATE INDEX IF NOT EXISTS idx_qa_job_design_links_target
            ON qa_job_design_links(design_external_id);

        CREATE TABLE IF NOT EXISTS qa_job_task_links (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            qa_job_id INTEGER NOT NULL REFERENCES qa_jobs(id) ON DELETE CASCADE,
            task_id INTEGER NOT NULL REFERENCES agent_tasks(id) ON DELETE CASCADE,
            sort_order INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(qa_job_id, task_id)
        );

        CREATE INDEX IF NOT EXISTS idx_qa_job_task_links_job_order
            ON qa_job_task_links(qa_job_id, sort_order);

        CREATE TABLE IF NOT EXISTS qa_job_tags (
            qa_job_id INTEGER NOT NULL REFERENCES qa_jobs(id) ON DELETE CASCADE,
            tag TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY(qa_job_id, tag)
        );

        CREATE INDEX IF NOT EXISTS idx_qa_job_tags_tag
            ON qa_job_tags(tag);

        CREATE TABLE IF NOT EXISTS qa_runs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            trigger_source TEXT NOT NULL DEFAULT 'user',
            query_snapshot TEXT NOT NULL DEFAULT '{}',
            status TEXT NOT NULL DEFAULT 'running' CHECK(status IN ('running', 'passed', 'failed')),
            started_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            finished_at TEXT,
            summary TEXT NOT NULL DEFAULT ''
        );

        CREATE TABLE IF NOT EXISTS qa_job_runs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            qa_run_id INTEGER NOT NULL REFERENCES qa_runs(id) ON DELETE CASCADE,
            qa_job_id INTEGER NOT NULL REFERENCES qa_jobs(id) ON DELETE CASCADE,
            command_snapshot TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'running' CHECK(status IN ('running', 'passed', 'failed', 'timed_out')),
            exit_code INTEGER,
            started_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            finished_at TEXT,
            duration_ms INTEGER,
            output TEXT NOT NULL DEFAULT ''
        );

        CREATE INDEX IF NOT EXISTS idx_qa_job_runs_job_latest
            ON qa_job_runs(qa_job_id, id DESC);",
    )?;

    db.execute(
        "INSERT INTO qa_jobs(project_id, number, name, description, command, enabled, created_by)
         SELECT
            q.project_id,
            ROW_NUMBER() OVER (PARTITION BY q.project_id ORDER BY q.id),
            q.label,
            'Migrated from legacy QA gate.',
            q.command,
            q.required,
            'migration'
         FROM qa_checks q
         WHERE NOT EXISTS (
            SELECT 1 FROM qa_jobs j WHERE j.project_id = q.project_id
         )",
        [],
    )?;

    db.execute(
        "INSERT OR IGNORE INTO schema_migrations(version) VALUES (9)",
        [],
    )?;
    Ok(())
}

fn table_columns(db: &Connection, table: &str) -> rusqlite::Result<Vec<String>> {
    let mut statement = db.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
    columns.collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A database written by the previous release: the old three-state vocabulary, with the
    /// link table's foreign key pointing at `agent_tasks`.
    fn legacy_database() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "PRAGMA foreign_keys=ON;
             CREATE TABLE projects (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                slug TEXT NOT NULL UNIQUE,
                repository_path TEXT,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );
             INSERT INTO projects(name, slug) VALUES('P', 'p');
             CREATE TABLE agent_tasks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                number INTEGER NOT NULL DEFAULT 0,
                title TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                state TEXT NOT NULL DEFAULT 'open' CHECK(state IN ('open', 'finished', 'confirmed')),
                completed_at TEXT,
                confirmed_at TEXT,
                completion_memo TEXT NOT NULL DEFAULT '',
                created_files TEXT NOT NULL DEFAULT '[]',
                changed_files TEXT NOT NULL DEFAULT '[]',
                confirmation_commit_id TEXT,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(project_id, number)
             );
             CREATE TABLE task_design_specification_links (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id INTEGER NOT NULL REFERENCES agent_tasks(id) ON DELETE CASCADE,
                sort_order INTEGER NOT NULL DEFAULT 0,
                target_type TEXT NOT NULL CHECK(target_type IN ('element', 'relationship', 'uml')),
                design_external_id TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(task_id, design_external_id)
             );
             INSERT INTO agent_tasks(project_id, number, title, state, completed_at, confirmed_at, completion_memo)
                VALUES(1, 1, 'Never started', 'open', NULL, NULL, ''),
                      (1, 2, 'Reported done', 'finished', '2026-01-02 03:04:05', NULL, 'first attempt'),
                      (1, 3, 'Accepted work', 'confirmed', '2026-01-02 03:04:05', '2026-01-03 04:05:06', 'accepted');
             INSERT INTO task_design_specification_links(task_id, sort_order, target_type, design_external_id)
                VALUES(2, 0, 'element', 'mcp-server');
             INSERT INTO task_design_specification_links(task_id, sort_order, target_type, design_external_id)
                VALUES(3, 0, 'uml', 'ResourceScopedConcurrencyModel');",
        )
        .unwrap();
        db
    }

    #[test]
    fn migrating_the_task_vocabulary_preserves_rows_links_and_timestamps() {
        let mut db = legacy_database();
        migrate(&mut db).unwrap();

        let rows: Vec<(i64, String, Option<String>, Option<String>, String)> = db
            .prepare(
                "SELECT number, state, completed_at, confirmed_at, completion_memo
                 FROM agent_tasks ORDER BY number",
            )
            .unwrap()
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();

        // open -> todo because no stored task recorded that work had begun.
        assert_eq!(rows[0].1, "todo");
        assert_eq!(rows[1].1, "finished");
        // confirmed -> closed: both mean reviewed and accepted.
        assert_eq!(rows[2].1, "closed");
        assert_eq!(rows[1].2.as_deref(), Some("2026-01-02 03:04:05"));
        assert_eq!(rows[2].3.as_deref(), Some("2026-01-03 04:05:06"));
        assert_eq!(rows[1].4, "first attempt");

        // The links still resolve to their tasks: the rebuilt table kept its ids.
        let links: Vec<(i64, String)> = db
            .prepare("SELECT task_id, design_external_id FROM task_design_specification_links ORDER BY id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(
            links,
            vec![
                (2, "mcp-server".to_string()),
                (3, "ResourceScopedConcurrencyModel".to_string())
            ]
        );

        // The new vocabulary is enforced by storage, so the machine and the column cannot drift.
        let error = db
            .execute(
                "INSERT INTO agent_tasks(project_id, number, title, state) VALUES(1, 99, 'x', 'confirmed')",
                [],
            )
            .unwrap_err();
        assert!(error.to_string().contains("CHECK constraint failed"), "{error}");

        // A second migration is a no-op: the table is already rebuilt.
        let before: String = db
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name='agent_tasks'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        migrate(&mut db).unwrap();
        let after: String = db
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name='agent_tasks'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn a_fresh_database_uses_the_new_vocabulary_without_a_rebuild() {
        let mut db = Connection::open_in_memory().unwrap();
        migrate(&mut db).unwrap();
        let sql: String = db
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name='agent_tasks'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(sql.contains("'todo'"), "{sql}");
        assert!(sql.contains("'closed'"), "{sql}");
        assert!(!sql.contains("'confirmed'"), "{sql}");
    }
}
