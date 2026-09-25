<!-- adashi:architecture:begin -->
<!-- adashi:generated revision=8754970345988861 -->
# Architecture — `src-tauri/src/storage/migration` (generated)
Generated from the Adashi design model; do not edit, change the model.
These responsibilities are already owned here: extend them, do not duplicate.

- **Storage Migration Service** (Component) — Implemented SQLite/text migration; task 22 extends to server SQL. MigrationAdapter/SourceS…

Boundaries crossing this folder:
- Storage Migration Service -> Project Storage Descriptor: Activates validated destinations with writer coordination
- Storage Migration Service -> Shared Storage Core: Exports/imports verified snapshots through typed adapters

Bound here:
- file `src-tauri/src/storage/migration/journal.rs`

[Showing 1 of 1 design element(s) bound here, 3 further line(s) dropped. Retrieve the rest with the adashi_design get_scope operation.]
<!-- adashi:architecture:end -->
