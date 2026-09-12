# In-tree architecture projection

Status: **implemented.** Regeneration is opt-in per project; nothing is written until it is enabled.

## Problem

Agents do not pull architecture from Adashi. The formal design holds 45 described elements, 79 described relationships and 55 file/symbol bindings, but the only agent-facing surface was a bounded id index injected at `run.start` plus explicit MCP calls that are never made. On complex projects this shows up as architectural drift: agents lose the core design mid-run and build side cars — parallel mechanisms that duplicate a responsibility the model already assigns to an existing element.

The failure was a channel failure, not a storage failure. Agents' native affordance is reading the tree they are already working in. Adashi replaced push-by-proximity with pull.

## Decision

Adashi remains the canonical model. The project tree receives **generated projections** of it.

| Consumer | Medium | Source |
| --- | --- | --- |
| Human | C4 diagram, UML and mockups in the desktop app | rendered directly from the model |
| Agent | text, in the instruction files of the folders it works in | generated from the model |

One model, two renderers. Neither representation is authored by hand; only the model is.

## Delivery channel

Projections target the **instruction-file convention**, not a neutral filename, because nested instruction files are injected automatically by scope. In DeepSeek Harness (`packages/context/agent-instructions`):

- candidates default to `['AGENTS.md', 'CLAUDE.md']`, both configurable;
- files are discovered at `$DSH_HOME`, the project root, and **nested paths**;
- nested files are added as "Additional instructions from: `<path>`";
- "more specific instructions take precedence over broader ones";
- files are digest-tracked, and changes surface as "Updated instructions from: `<path>`";
- there is a **byte budget**, and overflow is resolved by **omission or truncation**, not graceful trimming.

Two consequences drive the rest of this contract: the projection must be **small**, and its **freshness is a correctness property**, because a stale folder block outranks correct root guidance.

## Artifacts

### Root projection

One managed block in the project-root instruction file. Carries only what is shared by every task:

- the Software System's purpose line;
- each Container with its responsibility (the element description truncated to one line);
- the top-layer relationships with their descriptions (the boundaries);
- a retrieval pointer for depth.

Person elements are omitted: actors are not implementable surface. Shape (illustrative):

```markdown
- **Adashi** (Software System) — local multi-project context layer for agentic coding
  workspaces: design browsing, MCP rule injection, project memory, tasks, QA evidence.
- **Adashi MCP Server** (Container) — passive stdio server exposing deterministic rule,
  memory, task, QA and formal design tools to coding agents.
- **Project Data Store** (Container) — project-local SQLite under `<project>/.adashi`,
  holding formal design, memory, rules, revision state, tasks and QA evidence.

Boundaries:
- Adashi MCP Server -> Project Data Store: reads and writes project-local resources
  through deterministic MCP tools.

These responsibilities are already owned. Extend them; do not create a parallel
mechanism for them.
```

### Per-folder projection

One managed block in `<folder>/<configured name>`, emitted **only for folders that contain at least one bound file**. Carries the local delta:

- the element(s) that own code in this folder: external id, name, type, responsibility line;
- the relationships crossing this folder's elements, with descriptions;
- the file/symbol -> element bindings present here;
- the same "already owned, extend don't duplicate" framing.

## Content rules

The projection must carry **responsibilities and boundaries**, because those are what prevent duplication. An inventory of ids and names does not, no matter how it is delivered.

It must be **shorter than the diagram, never different from it**. If the text drops relationships or truncates responsibilities, the agent reasons about a different system than the human reviews.

It must not restate things owned elsewhere (task records, QA evidence, memory notes). One home per fact: the model owns facts; the projection mirrors them.

## Budgets

| Level | Target | Hard cap |
| --- | --- | --- |
| Root block | 1.2 KB | 2 KB |
| Per-folder block | 400 B | 1 KB |

Rationale: nested instruction files compete with the user's own instructions for a harness byte budget, and overflow deletes content. A folder block that is truncated to nothing is worse than absent, because the root block then implies coverage it does not have.

Over-budget content is dropped deterministically from the end of the block (relationship lines before element lines) and the drop is stated explicitly in the block, never silent.

## Format

A single managed block, delimited so that everything outside it is untouched:

```markdown
<!-- adashi:architecture:begin -->
<!-- adashi:generated revision=42 -->
...generated content...
<!-- adashi:architecture:end -->
```

- Adashi writes only between the markers. If the file exists with user content, the block is spliced in; the rest of the file is byte-identical before and after.
- If the file does not exist, Adashi creates it containing only the block.
- The block states that it is generated from the Adashi design model and that edits belong in the model, and carries the design revision it was rendered from.
- **Byte-stable output**: elements sorted by external id, relationships sorted by external id, LF newlines, no timestamps, no wall-clock content. The revision integer is the only version marker. Re-rendering an unchanged model produces identical bytes and performs no write.

## Eligibility

- A folder qualifies when at least one **file** binding targets a path inside it. Symbol bindings have no path and never create a folder.
- Excluded directories: `target`, `node_modules`, `dist`, `build`, `out`, `.git`, `.adashi`, and any dot-directory.
- Bound paths that escape the project folder are rejected; absolute paths are normalised relative to it.
- Folder count is capped (200) and the directory walk that cleans up stale blocks is bounded (5,000 directories).
- When a folder loses its last binding, its block is removed; if Adashi created the file and nothing else remains, the file is deleted.

## Settings

Projection configuration lives in `AppSettings.architectureProjection`:

- `fileName` — the global instruction-file name, default `AGENTS.md`. One name, not a list: writing the block to both `AGENTS.md` and `CLAUDE.md` in one folder would inject it twice in harnesses that load both candidates.
- `enabledProjectIds` — the projects that opted in. Empty means nothing is written anywhere.
- `projectFileNames` — per-project overrides keyed by project id. Resolution order is the per-project override, then the global default.

A file name is accepted only when it stays inside its folder: empty, `.`, `..`, and any name containing a path separator or drive colon fall back to the default.

Two desktop commands drive this:

- `set_architecture_file_name({ fileName })` — sets the global name. Renaming removes the previous name's blocks first, so an orphaned block cannot keep being injected.
- `set_project_architecture_projection({ projectId, enabled, fileName })` — opts one project in or out, with an optional per-project override (`null` clears it).

Disabling a project removes every block Adashi wrote there and deletes files that held nothing else. Deleting a project forgets its projection configuration.

## Lifecycle

- **Generate** when a project's dashboard loads, which covers project open and every revision change, and whenever the projection settings change. Regeneration is idempotent and only writes when the rendered content actually differs.
- **Known limitation**: generation is driven by the resident desktop app. When an agent mutates the design over MCP while Adashi is closed, the next project load refreshes the projection; until then a folder block can lag the model. The alternative — writing files from the MCP save path — was rejected because a projection failure must not be reported as a design-save failure, and the MCP result has no place to report it.
- **Freshness** is read from the `revision` recorded inside each block: `current` when it matches the project revision and the text matches a fresh render, `stale` when the revision moved, `drifted` when the revision matches but the text does not, `missing` when the file or block is absent. The dashboard exposes this per file and surfaces a regeneration failure rather than silently leaving stale blocks behind.
- **Hand-edit drift** is detected without stored digests: a block whose embedded revision equals the current revision but whose text differs from a fresh render was edited outside Adashi. Regeneration restores it, so hand edits are transient rather than authoritative — this matters because the coding agent is itself the most likely actor to "fix" the file instead of the model.
- **Removal**: blocks that no longer correspond to bound design are dropped, including ones left under a previous configuration.

## Non-goals

- The database is not replaced, and markdown never becomes a source of truth.
- The MCP surface is unchanged: writes and validation stay behind the design save operation, and targeted pulls (`get_scope`, `get_bindings`) remain for precision work.
- This does not replace the `run.start` architecture brief. The two are complementary: the brief is zero-config and works in projects where projection is disabled; the projection is persistent and scope-triggered.

## Prerequisites

Both defects that undermined the design surface are fixed.

1. The design tool's input schema advertised `maxDepth`, `childrenDepth` and `limit` as required for every operation. The cause was `schema_with`: it substitutes a synthetic wrapper type for the field type, which defeats schemars' `Option` detection. `#[serde(default)]` now makes those fields optional, and a test asserts the tool schema requires only `operation` and `projectId`.
2. Injected prompts named tools by hard-coded name, so tool renames silently rotted them. `prompt_hygiene` now holds one registry of exposed tool names, rewrites stored rule and fixed-hook prompts that reference removed tools using exact identifier substitution (longest name first, so prefixes cannot shadow), and reports any reference it cannot map as a dashboard warning. The repair runs after the built-in legacy migrations so their exact-text comparisons still match.

## Implementation

| File | Responsibility |
| --- | --- |
| `src-tauri/src/projection.rs` | Rendering, managed-block splicing, budgets, freshness and status |
| `src-tauri/src/prompt_hygiene.rs` | Tool-name registry, stored-prompt repair, staleness detection |
| `src-tauri/src/settings.rs` | Projection settings, name validation, resolution, normalization |
| `src-tauri/src/desktop.rs` | The two commands, dashboard status, refresh on project load and settings change |
| `src/main.tsx`, `src/styles.css` | Settings controls for the file name and per-project opt-in, per-file freshness status, prompt warnings |

## Remaining open questions

- Should per-folder projection also follow a folder-depth limit, in addition to the directory cap and excluded-directory list it uses now?
- Should the desktop UI expose the rendered agent view verbatim, so a human can inspect exactly what agents read?
