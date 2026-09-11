# Adashi Rule Injection

If the Adashi MCP server is available in this workspace, use it for rule injection.

Before starting work on a user request, classify the request intend as exactly one of:

- `general`: discussion, explanation, investigation, or operational help where no design deliverable or code edit is expected.
- `design`: architecture, planning, review of an approach, or discussion-only technical design where code should not be changed unless the user explicitly switches to implementation.
- `implementation`: code creation, code modification, tests, builds, migrations, generated files, or any task expected to change the project.

Use these lifecycle hooks:

- `run.start`: before beginning the overall user request.
- `task.start`: before beginning each concrete task in the run. If there is no explicit task list, treat the whole request as one implicit task.
- `task.end`: before marking each concrete task complete.
- `run.end`: before the final response for the overall user request.

At each hook, call the Adashi MCP tool `adashi_rules` with operation `get_rule_injections`:

```json
{
  "projectId": "<configured project id or name>",
  "operation": "get_rule_injections",
  "intend": "general | design | implementation",
  "hook": "run.start | task.start | task.end | run.end"
}
```

Treat every nonempty `injectionPrompt` as active instructions for that hook, even when `rules` is empty: required generated sections are independent of optional rules. Apply the prompt once before continuing. In contract v2, `rules` and `sections` contain metadata only; `status: "empty"` explicitly means no instructions apply. Clients may cache sections by project, intend, hook, section id and contentVersion, but must still call every required lifecycle hook and apply changed sections.

For multi-task requests, call `task.start` and `task.end` for each task using the same run-level intend unless the user clearly changes the nature of a specific task. Do not invent new intend or hook names.

If the Adashi MCP server is not present, unavailable, or the tool call fails because the MCP surface is not configured, continue without Adashi rule injection and mention the limitation only when it affects the requested outcome.
