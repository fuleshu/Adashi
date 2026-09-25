<!-- adashi:architecture:begin -->
<!-- adashi:generated revision=3137752568632651 -->
# Architecture — `scripts/git_collaboration` (generated)
Generated from the Adashi design model; do not edit, change the model.
These responsibilities are already owned here: extend them, do not duplicate.

- **Git Text Storage Adapter** (Component) — Version-2 contract: docs/storage-text-format.md. UTF-8/LF sorted JSON per immutable UUID p…

Boundaries crossing this folder:
- Shared Storage Core -> Git Text Storage Adapter: Selects the implemented shared text backend for a text descriptor; both clients use the sa…

Bound here:
- file `scripts/git_collaboration/common.py`
- file `scripts/git_collaboration/conflicts.py`
- file `scripts/git_collaboration/merges.py`

[Showing 1 of 1 design element(s) bound here, 2 further line(s) dropped. Retrieve the rest with the adashi_design get_scope operation.]
<!-- adashi:architecture:end -->
