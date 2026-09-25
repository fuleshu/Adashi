# Adashi Rule Injection

If the Adashi MCP server is available in this workspace read the agents_template.md in the project root folder now for useage instructions.

If the Adashi MCP server is not present, unavailable, or the tool call fails because the MCP surface is not configured, continue without Adashi rule injection and mention the limitation only when it affects the requested outcome.

<!-- adashi:architecture:begin -->
<!-- adashi:generated revision=8754970345988861 -->
# Architecture — `` (generated)
Generated from the Adashi design model; do not edit, change the model.
These responsibilities are already owned here: extend them, do not duplicate.

- **Lifecycle Rule Injection API** (Component) — Lifecycle contract v2 returns one canonical injectionPrompt plus metadata-only rules and a…

Boundaries crossing this folder:
- Lifecycle Rule Injection API -> Lifecycle Rules Store: Loads enabled rule prompts by intent and lifecycle hook from
- Lifecycle Rule Injection API -> Project Memory Store: Adds fixed memory protocol and current project memory context to run.start injections from

Bound here:
- file `agents_template.md`

[Showing 1 of 1 design element(s) bound here, 2 further line(s) dropped. Retrieve the rest with the adashi_design get_scope operation.]
<!-- adashi:architecture:end -->
