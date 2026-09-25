# Markdown implementation and acceptance evidence

This is an implementation/test report for Adashi tasks 25–36, executed in order on
2026-09-25. The canonical design remains the linked Adashi owner specifications.
Each task and its complete attached design scope was read before that task started.

## Delivered behavior

| Area | Implementation and verification |
| --- | --- |
| Canonical storage | First-class `MarkdownDesignDocument` with stable identity, title, exact complete body and ordered typed associations. Shared snapshots/mutations, resource hashes, guarded transactions, receipts, no-ops, reference validation and backlinks. The same conformance function runs against SQLite and text factories. |
| SQLite and text | SQLite schema 15 and text format 2/schema 15, compatibility reads of schema 14, exact Unicode/CRLF/fences, transactional upgrades, deterministic records, UUID references, sequential numeric aliases and deletion tombstones. Existing numeric ID allocation tests remain passing. |
| Conversion and Git | SQLite↔text transfers preserve bodies, identities, associations, bindings, task/QA links, receipts and recovery safeguards. Independent-clone merges cover disjoint documents, overlaps, stale tokens, references and deletions. |
| MCP and discovery | Closed `upsert_markdown`/`delete_markdown` variants, bounded `list_markdown`, complete token-bearing reads, search, scopes, bindings, task scopes and backlinks. Real stdio clients test both adapters, concurrent writers, stale deletes, replay, no-op and write-free reads. Help and agent-template examples are executed. |
| Generated files | Opt-in complete Markdown exports, index, nearby architecture links and shared agent workflow. Stable identity-based paths, relative links, ownership manifests, collision/path checks, atomic write-if-changed, stale-output diagnostics and retry. Generated files never become canonical data. |
| Desktop | Project Documents entry, architecture associations, typed task/QA pickers and navigation, complete reads, existing EasyMDE toolbar/preview, explicit guarded saves/deletes, coherent title/body/association drafts, conflict review/reload and draft retention across navigation/project/backend changes. |
| Import | Explicit single-file preview and review, duplicate/identity/source-change checks, relative-link review, ordinary canonical save, original-file preservation, output-collision reporting and reviewed task/QA adoption. |
| Agent guidance | `agents_template.md`, on-demand MCP help and generated notices agree: agents author through MCP; humans edit canonical documents in Adashi; generated files are for discovery and reading. |

## Repeat the checks

Use an ordinary workspace shell with Rust, Node, Python, installed npm dependencies,
and Playwright available (`ADASHI_PLAYWRIGHT_PACKAGE` can point to its package).

```powershell
npm run build
cargo build --manifest-path src-tauri/Cargo.toml --bins
& scripts/check_markdown_feature.ps1 -Suite Core -SkipBuild
& scripts/check_markdown_feature.ps1 -Suite Native
```

Core runs the versioned-draft regression, 13 storage-api contract tests, the full Rust
library suite, actual MCP checks on both backends, storage-core checks, 45 write-free
MCP reads, both adapter parity suites and 19 independent-Git-clone scenarios. The
Rust library suite has 228 passing tests and five existing ignored manual/live-workspace
checks. Those ignored checks were not run against the user's live project.

Native starts fresh isolated desktop/MCP processes for both initial backends and uses
the actual Tauri WebView2 with bundled frontend assets. It also converts each fixture
to the other backend while preserving a draft. Screenshots, canonical-state checks and
timings cover task/QA navigation, conflict handling, safe preview, import, projection,
and exact MCP→editor→MCP→nearby-short→full-file round-trips. A 560,063-byte Unicode
document is opened, edited at its end, previewed and saved without truncation.

Acceptance exposed two save timing defects and added regressions: the Markdown editor
is read-only while its save is pending (tested by holding the real IPC response), and
continued typing in a versioned field never adopts a peer's guard from a late dashboard
readback. Out-of-order document reads also cannot replace a newer local save result.

## Evidence and execution limits

Final rebuilt acceptance passed. Adashi QA run **13** verifies Native (job 7), and
run **14** verifies Core (job 6). Thirteen formal owner descriptions now mark their
Markdown extension implemented on SQLite/text; the server SQL owner stays pending.

| Final evidence | Local directory under `target/` |
| --- | --- |
| Core aggregate | `markdown-acceptance/d58aa61b-e79a-441d-ba51-23173ddd7d36` |
| Native aggregate | `markdown-acceptance/3b9e1bb3-f784-4469-8b89-84d2bf2ea75c` |
| Actual stdio MCP | `markdown-mcp/1161ada2-9ba4-4148-9657-036f3461b24f` |
| Independent Git clones | `git-collaboration/5d9bb546-b5d9-473a-a7e3-9c4a17c070d6` |
| Native, initially SQLite | `native-storage/33c5fa81-353b-4eee-affd-c00387c9e7f2` |
| Native, initially text | `native-storage/2d146a36-2301-4edf-97d2-3e1c2efc801a` |

The large-document check measured open/edit times of 1711/630 ms and 1809/642 ms
respectively on this machine, including automation overhead. These are observed
acceptance timings, not a general performance guarantee.

Final SHA-256: MCP `3F7123DB975721CDBDE560FE19B587F006689E7A322976AC3628269B35892D7A`;
desktop `1F6F3D5915CAF82B859F54181B919087D7CB988753D4ADF6F47FCC6759208307`.

Successful suites write `target/markdown-acceptance/latest-Core.json` and
`latest-Native.json`. Each records the complete source path/hash set, exact binary
and frontend entry hashes, log hashes and the unique evidence directory. Verification
compares path/hash sets independently of PowerShell 5/7 sorting differences. Any source,
binary or retained log change invalidates that evidence.

QA jobs **6** (Core) and **7** (Native), linked to task **36**, verify those completed
workspace runs using `-VerifyEvidence` and retain their output in Adashi. The installed
QA host repeatedly stalled nested Node/Python process startup; execution through the
workspace runner succeeded. The QA records therefore explicitly verify executed evidence,
rather than claiming the installed host re-executed the entire suite. Earlier failed
attempts remain in QA history.

Server SQL remains unavailable and unvalidated. Its implementation, conversion and
full Markdown parity requirements remain assigned to storage tasks 20–23; the future
server acceptance gate was not removed. Text storage still materializes the project
and does not automatically resolve Git conflicts. Projection remains one-way and opt-in.

The tested executables are freshly built debug binaries. This work does not install a
release or replace the running Program Files MCP server. That older installed server
was used only to record tasks, QA and formal design status; new Markdown contracts
were tested through fresh processes in isolated fixtures. The real project's canonical
storage was not upgraded using the new binaries. Frontend build warnings about existing
large chunks and Rust dead-code warnings remain non-fatal.
