<!-- adashi:architecture:begin -->
<!-- adashi:generated revision=281 -->
# Architecture — `src-tauri/src/mcp` (generated)
Generated from the Adashi design model; do not edit, change the model.
These responsibilities are already owned here: extend them, do not duplicate.

- **Lifecycle Rule Injection API** (Component) — Lifecycle contract v2 returns one canonical injectionPrompt plus metadata-only rules and a…
- **Adashi MCP Server** (Container) — Passive stdio MCP server exposing grouped rule, memory, task, QA, search, intent and forma…

Boundaries crossing this folder:
- Codex -> Adashi MCP Server: Reads memory, rules, tasks, and formal design context through

Bound here:
- file `src-tauri/src/mcp/context.rs`
- file `src-tauri/src/mcp/help.rs`

[Showing 2 of 2 design element(s) bound here, 7 further line(s) dropped. Retrieve the rest with the adashi_design get_scope operation.]
<!-- adashi:architecture:end -->
