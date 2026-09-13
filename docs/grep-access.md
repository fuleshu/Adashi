# Grep-shaped project access

Status: **implemented.** `src-tauri/src/grep.rs` holds the search, the tolerant pattern parser and the budgeted renderer; `src-tauri/src/mcp.rs` exposes it as the `adashi_grep` tool.

## Problem

Agents do not address Adashi the way they address a codebase. They grep and read files habitually; they do not fish for opaque ids through narrow tool calls. Every id-shaped retrieval costs a decision — decide you need it, know the tool, know the id — and each of those is a place to drop out.

Two changes reduce that cost: address the project the way a person does, and search it the way an agent already searches code.

## The content/tooling distinction

Adashi stores two different kinds of thing, and only one of them is *project content* worth grepping.

| Kind | Examples | Searchable |
| --- | --- | --- |
| **Project content** | C4 elements and relationships, UML artifacts, UI mockups, bindings, tasks, memory summary and notes | **yes** |
| **Project tooling** | QA jobs and run evidence, lifecycle rules, fixed-hook prompts | **no** |

Tooling defines *how the project is operated and verified*; it is not a description of the project. Searching it would surface Adashi's own machinery in answers about the codebase. QA run output is also the one field that can detonate a context window. Both are excluded.

## The tool: `adashi_grep`

One cross-domain tool. Not per-domain operations: grep's habit is "search everything", and making the agent pick a domain reintroduces exactly the decision cost this is meant to remove.

### Parameters

| Field | Meaning |
| --- | --- |
| `pattern` | What to look for. Deliberately tolerant — see below. |
| `in` | Optional scope: `all` (default), `design`, `tasks`, `memory`. |
| `file` | Optional: restrict design hits to elements bound to this file or symbol. |
| `type` | Optional: C4 element type, for example `Container`. |
| `state` | Optional: task state — `todo`, `active`, `finished`, `closed`. |
| `limit` | Optional: maximum matches returned. Bounded. |

`file` is the bridge to the codebase: given the file an agent is already editing, it answers "which design elements own this?" using the existing bindings, without requiring the agent to know any id.

### Input tolerance: tolerant, never clever

Adashi's stated principle is that the MCP exposes facts and validation, not intelligent task inference. Semantic guessing here would contradict that and, worse, make results irreproducible — an agent cannot learn a tool whose interpretation shifts between calls.

So the tool is **tolerant in shape, deterministic in meaning**:

- Accepts a bare string, whitespace-separated terms, quoted phrases, or `key:value` clauses.
- **Case-insensitive always.** This differs from grep deliberately: it removes a decision the agent would otherwise have to make and get wrong.
- Whitespace-separated terms are **AND** — every term must appear. This narrows rather than broadens, which is what keeps results from becoming noise.
- A quoted phrase matches as an exact substring.
- An **unknown `key:value` clause becomes a literal term**, never an error.
- Anything that is not recognisable as a filter is searched as text.
- The only failure is having nothing to search.

### Empty pattern

Grep with no pattern matches everything. The useful analogue is not a dump and not an error: **an empty pattern returns the top-layer overview with counts**, which is what an agent with no idea yet actually needs.

## Output shape

Make it look like grep output. The habit is the feature:

```
412 matches — design(318), tasks(82), memory(12)

design:5: Container "Adashi MCP Server" — Passive stdio MCP server that exposes…
design:6: Container "Project Data Store" — Project-local SQLite database under…
task:4: finished — Implement v1 QA system from formal design
memory:note-7: …
```

- The header states the true total and the per-domain split, so the agent sees the shape before the detail and can narrow deliberately.
- One match per line, whitespace-collapsed, so the output stays grep-shaped rather than becoming nested JSON.
- **The locator prefix is a drillable address**: `design:5` → the design `get_scope` operation; `task:4` → the tasks `get` operation. The loop is then grep → drill, exactly like grep → read file.

## Bounds: no spam, no broken truncation

Grep shows the **matched line**, not the first eighty characters of the file. Truncating a field from its head can hide the very text that matched, returning a result the agent cannot act on. Therefore:

- **Window the text around the match**, with a little context on each side, and never cut mid-word at the tail.
- **Budget the whole response** and report `showing N of M — narrow the query` when it is exceeded.
- **Collapse to one line per match**; newlines inside stored text become spaces.
- **Deduplicate**: the same sentence is reachable through a description, a binding and a projection. Return it once, at its best locator.
- **Never return a whole artifact.** Source-class fields — Structurizr DSL, Mermaid source, mockup SVG — may be *matched* but only ever returned as a short window. Artifact metadata (type, label, title, screen, state) is returned normally.
- **Deterministic ordering**: domain order, then field weight (name and title before description and body before bindings), then id. No ranked relevance, because ranking that cannot be explained cannot be learned.

## Companion change: `projectName`

The tool surface should address the project the way a person does. `resolve_project_from_settings` already accepts an id *or* a name, so this is a naming change, not a behavioural one.

- The wire field becomes **`projectName`**; the schema advertises only that.
- **`projectId` is removed outright — no deprecation window and no alias.** Even a temporary alias leaves the agent reasoning about which one to use, which is the opposite of the goal. Every caller moves in the same change: this repository's `agents.md` and `agents_template.md`, the QA scripts, and the MCP contract documents. Projects outside this repository own their own instruction files and are updated by their owner.
- **Duplicate project names are forbidden.** Name comparison folds case, so `adashi` and `Adashi` collide; the name is still displayed exactly as written. Uniqueness is enforced when a project is added. No settings migration is needed, because no configured project currently shares a name.
- Resolution by name must **fail loudly on ambiguity** rather than silently choosing one.

## Companion change: memory note addressing

A locator is only useful if it can be opened. `memory:note-7` had no address: the memory `get` operation filters by query, runId and taskId, but never by note id. It gains a **`noteId` filter**, which keeps the locator honest and closes the same gap for any agent already holding a note id from an earlier result.

## Non-goals

- Deliberately **not** a replacement for the in-tree projection. Grep lowers the cost of pulling; the projection is what makes the agent aware there is something worth pulling. They are complements, and the intended loop is projection names the responsibility → grep the concept → drill in.
- Not an LLM-in-the-loop interpreter. Deterministic substring matching only.
- No tokenised full-text search. Substring semantics match what an agent expects from grep; stemming and tokenisation would break that expectation.
- No searching of QA or rules.

## Resolved: scope of the `file` filter

The `file` filter applies to **both** design bindings and task file lists (`createdFiles` / `changedFiles`). It surfaces the component that owns the file *and* the tasks that touched it; answering only half would send the agent looking elsewhere for the rest.

It is a **scope, not a query**. `file:<path>` narrows a search the pattern still has to justify, so it must be paired with at least one term: `concurrency file:src-tauri/src/concurrency.rs`. Used alone it has nothing to search for, and the empty-pattern rule takes over, returning the overview.

## Searchable fields

| Domain | Fields |
| --- | --- |
| design | element `external_id`, `name`, `description`, `technology`, `tags`; relationship `description`, `technology`, `tags`; diagram/mockup titles and metadata; binding `target`. Artifact *source* fields (Structurizr DSL, Mermaid source, SVG) are matched but never returned whole. |
| tasks | `title`, `description`, `completion_memo`, `created_files`, `changed_files` |
| memory | the shared summary and note bodies. The memory protocol rule is a rule — tooling — and is excluded. |

Locators map onto the existing retrieval surface, which is what makes them drillable: `design:<external_id>` → the design `get_scope` operation, `task:<id>` → the tasks `get` operation, `memory:<note_id>` → the memory `get` operation with the new `noteId` filter.

## Implementation notes

Where the implementation had to be more specific than the shape above:

- **Clause values are validated, unknown keys are not.** `in`, `file`, `type`, `state` and `limit` are the filters; a value that is not valid for one of them (an unknown scope, an unknown task state, a non-numeric or out-of-range limit) fails explicitly, because those are closed vocabularies and a typo there is a mistake, not a search term. Any other `key:value` token is text. The task states are the task lifecycle's own, taken from one list rather than spelled out again here, so a search and a task listing cannot disagree about what a state is called; an unknown state is rejected with the accepted values named.
- **Closed tasks are out of a search by default**, matching a task listing, because accepted history is not current work. `state:closed` searches it deliberately.
- **A quote only opens a phrase at a token boundary.** Inside a word it is part of the word, so `it's` stays searchable text. An unclosed quote is left in the token rather than swallowing the rest of the pattern.
- **The empty-pattern overview is bounded to the top layer**: the Software System, the Containers, their responsibility lines, a fixed first slice of the boundaries, and per-domain counts. It is not a dump of the model.
- **The `file` filter narrows; the pattern still has to match the artifact's own text.** A binding's target is searchable text, so grepping a file path surfaces the component that owns it and the tasks whose file lists contain it. Filtering alone never invents a match, and because the tool requires at least one term, a `file:` clause on its own returns the overview rather than everything bound to that path. Answering "what is involved with this file?" from a path alone would need a different result shape — identities rather than matched text — so it is deliberately not a query of its own.
- **Budget accounting reserves the trailing omission notice before writing any line**, so the rendered output cannot exceed the budget by the width of its own accounting.
- **A binding is a property of the element, not a separate artifact**, so `design:<external_id>` is one locator whether the match came from the element's own fields or from a file or symbol bound to it.

