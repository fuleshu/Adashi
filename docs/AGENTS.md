<!-- adashi:architecture:begin -->
<!-- adashi:generated revision=310 -->
# Architecture — `docs` (generated)
Generated from the Adashi design model; do not edit, change the model.
These responsibilities are already owned here: extend them, do not duplicate.

- **Shared Storage Core** (Component) — Owns ProjectStore handles, the backend factory, typed domain requests/results, common vali…

Boundaries crossing this folder:
- Shared Storage Core -> Project Revision Marker: Exposes committed change identity without granting write authority
- Shared Storage Core -> Project Storage Descriptor: Validates authoritative project selection before opening storage

Bound here:
- file `docs/project-storage.md`

[Showing 1 of 1 design element(s) bound here, 7 further line(s) dropped. Retrieve the rest with the adashi_design get_scope operation.]
<!-- adashi:architecture:end -->
