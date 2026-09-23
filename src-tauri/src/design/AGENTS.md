<!-- adashi:architecture:begin -->
<!-- adashi:generated revision=310 -->
# Architecture — `src-tauri/src/design` (generated)
Generated from the Adashi design model; do not edit, change the model.
These responsibilities are already owned here: extend them, do not duplicate.

- **Resource-Scoped Concurrency API** (Component) — Cross-cutting MCP mutation contract for safe parallel agents. Preconditions apply only to…

Boundaries crossing this folder:
- Resource-Scoped Concurrency API -> Resource Version and Intent Store: Validates resource-scoped preconditions and records idempotency and advisory intent facts…
- Resource-Scoped Concurrency API -> Project Revision Marker: Bumps the notification-only project change cursor after a successful committed mutation th…

[Showing 1 of 1 design element(s) bound here, 6 further line(s) dropped. Retrieve the rest with the adashi_design get_scope operation.]
<!-- adashi:architecture:end -->
