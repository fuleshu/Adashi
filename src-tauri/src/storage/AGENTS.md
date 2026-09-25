<!-- adashi:architecture:begin -->
<!-- adashi:generated revision=3137752568632651 -->
# Architecture — `src-tauri/src/storage` (generated)
Generated from the Adashi design model; do not edit, change the model.
These responsibilities are already owned here: extend them, do not duplicate.

- **Project Storage Descriptor** (Component) — Optional portable .adashi/storage.json, schemaVersion 1, closed backend enum sqlite/text/s…
- **Shared Storage Core** (Component) — Owns the driver-independent adashi-storage-api crate: ProjectStorage, StorageBackend, Stor…

Also bound here:
Storage Migration Service, SQLite Storage Adapter.

Boundaries crossing this folder:
- Shared Storage Core -> Project Revision Marker: Exposes committed change identity without granting write authority

[Showing 2 of 4 design element(s) bound here, 16 further line(s) dropped. Retrieve the rest with the adashi_design get_scope operation.]
<!-- adashi:architecture:end -->
