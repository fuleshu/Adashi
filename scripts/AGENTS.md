<!-- adashi:architecture:begin -->
<!-- adashi:generated revision=476204540909476 -->
# Architecture — `scripts` (generated)
Generated from the Adashi design model; do not edit, change the model.
These responsibilities are already owned here: extend them, do not duplicate.

- **Shared Storage Core** (Component) — Owns the driver-independent adashi-storage-api crate: ProjectStorage, StorageBackend, Stor…
- **Storage Migration Service** (Component) — Implemented SQLite/text migration; task 22 extends to server SQL. MigrationAdapter/SourceS…

Also bound here:
Git Text Storage Adapter, Dashboard UI.

Boundaries crossing this folder:
- Dashboard UI -> Tauri Desktop Runtime: Invokes dashboard, settings, memory, rules, and narrow design edit commands through

[Showing 2 of 4 design element(s) bound here, 21 further line(s) dropped. Retrieve the rest with the adashi_design get_scope operation.]
<!-- adashi:architecture:end -->
