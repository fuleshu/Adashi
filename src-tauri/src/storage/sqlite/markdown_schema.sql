CREATE TABLE IF NOT EXISTS markdown_design_documents (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    external_id TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    UNIQUE(project_id, external_id)
);
CREATE TABLE IF NOT EXISTS markdown_design_links (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    document_id INTEGER NOT NULL REFERENCES markdown_design_documents(id) ON DELETE CASCADE,
    sort_order INTEGER NOT NULL,
    target_type TEXT NOT NULL CHECK(target_type IN ('element','relationship','uml','mockup','markdown')),
    design_external_id TEXT NOT NULL,
    UNIQUE(document_id, target_type, design_external_id),
    UNIQUE(document_id, sort_order)
);
CREATE INDEX IF NOT EXISTS idx_markdown_links_target ON markdown_design_links(target_type, design_external_id);
