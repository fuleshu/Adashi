# Compact MCP responses and lifecycle context

Implemented for the 2026-09-08 Aworkit context-efficiency handoff.

## Lifecycle contract v2 and migration

The `adashi_rules` tool's `get_rule_injections` operation still accepts projectName, intend and hook. Its canonical executable body remains `injectionPrompt`. Apply it once even if `rules` is empty. An empty hook explicitly returns `status: "empty"`, an empty prompt, and empty section/rule arrays. No lifecycle hooks have been removed.

The response now declares `contractVersion: 2`. Clients reading only injectionPrompt continue to work. Clients consuming v1's generatedContext, memoryRule, or rules[].prompt must migrate to injectionPrompt plus sections. Those redundant body fields are removed rather than filled with misleading empty placeholders. Refresh cached tool schemas when adopting the rebuilt server. Task-list consumers must likewise adopt the v2 summary/page shape and use the `adashi_tasks` get operation for full records.

Each section has a stable id and kind, an optional underlying resource version, a contentVersion, and UTF-8 startByte/endByte offsets into injectionPrompt (exclusive end). contentVersion is a version-prefixed deterministic FNV-1a 64-bit fingerprint of that exact rendered section, not an authorization or cryptographic hash. Decode ranges as UTF-8, not JavaScript UTF-16 indices. Scope caches by projectName, intend and hook as well as section id. An unchanged fingerprint allows reusing a section only while its earlier instruction remains in the active model context. After compaction or context loss, reinject required content. Removed/disabled sections are absent from the complete current section inventory; remove their cached applicability. A changed section must be applied, even if other sections are unchanged.

Every tool addresses a project by `projectName`: a configured project name, compared case-insensitively, or a project id. `projectId` is removed outright, with no alias and no deprecation window, so a caller still sending it fails as an unknown field instead of silently selecting a project. Project names are unique case-insensitively and are displayed exactly as written; resolution by name fails loudly when a reference is ambiguous rather than choosing one.

MCP's standard structured-result compatibility text still mirrors structuredContent for text-only clients. Each representation contains a single canonical prompt, without internal duplication. A structured-aware client must consume one representation. Aworkit's actual model_result adapter was exercised against these responses and preserved the exact structured value while removing only the equivalent text mirror. Required custom rules/protocols are never truncated to meet informational budgets.

## Task summaries and complete enumeration

The `adashi_tasks` list operation selects only id, number, title, titleTruncated, state and version from SQL. Titles are bounded to 240 Unicode characters; titleTruncated identifies an abbreviated title. The full title, description, completion memo, file lists and linked design remain available via the `adashi_tasks` get operation.

- states omitted returns todo, active and finished, so closed history stays out of the way; closedHidden reports how many closed tasks that default withheld. Pass ["closed"] or all four values to include them, or [] to select nothing.
- Supported schema enum values are exactly todo, active, finished and closed, and the enum is published in the tool schema so a client can validate before calling. Unknown values, including the removed open/confirmed vocabulary and uppercase alternatives, fail explicitly with an error that names the accepted values and the default. Repeated valid values are normalized.
- limit defaults to 25 and must be 1 through 100.
- filteredTotal is the count of all matching tasks, including those on preceding/following pages. hasMore and nextCursor describe continuation. A zero-match query has filteredTotal=0, tasks=[], hasMore=false and nextCursor=null.
- Follow nextCursor with the same project and states. Pages are ordered by stable task id and read under a database transaction. A cursor includes the resolved filter and project revision; any intervening project change produces tasks.stale_cursor rather than silent omissions. Restart enumeration without a cursor. Do not guess or edit cursors.
- There is no implicit fallback from an empty open-task query to all states.

The task lifecycle is todo (created, unclaimed) -> active (being worked on) -> finished (reported complete) -> closed (reviewed and accepted). Closing is its own operation, so the review verdict is never an agent's own claim, and only finished work can be closed. Reopening is an update to active, which clears completedAt while keeping the completion memo and file lists as evidence for the next attempt; todo cannot be re-entered.

A task read returns its design specification links as metadata: `scope` and `mockup` stay null unless the caller passes `includeDesignScopes: true`, and the result states how to obtain them either way. On a real 19-link task that read is 24 KB instead of 284 KB, and it matches the design guide's own advice to retrieve the branch you are working on rather than every linked branch at once.

## QA listings and targeted evidence

The `adashi_qa` list_jobs operation returns bounded job metadata only: id, version, number, name, enabled, derivedState, tags and the latest run's status, exit code, timestamps and duration. It never inlines command, commandSnapshot, output or runHistory. It accepts limit (default 25, range 1..=100) and a validated nextCursor continuation over the derived-state-then-number order; filteredTotal and hasMore distinguish a complete empty result from a partial page, and a stale or filter-mismatched cursor fails explicitly. The list_runs operation likewise returns run metadata plus per-job status, exit code and duration, never console output.

Console evidence is explicit and targeted: get_job returns one job with its latest run and retained history, and get_run returns one run with all of its per-job output. Per-job-run output stays capped at 200,000 characters and only the two newest runs per job are retained, so a targeted detail read remains bounded while a listing stays small regardless of how much the suite printed.

## Separate startup and retention budgets

The required memory protocol is a separate section and is supplied in full. Default memoryContext=summary supplies at most 2,000 Unicode characters of current summary plus a small retrieval notice, never historical handovers. An oversized summary is omitted in full, with explicit instructions to retrieve it before work needing project constraints. It is never silently clipped. memoryContext=protocolOnly omits that summary section for operational requests. General intent does not imply that memory is irrelevant.

The `adashi_memory` get operation returns the current summary/protocol and a bounded selection of retained notes. Query is a literal case-insensitive substring of note bodies; noteId, runId and taskId are exact filters, combined with AND. Omitted filters select all active notes. retainedNotes counts all retained notes, including resolved history; matchedNotes counts the complete selected subset. includeSuperseded=true returns resolved notes with supersededByVersion. Historical reports are not authoritative current state.

## Grep-shaped project access

The `adashi_grep` tool is the pull channel over project content: design (C4 elements, relationships, diagrams, mockups, bindings), tasks, and the shared memory summary and note bodies. QA jobs and run evidence, lifecycle rules, fixed-hook prompts and the memory protocol rule are project tooling and are not searchable; QA run output is also the one field that could detonate a context window.

The pattern is tolerant in shape and deterministic in meaning: matching is always case-insensitive, whitespace-separated terms are AND, a quoted phrase matches as an exact substring, `key:value` clauses filter on in/file/type/state/limit, and an unrecognised key becomes a literal term rather than an error. A malformed value on a recognised key fails explicitly. The only other failure is a project reference that resolves to nothing. An empty pattern returns the top-layer overview with counts instead of a dump.

The output is budgeted to 8,192 UTF-8 bytes: the header states the true total and the per-domain split, one match per line follows, and an over-budget or over-limit result ends with `showing N of M — narrow the query`. Each line is a drillable locator (`design:<external_id>`, `task:<id>`, `memory:<note_id>`) plus a window around the matched text with context on each side, never cut mid-word and never the head of the field. Artifact sources (Structurizr DSL, Mermaid source, mockup SVG) may be matched and are only ever returned as a short window. Ordering is domain, then field weight (name and title before description and body before bindings and file lists), then locator: there is no ranked relevance, so the same query always produces the same output. Text repeated across artifacts is returned once, at its best locator.

The `file` filter applies to design bindings and to task created/changed file lists, so grepping a file path surfaces the component that owns it and the tasks that touched it.

Storage retains at most 20 notes and 12,000 Unicode body characters in total, with separate new-write limits of 4,000 summary characters and 1,000 note characters. New oversized writes fail, allowing an agent to write a complete handover. Legacy repair uses complete sentence/paragraph boundaries and explicit omissions, not SQL substr or an invented semantic summary. Previously lost text cannot be recovered by this migration. User-edited memory protocols are preserved; only exact old built-ins migrate.

Only an authorized coordinator may use the `adashi_memory` update operation with expectedVersion to replace the current summary and optionally resolve an exact list of reviewed supersededNoteIds. Unknown or duplicate ids fail atomically. The new summary version records each resolution; original note text, run, task, timestamp and operation id remain available within retention. Concurrent new notes are not implicitly covered by a review and remain active. Repeated operations do not reapply a stale summary; stale versions fail. Retention still counts resolved notes and removes their resolution rows when they expire.

## Formal design retrieval

Design and implementation startup include the complete configured fixed guidance and a metadata-only index, bounded to 3,000 UTF-8 bytes. The SQL projection reads ids, names, parent ids, element types and versions for at most 32 entries, and reports included/total counts. Oversized rows are omitted whole; retrieval ids are never truncated. One explanation covers omitted descriptions, relationships, bindings, artifacts and source.

Select relevant file/symbol bindings, explicit branches or artifacts through the `adashi_design` get_bindings, get_scope and get_by_ids operations. The search operation supplies ids absent from the startup index. The server does not infer the task's meaning. UML types remain class, sequence, flow and state; UI mockups remain separate. Exact old built-in fixed prompts migrate once with version/revision changes; custom prompts survive unchanged.

## Validation and measurements

The saved fixture is tests/fixtures/context-efficiency.json: 75 completed tasks with long descriptions/evidence, 14 historical handovers, no current summary, and 70 verbose design elements. Before and after use the same fixture and isolated settings/databases. No production task or design data is used by the regression script.

| Request | Before complete JSON-RPC bytes | After complete JSON-RPC bytes | Before Aworkit model bytes | After Aworkit model bytes |
| --- | ---: | ---: | ---: | ---: |
| General run.start | 51,947 | 3,929 | 25,897 | 1,923 |
| Design run.start | 116,184 | 11,138 | 57,848 | 5,297 |
| Implementation run.start | 116,916 | 11,578 | 58,204 | 5,505 |
| Open tasks, zero matches | 264 | 416 | 122 | 194 |
| All states, default first page | 1,849,110 | 5,682 | 922,070 | 2,626 |

The empty response grows slightly to make completeness explicit. The all-states comparison is the default request: v1 returned all 75 full records; v2 returns 25 summaries plus a cursor. The regression follows every page and separately verifies full detail retrieval. These are complete serialized UTF-8 byte measurements, not token counts or billed-token savings.

Validation covers every intend/hook, exact instruction preservation, stable section versions and ranges, complete empty results, all-task enumeration without duplicates/omissions, invalid filters/limits/cursors, stale/filter-mismatched cursors, detail retrieval, explicit history filters, versioned note supersession and provenance, oversized-summary omission, untruncated custom protocols, legacy Unicode boundaries, migration rollback, custom fixed prompts, and bounded title projection without detail hydration.

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib
cargo build --manifest-path src-tauri/Cargo.toml --release --bin adashi-mcp --no-default-features
python scripts/check_context_efficiency.py --binary src-tauri/target/release/adashi-mcp.exe --output target/context-efficiency/after.json --verify
```

For supported-client checks, the optional --model-projector executable reads a CallToolResult JSON on stdin and writes the client's model-facing JSON. The measured harness compiles Aworkit's unchanged desktop/src-tauri/src/runtime/mcp_tools/result.rs directly. This verifies the production result adapter, not a full desktop conversation or provider billing.

## Existing Aworkit memory review

docs/context-efficiency-aworkit-memory-review.json records the proposed current summary and exact 14 reviewed note ids. It resolves intermediate/routine reports and the superseded absence-of-compaction finding, retaining their historical provenance. Source checks found the current compaction implementation and its specification; the older absence statement in docs/workspace-instructions.md is itself stale.

Applying this separate project's data cleanup requires explicit user approval. scripts/apply_memory_review.py creates a SQLite backup before opening that project with the rebuilt MCP, verifies the expected summary version, applies the exact plan through the `adashi_memory` update operation, and verifies preserved note provenance. The code fix does not automatically rewrite project-specific facts.

Applied after explicit user approval on 2026-09-08. The summary was tightened to 563 characters so the original 14 notes all fit within retention (11,962 total characters); the application script now refuses plans that would evict retained notes and verifies every original note remains. Aworkit's canonical summary is version 2, with all 14 reviewed notes resolved and their text/provenance unchanged. The pre-cleanup backup is target/context-efficiency/aworkit-before-memory-review.sqlite3. Independent read-only verification confirmed database integrity and unchanged task, design and optional-rule data; the result is recorded in target/context-efficiency/aworkit-memory-review-verification.json.

## Activation

Adashi's own project memory was also cleaned after explicit user approval on 2026-09-08. docs/context-efficiency-adashi-memory-review.json records the reviewed plan. Canonical summary version 3 contains 1,651 characters, fits startup, and replaces contradictory task status and transient build/process history with current constraints. All three handovers are resolved with their original text/provenance unchanged; total retained memory is 3,966 characters. Fresh MCP checks verified that startup includes the summary exactly once and excludes historical notes. Database integrity and unchanged task/design/optional-rule data were independently checked. The backup and verification report are target/context-efficiency/adashi-before-memory-review.sqlite3 and target/context-efficiency/adashi-memory-review-verification.json.

The rebuilt server is src-tauri/target/release/adashi-mcp.exe. Existing MCP processes keep their previous executable/schema until restarted. This change does not replace the running Program Files installation or disconnect other agents. Point a client at the rebuilt executable (or install an updated package) and reconnect. Database migrations preserve existing task detail, optional rules, custom fixed prompts and version checks.
