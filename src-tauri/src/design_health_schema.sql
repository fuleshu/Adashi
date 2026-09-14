-- Design-to-code correspondence records.
--
-- These tables hold no design data and no code. They hold what a deterministic scan established
-- about the model: whether each element is attached to the C4 hierarchy, and whether the files it
-- claims actually exist. The design store stays canonical; this is evidence about it.

CREATE TABLE IF NOT EXISTS design_binding_checks (
    -- One row per broken binding. Resolved bindings need no row: absence of a row is the good
    -- case, and it keeps this table proportional to the problems rather than to the model.
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    design_external_id TEXT NOT NULL,
    target_type TEXT NOT NULL,
    target TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('broken')),
    detail TEXT NOT NULL DEFAULT '',
    checked_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY(project_id, design_external_id, target_type, target)
);

CREATE INDEX IF NOT EXISTS idx_design_binding_checks_project
    ON design_binding_checks(project_id, design_external_id);

CREATE TABLE IF NOT EXISTS design_element_checks (
    -- The scan's verdict per element.
    --   unmapped — attached to the model, but no file binding: its code cannot be located.
    --   orphaned — not attached: no parent and no relationship names it.
    --   broken   — attached, with at least one file binding that does not resolve.
    --   resolved — attached, with file bindings that all resolve.
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    design_external_id TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('unmapped', 'orphaned', 'broken', 'resolved')),
    detail TEXT NOT NULL DEFAULT '',
    -- The files this element's bindings resolved to, as a JSON array of project-relative paths.
    -- Recorded so the design view can show what an element owns without re-reading the source
    -- tree: a resolved element with nothing to show is indistinguishable from a broken one.
    files TEXT NOT NULL DEFAULT '[]',
    checked_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY(project_id, design_external_id)
);

CREATE INDEX IF NOT EXISTS idx_design_element_checks_state
    ON design_element_checks(project_id, state);

CREATE TABLE IF NOT EXISTS design_health_waivers (
    -- A finding that was reviewed and knowingly kept, with the reason it was kept. The count of
    -- these is the signal: a growing list means the model has drifted out of reach.
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    design_external_id TEXT NOT NULL,
    state TEXT NOT NULL,
    reason TEXT NOT NULL,
    task_id INTEGER REFERENCES agent_tasks(id) ON DELETE SET NULL,
    created_by TEXT NOT NULL DEFAULT 'agent',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_design_health_waivers_element
    ON design_health_waivers(project_id, design_external_id);

INSERT OR IGNORE INTO schema_migrations(version) VALUES (12);
