<!-- adashi:architecture:begin -->
<!-- adashi:generated revision=334 -->
# Architecture — `src-tauri/src/storage` (generated)
Generated from the Adashi design model; do not edit, change the model.
These responsibilities are already owned here: extend them, do not duplicate.

- **Project Storage Descriptor** (Component) — Optional portable .adashi/storage.json, schemaVersion 1, closed backend enum: sqlite; text…
- **Shared Storage Core** (Component) — Owns the driver-independent adashi-storage-api crate: ProjectStorage, StorageBackend, Stor…
- **SQLite Storage Adapter** (Component) — Implements every StorageFactory, StorageBackend and ReadSnapshot operation over the existi…

Bound here:
- file `src-tauri/src/storage/config.rs`
- file `src-tauri/src/storage/conformance.rs`

[Showing 3 of 3 design element(s) bound here, 14 further line(s) dropped. Retrieve the rest with the adashi_design get_scope operation.]
<!-- adashi:architecture:end -->
