<!-- adashi:architecture:begin -->
<!-- adashi:generated revision=3137752568632651 -->
# Architecture — `docs` (generated)
Generated from the Adashi design model; do not edit, change the model.
These responsibilities are already owned here: extend them, do not duplicate.

- **Shared Storage Core** (Component) — Owns the driver-independent adashi-storage-api crate: ProjectStorage, StorageBackend, Stor…
- **Storage Migration Service** (Component) — Implemented SQLite/text migration; task 22 extends to server SQL. MigrationAdapter/SourceS…
- **Git Text Storage Adapter** (Component) — Version-2 contract: docs/storage-text-format.md. UTF-8/LF sorted JSON per immutable UUID p…

Bound here:
- file `docs/project-storage.md`
- file `docs/storage-api-migration.md`

[Showing 3 of 3 design element(s) bound here, 15 further line(s) dropped. Retrieve the rest with the adashi_design get_scope operation.]
<!-- adashi:architecture:end -->
