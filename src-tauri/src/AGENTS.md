<!-- adashi:architecture:begin -->
<!-- adashi:generated revision=334 -->
# Architecture — `src-tauri/src` (generated)
Generated from the Adashi design model; do not edit, change the model.
These responsibilities are already owned here: extend them, do not duplicate.

- **Architecture Projection** (Component) — Renders bounded, deterministic managed blocks from the canonical design model into a proje…
- **Project Runtime** (Component) — Resolves local project registrations and uses the same shared storage factory as MCP. Read…

Also bound here:
Prompt Hygiene, QA Execution Runtime, Transactional Design Save API, QA MCP API, Shared Storage Core, …

Boundaries crossing this folder:
- Codex -> Adashi MCP Server: Reads memory, rules, tasks, and formal design context through

[Showing 2 of 17 design element(s) bound here, 68 further line(s) dropped. Retrieve the rest with the adashi_design get_scope operation.]
<!-- adashi:architecture:end -->
