PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS projects (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    slug TEXT NOT NULL UNIQUE,
    repository_path TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS project_state (
    project_id INTEGER PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    revision INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS design_workspaces (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    structurizr_dsl TEXT NOT NULL,
    structurizr_json TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS diagrams (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id INTEGER NOT NULL REFERENCES design_workspaces(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK(kind IN ('mermaid', 'structurizr')),
    key TEXT NOT NULL,
    title TEXT NOT NULL,
    source TEXT NOT NULL,
    diagram_type TEXT NOT NULL DEFAULT '',
    attached_to_external_id TEXT,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(workspace_id, key)
);

CREATE TABLE IF NOT EXISTS c4_elements (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id INTEGER NOT NULL REFERENCES design_workspaces(id) ON DELETE CASCADE,
    external_id TEXT NOT NULL,
    parent_external_id TEXT,
    element_type TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    technology TEXT NOT NULL DEFAULT '',
    tags TEXT NOT NULL DEFAULT '',
    UNIQUE(workspace_id, external_id)
);

CREATE TABLE IF NOT EXISTS c4_relationships (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id INTEGER NOT NULL REFERENCES design_workspaces(id) ON DELETE CASCADE,
    external_id TEXT NOT NULL,
    source_external_id TEXT NOT NULL,
    destination_external_id TEXT NOT NULL,
    description TEXT NOT NULL,
    technology TEXT NOT NULL DEFAULT '',
    tags TEXT NOT NULL DEFAULT '',
    UNIQUE(workspace_id, external_id)
);

CREATE TABLE IF NOT EXISTS design_bindings (
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
    ON design_bindings(workspace_id, target_type, target);

CREATE TABLE IF NOT EXISTS agent_tasks (
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
);

CREATE TABLE IF NOT EXISTS coding_guidelines (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    severity TEXT NOT NULL DEFAULT 'guidance',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS post_task_commands (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    label TEXT NOT NULL,
    command TEXT NOT NULL,
    trigger TEXT NOT NULL DEFAULT 'manual',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS qa_checks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    label TEXT NOT NULL,
    command TEXT NOT NULL,
    required INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS qa_jobs (
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
    ON qa_job_runs(qa_job_id, id DESC);

CREATE TABLE IF NOT EXISTS rules (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL DEFAULT '',
    enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0, 1)),
    intend TEXT NOT NULL CHECK(intend IN ('general', 'design', 'implementation')),
    hook TEXT NOT NULL CHECK(hook IN ('run.start', 'task.start', 'task.end', 'run.end')),
    prompt TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_rules_project_intend_hook
    ON rules(project_id, intend, hook, enabled);

CREATE TABLE IF NOT EXISTS fixed_hook_prompts (
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    title TEXT NOT NULL,
    intend TEXT NOT NULL CHECK(intend IN ('general', 'design', 'implementation')),
    hook TEXT NOT NULL CHECK(hook IN ('run.start', 'task.start', 'task.end', 'run.end')),
    prompt TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY(project_id, key)
);

CREATE TABLE IF NOT EXISTS project_memory (
    project_id INTEGER PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    protocol_rule TEXT NOT NULL,
    memory_body TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT OR IGNORE INTO schema_migrations(version) VALUES (1);
INSERT OR IGNORE INTO schema_migrations(version) VALUES (2);
INSERT OR IGNORE INTO schema_migrations(version) VALUES (3);
INSERT OR IGNORE INTO schema_migrations(version) VALUES (4);
INSERT OR IGNORE INTO schema_migrations(version) VALUES (5);
INSERT OR IGNORE INTO schema_migrations(version) VALUES (6);
INSERT OR IGNORE INTO schema_migrations(version) VALUES (7);
INSERT OR IGNORE INTO schema_migrations(version) VALUES (8);
INSERT OR IGNORE INTO schema_migrations(version) VALUES (9);
