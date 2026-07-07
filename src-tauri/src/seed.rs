use crate::settings::ProjectSettings;
use rusqlite::{params, Connection};

pub fn seed_initial_data(db: &mut Connection, project: &ProjectSettings) -> rusqlite::Result<()> {
    let project_count: i64 = db.query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))?;
    if project_count > 0 {
        return Ok(());
    }

    let tx = db.transaction()?;
    tx.execute(
        "INSERT INTO projects(name, slug, repository_path) VALUES (?1, ?2, ?3)",
        params![project.name, project.id, project.folder],
    )?;
    let project_id = tx.last_insert_rowid();

    tx.execute(
        "INSERT INTO project_state(project_id) VALUES (?1)",
        params![project_id],
    )?;

    tx.execute(
        "INSERT INTO project_memory(project_id, protocol_rule, memory_body) VALUES (?1, ?2, '')",
        params![project_id, crate::memory::DEFAULT_MEMORY_RULE],
    )?;

    tx.execute(
        "INSERT INTO design_workspaces(project_id, name, description, structurizr_dsl, structurizr_json)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            project_id,
            format!("{} Architecture Workspace", project.name),
            "C4 and UML design workspace for Codex-driven project planning and execution.",
            STRUCTURIZR_DSL,
            STRUCTURIZR_JSON,
        ],
    )?;
    let workspace_id = tx.last_insert_rowid();

    tx.execute(
        "INSERT INTO diagrams(workspace_id, kind, key, title, source, diagram_type, attached_to_external_id, sort_order)
         VALUES (?1, 'structurizr', 'AdashiContainers', 'Adashi container view', ?2, 'container', '2', 1)",
        params![workspace_id, STRUCTURIZR_JSON],
    )?;
    tx.execute(
        "INSERT INTO diagrams(workspace_id, kind, key, title, source, diagram_type, attached_to_external_id, sort_order)
         VALUES (?1, 'mermaid', 'TaskLifecycle', 'Agent task lifecycle', ?2, 'sequence', '5', 2)",
        params![workspace_id, MERMAID_UML],
    )?;

    for element in C4_ELEMENTS {
        tx.execute(
            "INSERT INTO c4_elements(workspace_id, external_id, parent_external_id, element_type, name, description, technology, tags)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                workspace_id,
                element.external_id,
                element.parent_external_id,
                element.element_type,
                element.name,
                element.description,
                element.technology,
                element.tags,
            ],
        )?;
    }

    for relationship in C4_RELATIONSHIPS {
        tx.execute(
            "INSERT INTO c4_relationships(workspace_id, external_id, source_external_id, destination_external_id, description, technology, tags)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                workspace_id,
                relationship.external_id,
                relationship.source_external_id,
                relationship.destination_external_id,
                relationship.description,
                relationship.technology,
                relationship.tags,
            ],
        )?;
    }

    tx.execute(
        "INSERT INTO agent_tasks(project_id, number, title, description, state) VALUES (?1, 1, ?2, ?3, 'open')",
        params![
            project_id,
            "Expose design workspace over MCP",
            "Add rmcp server resources for C4 workspaces, Mermaid diagrams, task injection, and QA gates.",
        ],
    )?;
    tx.execute(
        "INSERT INTO agent_tasks(project_id, number, title, description, state) VALUES (?1, 2, ?2, ?3, 'open')",
        params![
            project_id,
            "Add task injection workflow",
            "Persist coding agent tasks with context, acceptance criteria, and post-task commands.",
        ],
    )?;

    tx.execute(
        "INSERT INTO coding_guidelines(project_id, title, body, severity) VALUES (?1, ?2, ?3, 'required')",
        params![
            project_id,
            "Design data is source-controlled",
            "Persist design intent as DSL/JSON alongside database rows so agents can diff and review architecture changes.",
        ],
    )?;
    tx.execute(
        "INSERT INTO coding_guidelines(project_id, title, body, severity) VALUES (?1, ?2, ?3, 'required')",
        params![
            project_id,
            "MCP operations are explicit",
            "Task execution tools should expose intent, inputs, and expected verification before running project commands.",
        ],
    )?;

    tx.execute(
        "INSERT INTO post_task_commands(project_id, label, command, trigger) VALUES (?1, ?2, ?3, 'manual')",
        params![project_id, "Rust check", "cargo check --manifest-path src-tauri/Cargo.toml"],
    )?;
    tx.execute(
        "INSERT INTO post_task_commands(project_id, label, command, trigger) VALUES (?1, ?2, ?3, 'manual')",
        params![project_id, "Frontend build", "npm run build"],
    )?;

    tx.execute(
        "INSERT INTO qa_checks(project_id, label, command, required) VALUES (?1, ?2, ?3, 1)",
        params![
            project_id,
            "Tauri compile check",
            "cargo check --manifest-path src-tauri/Cargo.toml"
        ],
    )?;
    tx.execute(
        "INSERT INTO qa_checks(project_id, label, command, required) VALUES (?1, ?2, ?3, 1)",
        params![project_id, "TypeScript build", "npm run build"],
    )?;

    tx.commit()
}

struct ElementSeed {
    external_id: &'static str,
    parent_external_id: Option<&'static str>,
    element_type: &'static str,
    name: &'static str,
    description: &'static str,
    technology: &'static str,
    tags: &'static str,
}

struct RelationshipSeed {
    external_id: &'static str,
    source_external_id: &'static str,
    destination_external_id: &'static str,
    description: &'static str,
    technology: &'static str,
    tags: &'static str,
}

const C4_ELEMENTS: &[ElementSeed] = &[
    ElementSeed {
        external_id: "1",
        parent_external_id: None,
        element_type: "Person",
        name: "Codex",
        description: "Coding agent that reads design context and executes project tasks.",
        technology: "",
        tags: "Element,Person",
    },
    ElementSeed {
        external_id: "2",
        parent_external_id: None,
        element_type: "Software System",
        name: "Adashi",
        description:
            "Senior agent dashboard for architecture, task injection, and QA orchestration.",
        technology: "",
        tags: "Element,Software System",
    },
    ElementSeed {
        external_id: "3",
        parent_external_id: Some("2"),
        element_type: "Container",
        name: "Tauri Desktop",
        description: "Rust-backed desktop shell and command surface.",
        technology: "Rust, Tauri",
        tags: "Element,Container",
    },
    ElementSeed {
        external_id: "4",
        parent_external_id: Some("2"),
        element_type: "Container",
        name: "Dashboard UI",
        description: "React dashboard for design views, task queues, guidelines, and QA gates.",
        technology: "React, TypeScript",
        tags: "Element,Container",
    },
    ElementSeed {
        external_id: "5",
        parent_external_id: Some("2"),
        element_type: "Container",
        name: "MCP Server",
        description: "Protocol surface that exposes design and execution operations to Codex.",
        technology: "rmcp",
        tags: "Element,Container",
    },
    ElementSeed {
        external_id: "6",
        parent_external_id: Some("2"),
        element_type: "Container",
        name: "Design Store",
        description:
            "Local SQL store for projects, workspaces, diagrams, tasks, commands, and QA checks.",
        technology: "SQLite",
        tags: "Element,Container,Database",
    },
    ElementSeed {
        external_id: "7",
        parent_external_id: Some("2"),
        element_type: "Container",
        name: "Diagram Viewers",
        description: "Structurizr UI and Mermaid rendering surfaces.",
        technology: "Structurizr UI, Mermaid.js",
        tags: "Element,Container",
    },
];

const C4_RELATIONSHIPS: &[RelationshipSeed] = &[
    RelationshipSeed {
        external_id: "10",
        source_external_id: "1",
        destination_external_id: "5",
        description: "Reads architecture context and injects tasks through",
        technology: "MCP",
        tags: "Relationship",
    },
    RelationshipSeed {
        external_id: "11",
        source_external_id: "4",
        destination_external_id: "3",
        description: "Invokes commands through",
        technology: "Tauri IPC",
        tags: "Relationship",
    },
    RelationshipSeed {
        external_id: "12",
        source_external_id: "3",
        destination_external_id: "6",
        description: "Persists dashboard and design data in",
        technology: "SQL",
        tags: "Relationship",
    },
    RelationshipSeed {
        external_id: "13",
        source_external_id: "5",
        destination_external_id: "6",
        description: "Queries and updates agent-facing resources in",
        technology: "SQL",
        tags: "Relationship",
    },
    RelationshipSeed {
        external_id: "14",
        source_external_id: "4",
        destination_external_id: "7",
        description: "Displays C4 and UML diagrams with",
        technology: "Browser APIs",
        tags: "Relationship",
    },
];

const STRUCTURIZR_DSL: &str = r#"workspace "Adashi" "Senior agent dashboard for architecture and task execution." {
    model {
        codex = person "Codex" "Coding agent that reads design context and executes project tasks."
        adashi = softwareSystem "Adashi" "Senior agent dashboard for architecture, task injection, and QA orchestration." {
            desktop = container "Tauri Desktop" "Rust-backed desktop shell and command surface." "Rust, Tauri"
            ui = container "Dashboard UI" "React dashboard for design views, task queues, guidelines, and QA gates." "React, TypeScript"
            mcp = container "MCP Server" "Protocol surface that exposes design and execution operations to Codex." "rmcp"
            store = container "Design Store" "Local SQL store for projects, workspaces, diagrams, tasks, commands, and QA checks." "SQLite" "Database"
            viewers = container "Diagram Viewers" "Structurizr UI and Mermaid rendering surfaces." "Structurizr UI, Mermaid.js"
        }

        codex -> mcp "Reads architecture context and injects tasks through" "MCP"
        ui -> desktop "Invokes commands through" "Tauri IPC"
        desktop -> store "Persists dashboard and design data in" "SQL"
        mcp -> store "Queries and updates agent-facing resources in" "SQL"
        ui -> viewers "Displays C4 and UML diagrams with" "Browser APIs"
    }

    views {
        container adashi "AdashiContainers" {
            include *
            autolayout lr
        }
    }
}"#;

const STRUCTURIZR_JSON: &str = r##"{
  "id": 1,
  "name": "Adashi",
  "description": "Senior agent dashboard for architecture and task execution.",
  "model": {
    "people": [
      {
        "id": "1",
        "tags": "Element,Person",
        "name": "Codex",
        "description": "Coding agent that reads design context and executes project tasks.",
        "relationships": [
          {
            "id": "10",
            "tags": "Relationship",
            "sourceId": "1",
            "destinationId": "5",
            "description": "Reads architecture context and injects tasks through",
            "technology": "MCP"
          }
        ],
        "location": "Internal",
        "type": "Person",
        "canonicalName": "/Codex"
      }
    ],
    "softwareSystems": [
      {
        "id": "2",
        "tags": "Element,Software System",
        "name": "Adashi",
        "description": "Senior agent dashboard for architecture, task injection, and QA orchestration.",
        "relationships": [],
        "location": "Internal",
        "containers": [
          {
            "id": "3",
            "tags": "Element,Container",
            "name": "Tauri Desktop",
            "description": "Rust-backed desktop shell and command surface.",
            "technology": "Rust, Tauri",
            "relationships": [
              {
                "id": "12",
                "tags": "Relationship",
                "sourceId": "3",
                "destinationId": "6",
                "description": "Persists dashboard and design data in",
                "technology": "SQL"
              }
            ],
            "type": "Container",
            "canonicalName": "/Adashi/Tauri Desktop",
            "parentId": "2"
          },
          {
            "id": "4",
            "tags": "Element,Container",
            "name": "Dashboard UI",
            "description": "React dashboard for design views, task queues, guidelines, and QA gates.",
            "technology": "React, TypeScript",
            "relationships": [
              {
                "id": "11",
                "tags": "Relationship",
                "sourceId": "4",
                "destinationId": "3",
                "description": "Invokes commands through",
                "technology": "Tauri IPC"
              },
              {
                "id": "14",
                "tags": "Relationship",
                "sourceId": "4",
                "destinationId": "7",
                "description": "Displays C4 and UML diagrams with",
                "technology": "Browser APIs"
              }
            ],
            "type": "Container",
            "canonicalName": "/Adashi/Dashboard UI",
            "parentId": "2"
          },
          {
            "id": "5",
            "tags": "Element,Container",
            "name": "MCP Server",
            "description": "Protocol surface that exposes design and execution operations to Codex.",
            "technology": "rmcp",
            "relationships": [
              {
                "id": "13",
                "tags": "Relationship",
                "sourceId": "5",
                "destinationId": "6",
                "description": "Queries and updates agent-facing resources in",
                "technology": "SQL"
              }
            ],
            "type": "Container",
            "canonicalName": "/Adashi/MCP Server",
            "parentId": "2"
          },
          {
            "id": "6",
            "tags": "Element,Container,Database",
            "name": "Design Store",
            "description": "Local SQL store for projects, workspaces, diagrams, tasks, commands, and QA checks.",
            "technology": "SQLite",
            "relationships": [],
            "type": "Container",
            "canonicalName": "/Adashi/Design Store",
            "parentId": "2"
          },
          {
            "id": "7",
            "tags": "Element,Container",
            "name": "Diagram Viewers",
            "description": "Structurizr UI and Mermaid rendering surfaces.",
            "technology": "Structurizr UI, Mermaid.js",
            "relationships": [],
            "type": "Container",
            "canonicalName": "/Adashi/Diagram Viewers",
            "parentId": "2"
          }
        ],
        "type": "Software System",
        "canonicalName": "/Adashi"
      }
    ]
  },
  "views": {
    "containerViews": [
      {
        "softwareSystemId": "2",
        "key": "AdashiContainers",
        "description": "Container view for the first Adashi iteration.",
        "elements": [
          { "id": "1", "x": 70, "y": 260, "width": 450, "height": 300, "relationships": ["10"] },
          { "id": "3", "x": 720, "y": 130, "width": 450, "height": 300, "relationships": ["12"] },
          { "id": "4", "x": 720, "y": 520, "width": 450, "height": 300, "relationships": ["11", "14"] },
          { "id": "5", "x": 1360, "y": 130, "width": 450, "height": 300, "relationships": ["13"] },
          { "id": "6", "x": 1980, "y": 320, "width": 450, "height": 300, "relationships": [] },
          { "id": "7", "x": 1360, "y": 520, "width": 450, "height": 300, "relationships": [] }
        ],
        "animations": [],
        "automaticLayout": {
          "implementation": "Dagre",
          "rankDirection": "LeftRight",
          "rankSeparation": 300,
          "nodeSeparation": 300,
          "edgeSeparation": 50,
          "vertices": false
        }
      }
    ],
    "configuration": {
      "defaultView": "AdashiContainers",
      "styles": {
        "elements": [
          { "tag": "Person", "shape": "Person", "background": "#2f6f6d", "color": "#ffffff" },
          { "tag": "Software System", "background": "#335c67", "color": "#ffffff" },
          { "tag": "Container", "background": "#fffaf0", "color": "#1f2933", "stroke": "#2f6f6d" },
          { "tag": "Database", "shape": "Cylinder", "background": "#e4b363", "color": "#1f2933" }
        ],
        "relationships": [
          { "tag": "Relationship", "color": "#47615f", "thickness": 3 }
        ]
      }
    }
  }
}"##;

const MERMAID_UML: &str = r#"sequenceDiagram
    autonumber
    participant Codex
    participant MCP as Adashi MCP Server
    participant DB as Design Store
    participant UI as Dashboard UI
    participant QA as QA Gates

    Codex->>MCP: request design context
    MCP->>DB: load C4 workspace, UML, guidelines
    DB-->>MCP: architecture and task metadata
    MCP-->>Codex: resources and task tools
    Codex->>MCP: inject coding task
    MCP->>DB: persist task and acceptance criteria
    UI->>DB: refresh task queue
    Codex->>QA: run required checks
    QA-->>MCP: verification result
"#;
