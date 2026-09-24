<!-- adashi:architecture:begin -->
<!-- adashi:generated revision=367 -->
# Architecture — `src-tauri/src/storage/text` (generated)
Generated from the Adashi design model; do not edit, change the model.
These responsibilities are already owned here: extend them, do not duplicate.

- **Storage Migration Service** (Component) — Implemented task 18 for SQLite/text; task 22 extends to server SQL. MigrationAdapter/Sourc…
- **Git Text Storage Adapter** (Component) — Version-2 contract: docs/storage-text-format.md. UTF-8/LF sorted JSON per immutable UUID p…

Boundaries crossing this folder:
- Shared Storage Core -> Git Text Storage Adapter: Selects the implemented shared text backend for a text descriptor; both clients use the sa…

[Showing 2 of 2 design element(s) bound here, 12 further line(s) dropped. Retrieve the rest with the adashi_design get_scope operation.]
<!-- adashi:architecture:end -->
