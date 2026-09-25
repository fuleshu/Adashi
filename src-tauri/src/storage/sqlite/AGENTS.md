<!-- adashi:architecture:begin -->
<!-- adashi:generated revision=8754970345988861 -->
# Architecture — `src-tauri/src/storage/sqlite` (generated)
Generated from the Adashi design model; do not edit, change the model.
These responsibilities are already owned here: extend them, do not duplicate.

- **Storage Migration Service** (Component) — Implemented SQLite/text migration; task 22 extends to server SQL. MigrationAdapter/SourceS…
- **SQLite Storage Adapter** (Component) — Implements every StorageFactory, StorageBackend and ReadSnapshot operation over the existi…

Boundaries crossing this folder:
- Shared Storage Core -> SQLite Storage Adapter: Creates the implemented legacy-compatible backend

Bound here:
- file `src-tauri/src/storage/sqlite/mutations.rs`

[Showing 2 of 2 design element(s) bound here, 7 further line(s) dropped. Retrieve the rest with the adashi_design get_scope operation.]
<!-- adashi:architecture:end -->
