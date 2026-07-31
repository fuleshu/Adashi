CREATE TABLE IF NOT EXISTS resource_versions (
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    resource_kind TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 1 CHECK(version >= 1),
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY(project_id, resource_kind, resource_id)
);

CREATE INDEX IF NOT EXISTS idx_resource_versions_project_kind
    ON resource_versions(project_id, resource_kind, resource_id);

CREATE TABLE IF NOT EXISTS mutation_operations (
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    operation_id TEXT NOT NULL,
    result_json TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY(project_id, operation_id)
);

CREATE TABLE IF NOT EXISTS resource_intents (
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    agent_run_id TEXT NOT NULL,
    resource_kind TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY(project_id, agent_run_id, resource_kind, resource_id)
);

CREATE INDEX IF NOT EXISTS idx_resource_intents_live
    ON resource_intents(project_id, expires_at);

CREATE TABLE IF NOT EXISTS project_memory_notes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    note_id TEXT NOT NULL,
    operation_id TEXT NOT NULL,
    run_id TEXT NOT NULL,
    task_id INTEGER REFERENCES agent_tasks(id) ON DELETE SET NULL,
    body TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(project_id, note_id),
    UNIQUE(project_id, operation_id)
);

CREATE INDEX IF NOT EXISTS idx_project_memory_notes_project_created
    ON project_memory_notes(project_id, id);

INSERT OR IGNORE INTO resource_versions(project_id, resource_kind, resource_id)
SELECT w.project_id, 'design.element', e.external_id
FROM c4_elements e
JOIN design_workspaces w ON w.id=e.workspace_id;

INSERT OR IGNORE INTO resource_versions(project_id, resource_kind, resource_id)
SELECT w.project_id, 'design.relationship', r.external_id
FROM c4_relationships r
JOIN design_workspaces w ON w.id=r.workspace_id;

INSERT OR IGNORE INTO resource_versions(project_id, resource_kind, resource_id)
SELECT w.project_id, 'design.uml', d.key
FROM diagrams d
JOIN design_workspaces w ON w.id=d.workspace_id
WHERE d.kind='mermaid';

INSERT OR IGNORE INTO resource_versions(project_id, resource_kind, resource_id)
SELECT
    w.project_id,
    'design.binding',
    b.design_external_id || '|' || b.target_type || '|' || b.target
FROM design_bindings b
JOIN design_workspaces w ON w.id=b.workspace_id;

INSERT OR IGNORE INTO resource_versions(project_id, resource_kind, resource_id)
SELECT project_id, 'task', CAST(id AS TEXT)
FROM agent_tasks;

INSERT OR IGNORE INTO resource_versions(project_id, resource_kind, resource_id)
SELECT project_id, 'qa.job', CAST(id AS TEXT)
FROM qa_jobs;

INSERT OR IGNORE INTO resource_versions(project_id, resource_kind, resource_id)
SELECT project_id, 'rule', CAST(id AS TEXT)
FROM rules;

INSERT OR IGNORE INTO resource_versions(project_id, resource_kind, resource_id)
SELECT project_id, 'fixed-hook', key
FROM fixed_hook_prompts;

INSERT OR IGNORE INTO resource_versions(project_id, resource_kind, resource_id)
SELECT project_id, 'memory.canonical', 'canonical'
FROM project_memory;

INSERT OR IGNORE INTO resource_versions(project_id, resource_kind, resource_id)
SELECT project_id, 'memory.protocol', 'protocol'
FROM project_memory;

INSERT OR IGNORE INTO resource_versions(project_id, resource_kind, resource_id)
SELECT project_id, 'mockup.accepted', external_id
FROM ui_mockups;

INSERT OR IGNORE INTO resource_versions(project_id, resource_kind, resource_id)
SELECT project_id, 'mockup.working', external_id
FROM ui_mockups;

INSERT OR IGNORE INTO schema_migrations(version) VALUES (11);
