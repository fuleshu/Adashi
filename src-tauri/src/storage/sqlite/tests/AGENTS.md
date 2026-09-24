<!-- adashi:architecture:begin -->
<!-- adashi:generated revision=367 -->
# Architecture — `src-tauri/src/storage/sqlite/tests` (generated)
Generated from the Adashi design model; do not edit, change the model.
These responsibilities are already owned here: extend them, do not duplicate.

- **SQLite Storage Adapter** (Component) — Implements every StorageFactory, StorageBackend and ReadSnapshot operation over the existi…

Boundaries crossing this folder:
- Shared Storage Core -> SQLite Storage Adapter: Creates the implemented legacy-compatible backend

Bound here:
- file `src-tauri/src/storage/sqlite/tests/conformance.rs`
<!-- adashi:architecture:end -->
