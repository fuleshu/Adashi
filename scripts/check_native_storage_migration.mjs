/** Additional real WebView2 conversion scenarios, used by check_native_storage.mjs.
 * Exercises the settings buttons, a connected MCP process and a dirty native editor.
 */
import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import { randomUUID } from "node:crypto";

export async function verifyMigration({ page, call, request, getTask, updateTask, taskIds, until, passed, project, fixture, evidence }) {
  const markdownBody = "# Migration design\r\n\r\nGrüße 日本語 📝\r\n```rust\r\nlet preserved = true;\r\n```\r\n";
  await call("adashi_design", { operation: "save", changeIntent: "Verify native Markdown migration", operationId: randomUUID(), readTokens: [], changes: [{ op: "upsert_markdown", externalId: "migration-prose", title: "Migration prose", body: markdownBody, designLinks: [] }] });
  const markdownBefore = await call("adashi_design", { operation: "get_documents", ids: ["markdown:migration-prose"] });
  const selection = async () => JSON.parse(await fs.readFile(path.join(project, ".adashi/storage.json"), "utf8"));
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  const panel = page.getByRole("region", { name: "Project storage", exact: true });
  await panel.getByText("SQLite database", { exact: true }).first().waitFor();
  await panel.getByRole("combobox").selectOption("text");
  await panel.getByRole("button", { name: "Review conversion", exact: true }).click();
  await panel.getByRole("heading", { name: "SQLite database → Git text files" }).waitFor();
  await page.screenshot({ path: path.join(fixture, "native-migration-preview.png"), fullPage: true });
  const before = await getTask(1);
  await panel.getByRole("button", { name: "Convert and switch", exact: true }).click();
  await panel.getByText("Converted and switched to Git text files.", { exact: true }).waitFor();
  assert.equal((await selection()).backend.kind, "text");
  assert.equal((await getTask(1)).title, before.title);
  assert.deepEqual(await call("adashi_design", { operation: "get_documents", ids: ["markdown:migration-prose"] }), markdownBefore);
  const stale = await request("tools/call", { name: "adashi_tasks", arguments: { projectName: "fixture", operation: "update", taskId: before.id, expectedVersion: before.version, operationId: randomUUID(), title: "Must not overwrite after migration" } });
  assert.ok(stale.error || stale.result.isError, "pre-migration MCP guard rejected");
  const created = await call("adashi_tasks", { operation: "create", operationId: randomUUID(), title: "Created by MCP in text storage" });
  await page.getByRole("button", { name: "Tasks", exact: true }).click();
  await page.getByRole("button", { name: new RegExp(`Task #${created.task.number} Created by MCP in text storage`) }).waitFor();
  passed("settings converts SQLite to text and connected MCP reads and writes the selected backend");

  // Exercise the reverse settings action, including populated-destination consent.
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await panel.getByRole("combobox").selectOption("sqlite");
  await panel.getByRole("button", { name: "Review conversion", exact: true }).click();
  const replace = panel.getByRole("checkbox", { name: "Preserve the existing destination in a backup and replace it" });
  await replace.waitFor();
  assert.ok(await panel.getByRole("button", { name: "Convert and switch", exact: true }).isDisabled());
  await replace.check();
  await panel.getByRole("button", { name: "Convert and switch", exact: true }).click();
  await panel.getByText("Converted and switched to SQLite database.", { exact: true }).waitFor();
  assert.equal((await selection()).backend.kind, "sqlite");
  assert.deepEqual(await call("adashi_design", { operation: "get_documents", ids: ["markdown:migration-prose"] }), markdownBefore);
  assert.equal((await call("adashi_tasks", { operation: "get", taskId: created.task.id })).task.title, created.task.title);
  await updateTask(2, "MCP update after reverse conversion");
  assert.ok(await panel.evaluate(element => element.scrollWidth <= element.clientWidth), "backup path must wrap inside the settings panel");
  await page.screenshot({ path: path.join(fixture, "native-migration-complete.png"), fullPage: true });
  passed("settings converts text back to SQLite only after explicit destination backup choice");

  // A separate command can switch storage while a native editor remains dirty.
  await page.getByRole("button", { name: "Tasks", exact: true }).click();
  await page.getByRole("button", { name: new RegExp(`Task #${(await getTask(1)).number} `) }).click();
  const title = page.locator(".task-editor-form label").filter({ has: page.locator("span", { hasText: /^Title$/ }) }).locator("input");
  await title.fill("Draft retained across backend conversion");
  const report = await page.evaluate(async () => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    const plan = await invoke("preview_storage_migration", { projectId: "fixture", target: "text" });
    return invoke("migrate_project_storage", { projectId: "fixture", target: "text", planToken: plan.planToken, archiveDestination: true });
  });
  await until(async () => {
    const status = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke("get_project_storage", { projectId: "fixture" }));
    return status.backend === "text";
  }, "shared selection changed to text");
  assert.equal(await title.inputValue(), "Draft retained across backend conversion");
  await title.press("Tab");
  await page.getByRole("button", { name: "Close error dialog" }).waitFor();
  assert.notEqual((await getTask(1)).title, "Draft retained across backend conversion");
  await page.getByRole("button", { name: "Close error dialog" }).click();
  assert.equal(await title.inputValue(), "Draft retained across backend conversion");
  await page.getByRole("button", { name: "Use current value", exact: true }).click();
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await panel.locator("strong").filter({ hasText: /^Git text files$/ }).waitFor();
  assert.equal((await call("adashi_tasks", { projectName: "independent", operation: "list" })).tasks[0].title, "Other project remains SQLite");
  assert.ok((await fs.stat(path.join(fixture, "independent/.adashi/adashi.sqlite3"))).size > 0);
  await assert.rejects(fs.readFile(path.join(fixture, "independent/.adashi/storage.json")), { code: "ENOENT" });
  assert.ok((await fs.stat(path.join(report.backupFolder, "source/.adashi/adashi.sqlite3"))).size > 0);
  evidence.migration = { finalSelection: await selection(), report, secondProject: "sqlite", preservedTaskId: created.task.id };
  passed("backend switch preserves dirty native drafts, rejects their old guards and leaves other projects unchanged");
}
