# Aworkit Adashi write failures, 2026-09-16

The latest Aworkit chat was the SplatMCP milestone run, chat id
`chat.23796a15a4174a9dd2bb23a4729044e6e0b7b15a` (8,114 events).
Evidence was read without modifying Aworkit's database at
`%APPDATA%\com.aworkit.desktop\runtime\history\aworkit.sqlite3`.

There were 21 failed Adashi mutation calls and 20 successful mutation calls.
All 12 design saves and one description update failed. Tasks had nine successful
creates, one successful update and eight successful finishes; six task calls failed.
Both QA jobs were created successfully. Both attempts to **run** QA failed before
execution. Live API inspection confirmed two stored jobs, no runs, revision 26.

## Exact failed calls

Numbers refer to `semantic_events.sequence` for the `tool.requested` event.

| Requests | Actual rejection |
| --- | --- |
| 1738, 1758 | Element changes omit the required `op` discriminator. |
| 1782, 1820, 1866, 1925, 2002, 2041, 2223 | Design save omits `guard`. |
| 2080 | Guard has only `operationId`; `writeSet` omits `design.element:probe-container`. |
| 2356 | Guard uses a project revision and invented resource `design:splatmcp-1789033109991`; expected 15, actual resource version 0. |
| 2448 | Description update omits `guard`. |
| 2599 | Write target is correct, but `readSet` omits parent `design.element:1`. |
| 1926 | Task create omits `operationId`. |
| 2162, 2222, 2249, 2297 | Task updates omit `expectedVersion`. |
| 3194 | Aworkit rejects `completedAt: null` locally. `completedAt` is also an unsupported Adashi input. Removing it allowed the finish to succeed. |
| 8082 | `run_jobs` omits required `query`; `limit` is not a run selection parameter. |
| 8105 | `run_jobs` supplies a query but omits `operationId`. |

The missing guard/version/query/operationId and read/write-set failures were also
reproduced through the connected Adashi API. They are deterministic rejections,
not evidence of a dropped connection or an uncertain database write. The original
design payload also used `Database` as an element type; after fixing its guard it
must use a supported C4 type (for example Container with a Database tag).

## Adashi fixes

- Known-tool errors now pass through one `ServerHandler::call_tool` boundary and
  return a normal JSON-RPC result containing `isError: true`, rather than a JSON-RPC
  error that Aworkit misclassifies. This covers parameter decoding, missing fields,
  domain errors, version conflicts, and design saves with `ok: false`.
- The text and `structuredContent` contain the same detailed repair report: actual
  reason, original structured evidence, all required and missing parameters,
  fields not used by the selected operation, indexed design-change decoding errors,
  and the complete operation-specific schema (including nested types and enums).
- Write examples cover design saves/description updates, task create/update/finish,
  and QA creation/execution. They are explicitly illustrative, not replacement actions.
- The subsequent approved hash contract replaces the initial `requiredGuard`
  repair helper. Design reads now return complete documents and opaque `readToken`
  values. Writes pass `operationId` and `readTokens` for existing targets; creates
  need no token. Adashi derives reference/dependency checks internally. A stale write
  saves nothing and returns `out_of_date`, full current documents, fresh tokens, and
  explicit merge instructions. See [the document token contract](design-document-tokens.md).
- Tool descriptions now list the actual required fields instead of claiming a flat
  schema contains operation-specific requirements. Design examples and lifecycle
  guidance now explain read tokens and supersede old guard/project-revision examples.

Explicit design reads add `documents`; design write receipts add `readTokens`.
Idempotent retries check a saved request fingerprint, resource concurrency checks
remain internal, and design transactions remain atomic. No production task/QA data
was added or repaired. The approved hash contract was persisted in Adashi's formal
design separately from the disposable tests.

## Aworkit fix still needed

1. `crates/aworkit-capability-host/src/mcp/transport/peer.rs`, in
   `handle.await_response()` error mapping (around line 1120), maps every
   `ServiceError`, including a received JSON-RPC error response, to `request_failed`
   with dispatch `Started` and text claiming no terminal response was received.
   Distinguish a received server error from a transport failure. Keep the server's
   error code, message and structured data after existing secret redaction.
2. `crates/aworkit-capability-host/src/mcp/session.rs`, error settlement around
   lines 796-823, converts a started call's error to `MissingOrConflicting`, sets
   `result: None`, and drops the server's reason. Preserve a terminal server-error
   result. Do not infer safe replay for arbitrary server failures; only a genuine
   transport loss or missing terminal evidence should be described as uncertain.
3. `desktop/src-tauri/src/runtime/tool_loop.rs`, `mcp_outcome_error` around line
   4170, can then present the retained reason to the model. Currently it has only
   the disposition and emits "the call outcome is uncertain and will not be replayed".
4. `validate_mcp_arguments` in the same file (around line 3854) rejects all nested
   JSON null values. The recorded Adashi schema explicitly permits null for many
   optional fields. Validate against each tool's schema instead of banning null
   globally. The particular `completedAt` field is invalid even without this ban.

The recorded model input (event 198) contains Adashi's full nested input schema,
including `DesignChange.oneOf`, `MutationGuard`, and `ResourceExpectation`.
There is no evidence in that event that Aworkit stripped those schema definitions.
The required operation-level fields and guard semantics were insufficiently
documented by Adashi; the error suppression prevented the agent from recovering.

Suggested Aworkit regression: an MCP fixture returns a JSON-RPC `-32602` with a
specific missing-field message and structured details. The model must receive
those details and no automatic retry may occur. Separately test a disconnected
server, a normal `isError: true` tool result, and nullable schema properties.

## Reproduction and activation

Run `python scripts/check_mcp_error_contract.py` against the rebuilt release MCP.
The optional `--history` argument accepts the Aworkit SQLite path and replays the
21 failed calls into a disposable repository-local project. It never replays
successful historical writes or runs against the real SplatMCP project.

The fixture verifies unchanged revision after all historical failures, matching
text/structured errors, successful design/task/QA writes, document tokens,
merge retries, two competing MCP processes, cascading deletes, mockup draft conflicts,
idempotency, and atomic rollback. Detailed local
results are written to `target/mcp-error-contract.json`.

The rebuilt binary is `src-tauri/target/release/adashi-mcp.exe`. The connected
clients currently execute `C:\Program Files\Adashi\adashi-mcp.exe`; that installed
binary has not been replaced. Install the updated binary and reconnect MCP clients
to activate the fix. Aworkit freezes tool definitions per chat, so use a new chat
after refreshing its MCP discovery to get the updated descriptions.
