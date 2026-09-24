# Project storage contract

Status: tasks 12 (design), 13 (foundation), 24 (complete API) and 14 (SQLite adapter) implemented.
The compiled, backend-neutral API is in
[adashi-storage-api](../src-tauri/storage-api/src/api.rs); the exact operation map
and SQLite migration checklist are in [storage-api-migration.md](storage-api-migration.md).

The execution order is **12 → 13 → 24 → 14 → 15 → 16 → 17 → 18 → 19 →
20 → 21 → 22 → 23**. Task 14 implements SQLite against the full API and migrates
production callers. Tasks 16–19 deliver text storage; tasks 20–23 deliver server
SQL. ProjectStore now implements the full ProjectStorage interface through the
complete SQLite backend. The earlier RuleStorage subset exists only in historical
tests; the production connection bridge is removed.

## Ownership and configuration

Storage remains one logical store per project. Desktop and MCP use the same Rust
factory, domain operations, validation and backend implementations. Client handlers
translate input/output; they do not select their own storage implementation.

`ProjectSettings { id, name, folder }` remains a **local registration**. Its ID and
display name can differ from the canonical identity inside a copied project. The
existing `projects.slug` and resource IDs remain authoritative for legacy SQLite.
Opening a copied store must not rename it to match its local registration.

The optional UTF-8 `.adashi/storage.json` is the authoritative, portable backend
descriptor. Version 1 has a closed schema:

```json
{
  "schemaVersion": 1,
  "backend": { "kind": "sqlite" }
}
```

The other reserved selections are `{"kind":"text"}` and
`{"kind":"serverSql","connectionProfile":"team","namespace":"project-id"}`.
These selections are recognized but return `storage.backend_unavailable` until
their adapters ship. The profile is an opaque local configuration lookup name,
not a connection string or credential. Its eventual driver/credential resolution
belongs to task 20. Do not infer an SQL engine from this descriptor.

Resolution is deterministic:

1. Resolve the user's explicit local project registration.
2. Read `.adashi/storage.json`, without creating files or rewriting settings.
3. A missing descriptor selects the existing `.adashi/adashi.sqlite3` behavior.
4. A present descriptor must parse completely and have a supported schema version.
   Unknown fields, duplicate fields, invalid field combinations and unreadable
   files are errors, never grounds to fall back to SQLite.
5. Validate backend availability **before** creating directories, seeding, opening
   a database or resolving secrets. Resolve a server profile only for a supported
   server adapter. An unavailable backend must leave project bytes unchanged.

There is no second backend value in app settings and no environment-variable
override. Different projects can select different backends. New descriptors are
written only by an explicit initialization or migration workflow; reads do not
materialize defaults. Version-1 SQLite deliberately keeps its established path.
Custom paths can be added through an explicit future descriptor revision.

The format version, backend data-schema version and application version are
different concepts. Unsupported future schemas fail before writes. Backend
migrations are transactional/recoverable and never replay during ordinary reads.

## Data inventory and ownership

| Domain / current owner | Authoritative data to preserve | Adapter operations / boundaries |
|---|---|---|
| `project.rs`, `seed.rs`, `schema.rs`, `state.rs` | Project identity, initialization/schema state | Open/initialize, identity snapshot, schema compatibility, notification cursor |
| `design.rs`, `design/` | C4 elements/relationships, typed UML, bindings, editable source documents | Read documents/scopes/search; validate and save atomic changes with document readTokens |
| `design_workspaces`, `diagrams` | Canonical workspace source and imported artifacts | Preserve source provenance; regenerate derived Structurizr views from the chosen canonical model |
| `mockups.rs` | Accepted SVG, recoverable working drafts, annotations, edit logs, proposals, base/working/accepted revisions | Read revision context, save draft/proposal, accept/reject/delete atomically |
| `tasks.rs` | Task identity/number/state, descriptions, ordered links, file/completion/review evidence | Query/create/update/finish/close/delete with existing lifecycle and per-task guards |
| `qa.rs` | Job definitions, ordered task/design links, tags, immutable run groups and job evidence | Query and guarded definitions; reserve/append execution evidence without rerunning on replay |
| `memory.rs` | Summary, protocol, retained notes, resolutions and provenance | Read/filter, idempotent append, guarded summary/protocol update; preserve retention rules |
| `rules.rs`, `fixed_hooks.rs` | Optional lifecycle rules and separate fixed-hook overrides | Typed rule CRUD and guarded prompt changes |
| `concurrency.rs` | Resource versions/tombstones and durable operation receipts | Atomic preconditions, touched-resource versions, retry resolution |
| `concurrency.rs` intents | Expiring advisory intent metadata | Publish/list within the actual shared coordination scope; never grant write authority |
| Legacy `coding_guidelines`, `post_task_commands`, `qa_checks`, `task_qa_entries` | Existing project content still read by the dashboard | Preserve and expose through compatibility reads until explicitly migrated |
| `grep.rs`, `mcp/context.rs`, `projection.rs` | Derived search, lifecycle context and generated architecture files | Read projections of the same consistent store; generated files are not canonical storage |

Local app settings own window geometry, known-project registration, rule templates,
architecture-projection preferences, per-computer checkout locations and eventual
connection profiles/secret references. SQLite's current `project_computers` mapping
is preserved during compatibility work; text/server migrations move machine-local
locations out of shared canonical content without losing local registrations.

PNG previews, reproducible design-health scan results, search indexes and other reproducible caches
are disposable. Active processes, connection pools, in-flight jobs and expiring
local advisory intents are runtime state. No cache or machine path belongs in the
Git canonical record set. QA run **evidence**, health waivers and user drafts are not disposable.
Define their sharing/provenance explicitly in task 16. SQL user authorization and
the trust policy for executing shared QA commands are task 20 requirements.

## Shared API and transaction contract

The standalone adashi-storage-api crate owns ProjectStorage (the client API),
StorageBackend (the complete adapter contract), StorageClient (the shared facade)
and ReadSnapshot (a consistent view across all project domains). The application's
storage module reexports these traits. Domain DTOs were extracted and reexported
from their old modules, preserving existing desktop/MCP serialization.

The API declares typed design, mockup, task, QA evidence, memory, rules, fixed
prompt, coordination, legacy-content and health-waiver operations. Shared helpers
implement input preparation, canonical request fingerprints, version checks,
document-token conflicts, final-reference checks and receipt replay. Backend
implementations must run stateful domain validation and these checks inside their
atomic write unit. No driver, connection, SQL row or transport runtime occurs in
the standalone crate's dependency tree.

The complete source contract and operation map are documented in
[the implementation guide](storage-api-migration.md). ProjectStore selects the
configured adapter and delegates to StorageClient. SqliteFactory implements the
full backend and snapshot contracts. Desktop, MCP and the shared QA runner use
that same boundary; production code cannot extract a database connection.

SQL, row hydration, stateful domain validation, seeding and schema migration live
under src-tauri/src/storage/sqlite/. Root domain modules contain shared DTO
reexports and pure helpers. Raw connection helpers outside the backend compile
only in historical tests.

Every mutation follows this contract:

1. Validate operation ID and typed inputs in shared code. Reject duplicate targets,
   invalid identities and invalid/nonpositive update versions.
2. Begin a backend atomic unit of work. Acquire a consistent snapshot.
3. Look up the project-scoped operation receipt **inside** that unit. An identical
   retry returns the original result without executing writes or external effects.
   Reuse with different input returns `storage.operation_reused`.
4. Check all touched-resource expectations and document tokens against that same
   snapshot. Check dependencies/references there as well. Disjoint edits are not
   rejected merely because the project notification cursor changed.
5. Run shared domain validation and persist all changes or none. Increment versions
   only for changed resources; retain deletion versions so stale recreation fails.
6. Record the receipt and bump notification state for a material change in the
   same commit. Receipt-only/no-op operations do not advance the content cursor.
7. Return only after commit; publish refresh signals afterwards. An uncertain
   commit result is resolved through the receipt, not blind mutation replay.

Existing design readTokens remain hashes of canonical editable content and the
existing MCP schema remains intact. Conflicts carry resource identity plus expected
and current versions; design conflicts additionally return the current document
and matching token from one snapshot. Numerical versions cannot prove equality of
independent Git histories; task 16 must add branch-safe content identity without
weakening the public design-token contract. Advisory intents never act as locks.

Request retry results use the existing mutation_operations table inside SQLite.
The text backend keeps them in ignored checkout-local state and never exports
that table to Git. New writes persist a V1 fingerprint and typed result atomically
with project changes. Legacy results remain local; reusing their IDs is rejected
when they cannot prove request equality. A fresh clone has no request history.

A snapshot returns identity, requested domain values/resource versions and its
change cursor from one logical read. Dropping the read/transaction releases its
resources. Dropping an unfinished mutation rolls back. Handles are scoped to one
project; the factory does not use a process-global active-project connection.

`ChangeCursor` is opaque and equality-based at the adapter boundary. SQLite can
encode its existing revision; server SQL can expose a committed sequence; text
storage will use a content identity that detects pull/checkout/revert as well as
local writes. Consumers refresh on inequality and never treat it as write authority.
The existing numeric UI/MCP revision stays in compatibility responses until a
versioned transport change is needed. Refresh must preserve unsaved drafts.

Errors are typed: invalid configuration/version, unavailable backend, not found,
validation, conflict, operation-ID reuse, storage failure, and (when switching is
implemented) stale backend selection. Backend details do not expose credentials.
Protocol adapters map these to existing domain error envelopes where applicable.
Timeout/unavailable/unknown-commit cases must remain distinguishable for SQL.

## Identity and backend switching

The logical project ID is shared; a local registration and a machine ID are not a
user identity. Existing SQLite row IDs and task numbers do not change in task 13.
The adapter must not use display names/numbers as cross-project or cross-clone keys.
Task 16 selects globally collision-resistant resource IDs and a migration mapping
for integer task/QA APIs before text storage can create records in separate clones.
All links must be rewritten through a verified mapping, never guessed by title.

Task 18 introduces backend switching as a migration, not a dropdown that opens an
empty replacement. Capture a consistent source, stage/validate a destination,
verify identities/references/content/history, then activate the descriptor. Keep
the source recoverable. Establish a source-write barrier or detect source changes
before activation; a descriptor file edit alone is not that barrier. Refresh or
reject stale handles so connected desktop/MCP processes cannot keep writing the
old store. Failed/interrupted migration leaves the previous backend usable. Task
22 extends the same service to server SQL. Atomic rename of a descriptor does not
by itself make a multi-file data migration atomic.

## Conformance and delivery gates

| Contract | Task 13 foundation proof | Completion / additional backend proof |
|---|---|---|
| Shared resolution | Legacy/missing descriptor; explicit SQLite; per-project independence; malformed/unknown/unavailable configuration causes no project writes | Native desktop and real MCP read the same settings/backend |
| Consistent snapshot | Rules plus identity, resource versions and cursor read together | All domains and multi-resource dependent reads in task 15 |
| Guarded mutation | Rules batch; stale edit/delete; invalid batch rollback; disjoint updates | Design tokens, references, mockups/tasks/QA/memory in task 15 |
| Idempotency | Original result replay; no cursor/version change on replay; different request rejected | Lost responses and server reconnect in task 23 |
| Notifications | Cursor changes only on committed material mutation | Git checkout/pull/merge and server updates |
| Read stability | Repeated initialized SQLite reads preserve bytes/mtime/revision; app-settings loads preserve bytes | Clean Git clone and all MCP read surfaces |
| Migration | Existing DB uses same path/identity/IDs | Round trips and failure recovery in tasks 18/22 |
| Collaboration | Independent local handles; precise overlapping conflicts | Real Git clones in task 19; authenticated SQL users/project isolation in task 23 |

Reusable conformance cases take a fresh backend fixture factory and use only typed
interfaces. SQLite runs these now; future adapters run the same cases without SQL
test assumptions. Keep byte-level and native-client tests separately as
backend/client integration checks.

## Task 24 interface validation

The standalone crate passes its contract cases without SQLite or Tauri. It covers
all typed read groups, cross-domain snapshots/publication, version and document
conflicts, reference checks, replay and legacy-receipt rejection, rollback/no-op
behavior, advisory intents and independent handle lifetime. Its backend is a
scripted test double with fixture domain responses, not a new production backend.

## Task 14 SQLite implementation and caller migration

The complete SQLite adapter and caller migration are implemented. All production
storage calls from desktop, MCP, lifecycle context, search, projections, mockups,
health and QA use ProjectStorage or ReadSnapshot. Atomic commit owns guards,
domain validation, resource versions, receipts and one content revision per batch.
The raw connection escape hatch is removed.

Schema 14 seeds previously unversioned QA evidence, computer, memory-note and
legacy resources. Initialized, registered opens do not write. Explicit read-only
mode neither migrates nor registers a computer. Initialization enables WAL for
consistent readers during concurrent writes.

Task 15 owns the expanded compatibility suite and native desktop/MCP evidence.
Intentional API changes and operational behavior are recorded in
[the implementation guide](storage-api-migration.md).

## Foundation validation (2026-09-23)

- `cargo test --manifest-path src-tauri/Cargo.toml --lib --offline --quiet`:
  179 passed, five pre-existing manual workspace checks ignored. Includes adapter
  conformance, atomic rollback on a real SQLite failure, independent concurrent
  snapshots/writers, duplicate-operation races, settings read/write concurrency,
  future-schema rejection and desktop/MCP backend-resolution integration cases.
- `cargo build --manifest-path src-tauri/Cargo.toml --bins --offline` builds desktop
  and MCP. The MCP executable also builds with `--no-default-features`.
- `python scripts/check_storage_core.py --binary src-tauri/target/debug/adashi-mcp.exe`:
  two actual MCP processes, 80 concurrent reads, unchanged SQLite/settings bytes
  and timestamps, legacy/explicit SQLite identity preservation, and no fallback
  writes for unavailable or malformed backend selections.
- `python scripts/check_read_only_storage.py --binary src-tauri/target/debug/adashi-mcp.exe`:
  the existing 42-call read-only regression passed through the new factory.

Windows settings writes use same-directory atomic replacement, with bounded
retries for transient sharing errors. Valid settings are normalized only in memory;
invalid settings are left intact and reported. This prevents concurrent readers
from truncating settings or resetting project registration. Native desktop visual
and complete domain parity checks remain the task 15 gate.
