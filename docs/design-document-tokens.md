# Design document read tokens

The MCP design API uses SHA-256 content hashes to protect existing documents.
Agents no longer send `guard`, `readSet`, `writeSet`, or design resource versions.
Task, QA, rule, memory, and direct desktop command version contracts are unchanged.

## Read, edit, save

Read relevant content with `adashi_design` using `get_by_ids`, `get_scope`, or
`get_bindings`. These responses include `documents` entries:

```json
{
  "documentId": "element:api",
  "document": {
    "externalId": "api",
    "parentExternalId": "system",
    "elementType": "Container",
    "name": "API",
    "description": "Handles requests",
    "technology": "Rust",
    "tags": ""
  },
  "readToken": "sha256:<server-generated hash>"
}
```

`get_documents` accepts these opaque document identities in `ids` and returns
only complete document snapshots and their tokens. Overview, search, and startup
indexes are navigation aids, not editable snapshots. Generated Structurizr views
remain protected and do not get editable document tokens.

Copy each token unchanged. For a batch, pass one pair per existing document the
write will modify or delete. Only actual write targets require tokens; referenced
parents, endpoints, and attachments are validated by Adashi inside the transaction.

```json
{
  "projectName": "Your project",
  "operation": "save",
  "operationId": "rename-api-001",
  "changeIntent": "Clarify the API responsibility",
  "readTokens": [
    { "documentId": "element:api", "readToken": "sha256:<copied from read>" }
  ],
  "changes": [{
    "op": "upsert_element",
    "externalId": "api",
    "parentExternalId": "system",
    "elementType": "Container",
    "name": "Request API",
    "description": "Handles requests",
    "technology": "Rust",
    "tags": ""
  }]
}
```

Creates need no token and succeed only for an absent identity. An existing
identity without a token returns `read_required`, including its current content
and token. `set_element_descriptions` takes `operationId`, `readTokens`, and
`updates` and uses the same checks.

Each C4 element, relationship, Mermaid artifact, binding, and complete mockup is
an independent document. Mockup tokens cover accepted content, drafts, annotations,
and proposals. Canonical hashes exclude resource versions, timestamps, and derived
presentation metadata. Changing another document or the project revision does not
invalidate a token. Agents never compute tokens themselves.

## Out-of-date response

The MCP result has `isError: true`; its text and structured content carry the same
error, complete operation schema, and repair guidance. A single conflict includes:

```json
{
  "code": "out_of_date",
  "stored": false,
  "message": "The design document changed since you read it. Nothing was saved.",
  "documentId": "element:api",
  "currentDocument": { "...": "complete current editable content" },
  "readToken": "sha256:<hash of the returned current document>",
  "request": "Merge your intended changes into each returned currentDocument, then retry with its readToken in readTokens and a new operationId. Do not only replace the token on an old payload."
}
```

Multiple conflicts appear in `conflicts`, one complete document/token pair per
conflicted target; a single conflict uses the top-level fields shown above.
Full document bodies are included once per target. A deleted target has `currentDocument: null` and a matching
absence token; recreating it requires an explicit reconsidered create. Nothing is
partially saved, no resource version or project cursor changes, and the agent is
responsible for merging intended edits into the returned current content.

Cascading deletes protect every document they remove. Newly added dependencies
produce `read_required`; changed dependencies produce `out_of_date`. Review the
returned documents before retrying a destructive operation.

## Transactions and retries

An immediate SQLite transaction covers dependency discovery, hash comparison,
formal syntax/reference validation, persistence, token generation, and the saved
operation receipt. Competing writes cannot pass the same token concurrently.
Multi-document writes are all-or-nothing. Successful writes return `readTokens`
for the changed documents. The existing semantic no-change response also includes
the unchanged tokens without bumping the project cursor.

Reuse `operationId` only for an identical retry. A stored request fingerprint
allows exact replay across MCP restarts and rejects reuse with different arguments.
Use a new operation id for a merged request. Every new request checks tokens again.
Old receipts without a request fingerprint cannot safely prove an identical retry;
the error asks the caller to use a new operation id.

The current write protocol is injected separately from saved custom prompts, so
older `expectedRevision` or guard examples do not describe the active API.
Reconnect MCP clients after installing the rebuilt binary; frozen tool catalogs
must be refreshed. For Aworkit, start a new chat after refreshing MCP discovery.

## Verification

Run `python -B scripts/check_mcp_error_contract.py` against the release binary, or
pass `--binary <path>`. The disposable stdio fixture exercises two independent MCP
processes, first-call creates, exact reads, independent edits, stale conflicts,
merge retries, persisted idempotency, atomic rollback, cascading deletes, deleted
targets, mockup draft conflicts, and detailed parameter errors. `--history <path>`
also replays the 21 recorded failed Aworkit calls into the fixture only.
