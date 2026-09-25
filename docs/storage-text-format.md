# Git text storage, version 2

Task 16 specification, implemented by task 17. Desktop and MCP use the same `ProjectStorage` facade.
The portable descriptor is `.adashi/storage.json` with `schemaVersion: 1` and
`backend: { "kind": "text" }`. No Git command is run by the adapter.

## Canonical representation

Use UTF-8 JSON, without a BOM, two-space indentation, LF and one final newline.
Object keys are sorted lexically. Arrays retain explicit domain order. Compare
serialized bytes before publishing; never rewrite unchanged files. JSON was
selected for strict types and the existing Rust/TypeScript tooling. YAML's implicit
types and a single project-wide JSON document add avoidable merge problems.

`.adashi/text/format.json` declares format version 2 and relational schema 15.
Schema 14 remains readable without file changes. A successful new write upgrades
the format marker to 15; unrelated records retain their bytes. Newer unsupported
schemas fail explicitly. Markdown records are forbidden under a schema-14 marker.
The `markdown_design_documents` and `markdown_design_links` collections retain
complete multiline bodies, stable external identities and ordered typed links.
Their SQL foreign keys use the same UUID record-reference encoding as other
domains. Content tokens are derived from canonical fields, never stored as a
second copy that could cause spurious Git conflicts. Generated `.md` files are
not part of this record vocabulary and are never imported during reads.
Record envelopes remain version 1. Version 2 removes application request history
from project files; request retry results belong to ignored local runtime state.
`.adashi/text/records/<collection>/<identity>.json` stores one record per immutable
identity. Collections follow the established project schema; this is a versioned
record vocabulary, not permission to execute arbitrary SQL from project files.
An envelope contains `schemaVersion`, `identity`, `collection`, `deleted`, and
`data`. Unknown collections, fields, envelope versions and duplicate identities
are errors. Only an explicit format migration can introduce another vocabulary.

The implementation may materialize these records into an **in-memory** SQLite
projection to reuse existing queries, constraints and domain validation. That
projection is disposable, rebuilt from canonical records, and never authoritative.
This avoids independently reimplementing task, design, memory and QA semantics.
The cost is O(project size) validation/materialization; this format targets local
project metadata, not large media or an unbounded event archive.

All schema-14 authoritative collections are retained: project/workspace identity;
C4 elements, relationships, bindings and Mermaid artifacts; tasks and ordered
links; QA definitions, links, tags, reservations and retained evidence; rules and
fixed prompts; memory summary/protocol, notes and resolutions; mockup accepted
content, drafts, annotations, operations and proposals; health waivers; legacy
guidelines, commands and task QA evidence; and resource deletion/version provenance.
A schema inventory test must reject unclassified tables. `mutation_operations`
is used by the internal SQL projection and legacy import only; it is never exported.

Generated Structurizr workspace text/JSON and its diagram are reconstructed from
the formal model. Project revision, autoincrement sequences, schema migration
timestamps, previews and design-health scan results are derived. Computer checkout
mappings, request retry results, advisory intents, locks, journals and migration state live under ignored
`.adashi/local/`; local registrations and credentials remain in app settings.
Migration retains computer mappings locally for conversion back to SQLite.
An absolute QA working directory is user-authored executable configuration: retain
it with an explicit portability warning in migration results, never silently alter it.

## Identity and compatibility

Record identity is a lowercase 128-bit UUID. Existing SQLite records receive a
deterministic UUIDv8 from SHA-256 of the project identity, collection and primary key;
new records receive UUIDv4 identities. Integer API IDs are stable compatibility
aliases, not the record's identity or its task/QA display number. Preserve every
existing alias during migration. Every numeric collection retains SQL's original
AUTOINCREMENT behavior, including deleted IDs and IDs consumed by upserts. Its
next allocation follows the highest live, deleted or consumed ID, with no random
gap. High-water marks without live rows are retained as minimal tombstone records,
using the existing format; there is no tracked sequence file. All IDs are positive
JavaScript-safe integers (at most 2^53-1). UUID identities and foreign-key references remain unchanged by an
explicitly authorized numeric correction, which must also update task resource
keys and textual dependencies. Corrections are never automatic on read.
Different records claiming an alias are a merge error, never an automatic reassignment.

Canonical foreign-key references contain the target record identity, e.g.
`"task_id": { "ref": "8a79832e-c811-4dba-bcbd-a0aa4c638535" }`.
The loader resolves these to integer aliases only in its disposable projection.
Named design identities remain their existing user-visible strings; polymorphic
references also retain and validate the target kind.

Task and QA numbers are separate human labels. Both use the project's
current maximum plus one, matching pre-adapter SQLite. Independent clones can
allocate the same next numeric ID or number in any collection;
a merged collision is rejected with the affected records and requires explicit
reconciliation. Clients use API IDs for addressing. IDs do not replace chronological order:
QA history and retention use timestamps with a stable identity tie-breaker.

Multiline string values use `{ "lines": ["first line\n", "second line"] }`,
including terminators in each chunk. This preserves trailing newlines, CRLF and
empty strings exactly while allowing line-based Git merges of Mermaid/DSL/SVG,
Markdown and QA output. It is distinct from a reference object. Other strings,
numbers and null remain JSON scalars. Domain lists use explicit `sort_order` or
`sequence` fields; directory order is never meaningful. Task/QA link positions must
be unique within their owner and collection; gaps are allowed, but two entries at
the same position after a merge are rejected. Mockup annotations retain their
existing `(sort_order, id)` tie-break semantics. Removing a record writes
a tombstone retaining identity and primary-key aliases, without its old body.
Live references to tombstones fail validation. Completed QA evidence is immutable
through ordinary editing operations; retention writes tombstones.

## Guards and external changes

Compute a SHA-256 digest over sorted canonical paths and bytes. Recompute it for
each operation; timestamps, cached directory listings and numeric project revisions
are insufficient. A different digest invalidates cached snapshots and yields a
shared change notification, including Git checkout, pull, merge and branch changes.
The desktop preserves dirty drafts and their original preconditions when refreshing.

Per-resource preconditions include the resource's authoritative aggregate and
ordered children, not unrelated resources. Derive integer compatibility versions
from content fingerprints; they are opaque, non-monotonic JavaScript-safe tokens.
Detect ambiguous token collisions instead of treating them as equality. Existing
full hashes are remembered in an ignored checkout-local ledger, so returning to
an earlier branch also checks for an ambiguous truncated token. Existing
full SHA-256 design document tokens remain unchanged. Stored numeric counters are
provenance, not evidence that two branches contain equal content.

Request retry results remain in `.adashi/local/state.json`. They are checked before
guards and preserve the original fingerprint/result across local process restarts.
An identical request returns its previous result; reusing its ID with different
content fails. Desktop and MCP share this protection in the same checkout.
Fresh clones start with no request history, and retries must return to their
original checkout. Git shares project state, not application request delivery.
Reads, fresh writes with no persistent SQL effect and repeated requests leave
tracked bytes and mtimes unchanged. A fresh SQL upsert can consume an ID even when
its content is unchanged; preserving pre-adapter allocation requires persisting
that sequence advance as a tombstone. An identical request retry does not consume
another ID. Local results are journaled with the project changes but are never
exported as project records, portable barriers or deletion tombstones.
Clones coordinate only through committed files, not through intents from another
computer. QA execution ownership is checkout-local; copied running reservations
must not authorize a second process to execute an already claimed command.

## Consistency and recovery

All cooperating desktop/MCP readers, writers and migration operations acquire the
same checkout-local OS file lock. The OS releases it when a process exits. A read
loads a full immutable snapshot while locked, then releases the lock; a later
writer cannot change that snapshot. Ordinary reads do not alter canonical bytes.
External Git/editor processes do not honor this lock: compare complete inventories
before/after reads and before publication. Reject a changing inventory and ask for
a retry after the external operation finishes. Users must not run Git mutations
concurrently with Adashi writes; portable filesystems cannot make a directory-wide
transaction atomic to an uncooperative Git process.

A writer validates the complete staged model before publication. Under the lock,
write and sync an ignored transaction journal containing exact before/after bytes,
then durably mark it prepared. Publish only changed files via same-directory atomic
replacement, then retire the journal. No cooperating reader observes a subset:
it first completes recovery under the lock. Recovery rolls a prepared transaction
forward only when each file matches its before or after image. An unexpected user
edit stops recovery with the affected path and preserves the journal and files;
never overwrite that edit. Failure before preparation changes no canonical files.
An ambiguous failure after preparation reports `storage.commit_uncertain`; retry
or receipt retrieval first recovers and then resolves the outcome. Atomic rename
and file sync are required; directory durability is platform dependent and must be
documented/tested rather than overstated as a power-loss guarantee.

## Validation and merge examples

Parse the closed schema, reject duplicate JSON keys and unresolved conflict-marker
lines, and check filename/identity/collection agreement before importing. Reject
duplicate aliases, duplicate natural keys, invalid scalar types, unresolved/deleted
references, invalid ordering, C4 cycles/hierarchy, invalid Mermaid, unsafe SVG,
mockup draft/proposal inconsistencies, invalid task/QA states and unsupported schema
versions. Invalid projects remain inspectable as files but dependent writes fail
with actionable paths. No automatic merge resolution, commit, checkout or push.

* Two clones add different tasks: UUID identities and references remain distinct,
  but sequential IDs/numbers can collide. Validation blocks the merge until an
  explicit correction of numeric aliases and task resource keys; no task is lost.
* Clone A edits task X; clone B edits task Y: only their record and guard provenance
  files change. Both survive a normal merge.
* Both edit X: Git may report a conflict; if separate fields merge cleanly, the
  merged content gets a new fingerprint and both pre-merge drafts become stale.
* A deletes design element E while B adds a task link to E: Git can merge the files,
  but semantic validation rejects the live reference to E's tombstone.
* Branches independently advance the same stored counter to 4 with different
  bodies: fingerprints differ, so a precondition from one cannot overwrite the other.
* A journal is interrupted after its first replacement: the next reader completes
  the prepared transaction before exposing data; an unrelated user edit blocks
  recovery without losing that edit.

## Migration boundary (task 18)

Version-1 projects remain readable without rewriting files. Their next successful
content write upgrades `format.json` to version 2 and removes the old
`records/mutation_operations/*.json` files, preserving retry results locally.
This one-time format upgrade may appear in Git even when that first request makes
no domain change. Subsequent no-op requests change no tracked files. Commit the
format change and history-file deletions together. Read-only opens never upgrade.
Staged conversions also produce version 2. Older binaries reject version 2 rather
than reintroducing shared request history. The local journal supports interrupted
cleanup and refuses to delete a file whose bytes were changed externally.

Export a consistent source snapshot under the shared writer barrier. Stage an
unselected destination, preserve all authoritative records/aliases/provenance and
locally retained computer mappings, import it, and compare canonical domain counts
and content. Reject unknown records. A populated destination requires an explicit
choice to preserve its complete before-image in the backup before replacement;
never overwrite it silently. Recheck source content and descriptor generation before activation. Keep a
recoverable source copy. Atomically switch the descriptor only after verification.
Every open handle checks its descriptor generation before writing; a switch makes
old handles unusable and forces both clients to reopen. Conversion failure leaves
the source selected. Interrupted activation is resolved from a local migration
journal; it never guesses from whichever destination files happen to exist.

Track `storage.json` and `text/**`; ignore `local/**`, retained SQLite files and
sidecars. Joining a clone validates existing text data without seeding or rewriting
it, and supplies that computer's checkout path through its local registration.

The implemented workflow and recovery procedure are in
[Switching project storage](storage-migration.md).
Everyday commit/pull/merge and conflict resolution are documented in
[Collaborating through Git text storage](storage-git-collaboration.md).
