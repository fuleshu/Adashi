<!-- adashi:architecture:begin -->
<!-- adashi:generated revision=334 -->
# Architecture — `scripts` (generated)
Generated from the Adashi design model; do not edit, change the model.
These responsibilities are already owned here: extend them, do not duplicate.

- **Shared Storage Core** (Component) — Owns the driver-independent adashi-storage-api crate: ProjectStorage, StorageBackend, Stor…
- **Dashboard UI** (Container) — React dashboard for browsing and rendering formal C4, UML, and UI mockup artifacts, plus m…

Boundaries crossing this folder:
- Dashboard UI -> Tauri Desktop Runtime: Invokes dashboard, settings, memory, rules, and narrow design edit commands through

Bound here:
- file `scripts/check_native_storage.mjs`
- file `scripts/check_read_only_storage.py`

[Showing 2 of 2 design element(s) bound here, 14 further line(s) dropped. Retrieve the rest with the adashi_design get_scope operation.]
<!-- adashi:architecture:end -->
