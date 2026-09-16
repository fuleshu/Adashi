# Operation help and startup instructions

Adashi exposes eight tools: the existing seven grouped capabilities plus the read-only
`adashi_help`. Capability descriptions remain compact. This change does not redesign the
grouped input schemas into conditional schemas.

Before an unfamiliar call, request the selected operation:

```json
{"tool":"adashi_qa","operation":"create_job"}
```

For a design save, select only the change variants the task needs:

```json
{"tool":"adashi_design","operation":"save","changeTypes":["upsert_uml"]}
```

Omit `operation` to retrieve a compact operation list. For `adashi_grep`, omit it to get
its contract directly. Help requires no project settings or database. It returns the
operation's complete parameter schema, required fields, examples, workflow, and an exact
content fingerprint. Filtered design help retains the requested `DesignChange` variants
and all transitively referenced definitions, and explicitly labels its scope. Invalid
selectors return the allowed choices. Help schemas and error schemas share the typed
operation registry; changing a handler input changes both.

Examples require real project identities/content and retrieved versions or tokens.
Help never executes an example. Agents can reuse a selected contract while it remains
in active context, but must retrieve it again after context loss or server upgrades.
The server never suppresses a help response because another call already requested it.

`agents_template.md` contains shared Adashi workflow: lifecycle hooks, operation help,
targeted design retrieval, document-token writes and merge recovery, memory and search.
Project copies of this template must be updated by their owners; changing the template
does not rewrite unrelated projects' instruction files.

Startup injections contain applicable project-specific rules, custom memory/fixed-hook
instructions, a bounded current summary and a bounded design index. Known old built-in
manuals migrate to empty defaults with resource-version changes. Matching is exact
apart from line endings and known tool-name rewrites; custom additions are preserved.
The obsolete full authoring default containing `expectedRevision` is explicitly covered.
Generic hash-write guidance is no longer injected separately. Empty custom instruction
fields are supported to disable those optional sections.

Validation: Rust contract/migration tests plus `scripts/check_mcp_error_contract.py`.
The stdio fixture requests help, then makes its first UML/mockup save and QA create/run
without using failed writes to discover parameters. This verifies executable examples
and contracts; it does not claim that every unfamiliar language model will always choose
the right operation. Rebuild/reconnect the MCP server and refresh client tool metadata
to activate the new help tool; already running old server processes keep their old code.
