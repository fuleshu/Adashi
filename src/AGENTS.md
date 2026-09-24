<!-- adashi:architecture:begin -->
<!-- adashi:generated revision=476204540909476 -->
# Architecture — `src` (generated)
Generated from the Adashi design model; do not edit, change the model.
These responsibilities are already owned here: extend them, do not duplicate.

- **Project Storage Descriptor** (Component) — Optional portable .adashi/storage.json, schemaVersion 1, closed backend enum sqlite/text/s…
- **First Project Onboarding View** (Component) — Blocking first-run and recovery surface shown when settings are missing, no projects are c…

Also bound here:
QA Workspace View, Dashboard UI.

Boundaries crossing this folder:
- Dashboard UI -> Tauri Desktop Runtime: Invokes dashboard, settings, memory, rules, and narrow design edit commands through

[Showing 2 of 4 design element(s) bound here, 13 further line(s) dropped. Retrieve the rest with the adashi_design get_scope operation.]
<!-- adashi:architecture:end -->
