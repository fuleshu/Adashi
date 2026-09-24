<!-- adashi:architecture:begin -->
<!-- adashi:generated revision=367 -->
# Architecture — `src-tauri/src/storage/text/tests` (generated)
Generated from the Adashi design model; do not edit, change the model.
These responsibilities are already owned here: extend them, do not duplicate.

- **Git Text Storage Adapter** (Component) — Version-2 contract: docs/storage-text-format.md. UTF-8/LF sorted JSON per immutable UUID p…

Boundaries crossing this folder:
- Shared Storage Core -> Git Text Storage Adapter: Selects the implemented shared text backend for a text descriptor; both clients use the sa…

Bound here:
- file `src-tauri/src/storage/text/tests/upgrade.rs`
<!-- adashi:architecture:end -->
