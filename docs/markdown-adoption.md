# Adopt an existing Markdown design

In **Design → Documents → Import document**, choose one UTF-8 `.md` file or enter its
path and click **Preview import**. Review the entire source, proposed title and stable
identity. The default identity is derived from content, never the filename. Existing
identities and identical content are reported; open the existing document instead of
importing it again. Generated Adashi output is rejected as an import source.

Relative Markdown links, reference links, images and unsupported destinations are listed
for review. Assets are not copied or rewritten. Either retain those links deliberately
and repair them in the editor before saving, or cancel and prepare the intended content.
Preview uses the Markdown parser without fetching or executing linked content.

**Review in editor** creates an unsaved draft using the existing Markdown panel. Review
its associations, edit if needed and choose **Save document**. The normal canonical
transaction/readToken rules apply. If the source changed after preview, save is rejected
and the draft remains; preview the source again. Successful import reports the original
source path. Original files are never changed or deleted. Generated destination collisions
are reported separately from a successful canonical save; resolve the output location or
collision and retry generation in Settings. There is no directory scan or file watcher.

Agents that have read an authorized source file use ordinary `upsert_markdown` with its
complete intended body, title, stable identity and associations. Retrieve current operation
help first. Do not include generated notices. Use `list_markdown` and complete retrieval to
check likely duplicates; never replace a document merely because its title resembles a path.

## Convert selected task references

1. Identify one task whose description refers to the adopted source. Read that task and
   the new canonical Markdown document completely; confirm the intended relationship.
2. Keep the task description and existing ordered specification links. Prepare a reviewed
   link list by retaining each current `targetType`/`designExternalId` pair and appending
   `{"targetType":"markdown","designExternalId":"<new stable identity>"}` once.
3. Retrieve `adashi_help` for `adashi_tasks/update`. Submit the task ID, its actual
   `expectedVersion`, a new `operationId`, and the reviewed `designSpecificationLinks`.
   Omit `description` to preserve it verbatim. On a stale version, reread and reconcile;
   never refresh the guard on an old link list. The desktop task picker provides the same
   guarded edit and opens the new reference directly in Documents.
4. For a selected QA job, use its picker or `adashi_qa/update_job` with the job's current
   version and preserved link list. Verify title resolution and direct document navigation.

Deleting or replacing the old file, removing useful explanatory task text, or bulk-converting
tasks is a separate action requiring an explicit user request. Developing this workflow does
not import this repository's docs or rewrite its live task descriptions.
