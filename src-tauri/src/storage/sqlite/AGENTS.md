<!-- adashi:architecture:begin -->
<!-- adashi:generated revision=334 -->
# Architecture — `src-tauri/src/storage/sqlite` (generated)
Generated from the Adashi design model; do not edit, change the model.
These responsibilities are already owned here: extend them, do not duplicate.

- **SQLite Storage Adapter** (Component) — Implements every StorageFactory, StorageBackend and ReadSnapshot operation over the existi…

Boundaries crossing this folder:
- Shared Storage Core -> SQLite Storage Adapter: Creates the implemented legacy-compatible backend

Bound here:
- file `src-tauri/src/storage/sqlite/mutations.rs`
- file `src-tauri/src/storage/sqlite/references.rs`
- file `src-tauri/src/storage/sqlite/schema.rs`
- file `src-tauri/src/storage/sqlite/snapshot.rs`

[Showing 1 of 1 design element(s) bound here, 1 further line(s) dropped. Retrieve the rest with the adashi_design get_scope operation.]
<!-- adashi:architecture:end -->
