/** Real Git pull/branch/conflict recovery while native Adashi and two MCPs stay open.
 * First run check_git_collaboration.py; pass its evidence directory as argv[2].
 * All Git operations and desktop settings are confined to that generated fixture.
 */
import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import fs from "node:fs/promises";
import { createRequire } from "node:module";
import path from "node:path";
import { canonical, client, git, startDesktop, until, assertError } from "./git_collaboration/native_helpers.mjs";
import { checkTaskNumbers } from "./git_collaboration/native_numbers.mjs";

const { chromium } = createRequire(import.meta.url)(process.env.ADASHI_PLAYWRIGHT_PACKAGE || "playwright");
const fixture = JSON.parse(await fs.readFile(path.join(process.argv[2], "native-fixture.json"), "utf8"));
const allowed = path.resolve("target/git-collaboration") + path.sep;
assert.ok(path.resolve(fixture.root).startsWith(allowed));
for (const field of ["repoA", "repoB", "settingsA", "settingsB"]) assert.ok(path.resolve(fixture[field]).startsWith(path.resolve(fixture.root) + path.sep));
const evidence = { fixture: fixture.root, binary: fixture.binary, checks: [], gitCommands: [] };
const a = client(fixture.binary, fixture.settingsA), b = client(fixture.binary, fixture.settingsB);
const runGit = (folder, ...args) => git(fixture, evidence, folder, args);
const commit = (folder, message) => { runGit(folder, "add", "--all"); return runGit(folder, "commit", "-m", message); };
const task = async (peer = a, index = 0) => (await peer.call("adashi_tasks", { operation: "get", taskId: fixture.tasks[index].id })).task;
const update = async (peer, title, index = 0) => {
  const current = await task(peer, index);
  return peer.call("adashi_tasks", { operation: "update", operationId: randomUUID(), taskId: current.id, expectedVersion: current.version, title });
};
const scope = peer => peer.call("adashi_design", { operation: "get_scope", elementId: "app", childrenDepth: 0, includeAncestors: false });
const document = value => value.documents.find(d => d.documentId === "element:app");
const saveArgs = (doc, description) => ({ operation: "save", operationId: randomUUID(), changeIntent: "Native Git fixture",
  readTokens: [{ documentId: doc.documentId, readToken: doc.readToken }], changes: [{ op: "upsert_element", ...doc.document, description }] });
const passed = check => { evidence.checks.push(check); console.log(`PASS ${check}`); };
let desktop, browser, page;
try {
  await Promise.all([a.initialize(), b.initialize()]);
  const started = await startDesktop(fixture, path.join(path.dirname(fixture.binary), "adashi.exe"));
  desktop = started.desktop;
  browser = await chromium.connectOverCDP(started.endpoint);
  await until(async () => {
    page = browser.contexts().flatMap(c => c.pages()).find(p => /tauri\.localhost|^tauri:\/\//.test(p.url()));
    return Boolean(page);
  }, "native Tauri page");
  page.setDefaultTimeout(20000);
  const pageErrors = [];
  page.on("pageerror", error => pageErrors.push(error.stack || String(error)));
  await page.getByRole("button", { name: "Tasks", exact: true }).click();
  await page.getByRole("button", { name: new RegExp(`Task #${fixture.tasks[0].number} Git desktop task`) }).click();
  const title = page.locator(".task-editor-form label").filter({ has: page.locator("span", { hasText: /^Title$/ }) }).locator("input");
  await title.waitFor();
  const initialTask = await task();
  const initialList = await a.call("adashi_tasks", { operation: "list" });
  for (const item of initialList.tasks) {
    await page.getByRole("button", { name: new RegExp(`Task #${item.number} ${item.title}`) }).waitFor();
    const resolved = await a.call("adashi_tasks", { operation: "get", taskNumber: item.number, expectedRevision: initialList.revision });
    assert.equal(resolved.task.id, item.id);
  }
  passed("native task labels and MCP taskNumber lookup identify the same permanent tasks");
  const beforeNoop = await canonical(fixture.repoA);
  await page.evaluate(input => window.__TAURI_INTERNALS__.invoke("update_task", { input }), {
    projectId: "fixture", taskId: initialTask.id, expectedVersion: initialTask.version,
    operationId: randomUUID(), title: initialTask.title,
  });
  assert.deepEqual(await canonical(fixture.repoA), beforeNoop);
  assert.equal((await task()).version, initialTask.version);
  assert.equal(runGit(fixture.repoA, "status", "--porcelain"), "");
  assert.ok(!Object.keys(beforeNoop).some(name => name.includes("mutation_operations")));
  passed("native fresh no-op leaves Git clean and creates no tracked request history");
  const oldDocument = document(await scope(a));
  await title.fill("Desktop draft before Git pull");
  await update(b, "Bob pulled title");
  await b.call("adashi_design", saveArgs(document(await scope(b)), "Bob pulled design"));
  commit(fixture.repoB, "Bob task and design work");
  runGit(fixture.repoB, "push", "origin", "HEAD:refs/heads/native-peer");
  runGit(fixture.repoA, "pull", "--no-rebase", "origin", "native-peer");
  await page.getByRole("heading", { name: "Bob pulled title", exact: true }).waitFor();
  assert.equal(await title.inputValue(), "Desktop draft before Git pull");
  await title.press("Tab");
  await page.getByRole("button", { name: "Close error dialog" }).waitFor();
  assert.equal((await task()).title, "Bob pulled title");
  await page.screenshot({ path: path.join(fixture.root, "native-git-stale-draft.png"), fullPage: true });
  await page.getByRole("button", { name: "Close error dialog" }).click();
  assert.equal(await title.inputValue(), "Desktop draft before Git pull");
  assertError(await a.raw("adashi_design", saveArgs(oldDocument, "Stale design must fail")), /out_of_date/);
  assert.equal(document(await scope(a)).document.description, "Bob pulled design");
  await page.getByRole("button", { name: "Use current value", exact: true }).click();
  passed("real Git pull refreshes native data, preserves dirty drafts and rejects stale desktop/MCP saves");

  await title.fill("Draft across branch reversal");
  runGit(fixture.repoA, "switch", "-c", "native-old", fixture.base);
  await page.getByRole("heading", { name: "Git desktop task", exact: true }).waitFor();
  assert.equal(await title.inputValue(), "Draft across branch reversal");
  await title.press("Tab");
  await page.getByRole("button", { name: "Close error dialog" }).waitFor();
  assert.equal((await task()).title, "Git desktop task");
  await page.getByRole("button", { name: "Close error dialog" }).click();
  await page.getByRole("button", { name: "Use current value", exact: true }).click();
  await title.fill("Draft after explicit base reload");
  runGit(fixture.repoA, "switch", "main");
  await page.getByRole("heading", { name: "Bob pulled title", exact: true }).waitFor();
  assert.equal(await title.inputValue(), "Draft after explicit base reload");
  const filesBeforeReload = await canonical(fixture.repoA);
  await page.getByRole("button", { name: "Use current value", exact: true }).click();
  assert.equal(await title.inputValue(), "Bob pulled title");
  assert.deepEqual(await canonical(fixture.repoA), filesBeforeReload, "reload must preserve stored data");
  await title.fill("Alice desktop edit after reload");
  await title.press("Tab");
  await until(async () => (await task()).title === "Alice desktop edit after reload", "native save after branch recovery");
  assert.equal(await page.getByRole("button", { name: "Close error dialog" }).count(), 0, "discarding the old draft must not cause an asynchronous stale-save error");
  await a.call("adashi_design", saveArgs(document(await scope(a)), "Bob pulled design; Alice reviewed"));
  commit(fixture.repoA, "Alice native edit and reviewed design");
  passed("branch switching in both directions preserves drafts and explicit reload allows a guarded native edit");

  await update(b, "Bob overlapping branch title");
  await update(b, "Bob independent task survives", 1);
  commit(fixture.repoB, "Bob overlap and independent work");
  runGit(fixture.repoB, "push", "origin", "HEAD:refs/heads/native-overlap");
  runGit(fixture.repoA, "fetch", "origin", "native-overlap");
  const preConflict = await task();
  await title.fill("Draft preserved during unresolved merge");
  git(fixture, evidence, fixture.repoA, ["merge", "--no-edit", "FETCH_HEAD"], true);
  const conflictBytes = await canonical(fixture.repoA);
  assert.ok(Object.values(conflictBytes).some(v => v.includes("<<<<<<<")));
  assertError(await a.raw("adashi_tasks", { operation: "get", taskId: preConflict.id }));
  assertError(await a.raw("adashi_tasks", { operation: "update", taskId: preConflict.id, expectedVersion: preConflict.version, operationId: randomUUID(), title: "Must fail" }));
  await page.getByRole("button", { name: "Close error dialog" }).waitFor();
  assert.equal(await title.inputValue(), "Draft preserved during unresolved merge");
  assert.deepEqual(await canonical(fixture.repoA), conflictBytes);
  await page.screenshot({ path: path.join(fixture.root, "native-git-merge-conflict.png"), fullPage: true });
  const unresolved = runGit(fixture.repoA, "diff", "--name-only", "--diff-filter=U").split(/\r?\n/);
  runGit(fixture.repoA, "checkout", "--ours", "--", ...unresolved);
  for (const [relative, content] of Object.entries(await canonical(fixture.repoA))) {
    if (!relative.startsWith("records" + path.sep + "agent_tasks")) continue;
    const record = JSON.parse(content);
    if (record.data.id !== preConflict.id) continue;
    record.data.title = "Reviewed Alice and Bob title";
    await fs.writeFile(path.join(fixture.repoA, ".adashi/text", relative), JSON.stringify(record, null, 2) + "\n");
  }
  commit(fixture.repoA, "Explicit combined resolution retains independent work");
  await page.getByRole("button", { name: "Close error dialog" }).click();
  await page.getByRole("heading", { name: "Reviewed Alice and Bob title", exact: true }).waitFor();
  assert.equal(await title.inputValue(), "Draft preserved during unresolved merge");
  assert.equal((await task(a, 1)).title, "Bob independent task survives");
  assert.equal(document(await scope(a)).document.description, "Bob pulled design; Alice reviewed");
  await page.getByRole("button", { name: "Use current value", exact: true }).click();
  passed("native conflict visibility and MCP write blocking preserve files and drafts until explicit Git resolution");

  const beforeTasks = [await task(), await task(a, 1)];
  const beforeJob = (await a.call("adashi_qa", { operation: "get_job", qaJobId: fixture.job.id })).job;
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  const panel = page.getByRole("region", { name: "Project storage", exact: true });
  for (const [target, label] of [["sqlite", "SQLite database"], ["text", "Git text files"]]) {
    await panel.getByRole("combobox").selectOption(target);
    await panel.getByRole("button", { name: "Review conversion", exact: true }).click();
    await panel.getByRole("heading", { name: / → / }).waitFor();
    const replace = panel.getByRole("checkbox", { name: "Preserve the existing destination in a backup and replace it" });
    if (await replace.count()) await replace.check();
    await panel.getByRole("button", { name: "Convert and switch", exact: true }).click();
    await panel.getByText(`Converted and switched to ${label}.`, { exact: true }).waitFor();
    for (let i = 0; i < 2; i++) {
      const current = await task(a, i);
      for (const field of ["id", "number", "title", "description", "designSpecificationLinks"]) assert.deepEqual(current[field], beforeTasks[i][field]);
    }
    const currentJob = (await a.call("adashi_qa", { operation: "get_job", qaJobId: fixture.job.id })).job;
    for (const field of ["id", "number", "taskLinks"]) assert.deepEqual(currentJob[field], beforeJob[field]);
  }
  await page.screenshot({ path: path.join(fixture.root, "native-git-roundtrip.png"), fullPage: true });
  const finalFiles = await canonical(fixture.repoA);
  assert.ok(!Object.keys(finalFiles).some(name => name.includes("mutation_operations")));
  assert.equal(JSON.parse(finalFiles["format.json"]).schemaVersion, 3);
  await checkTaskNumbers(fixture, a, page, passed);
  assert.deepEqual(pageErrors, []);
  evidence.tasks = [await task(), await task(a, 1)];
  evidence.success = true;
  passed("merged clone survives native text/SQLite/text conversion with IDs, numbers and references intact");
} catch (error) {
  evidence.error = String(error.stack || error);
  process.exitCode = 1;
  console.error(evidence.error);
  if (page) {
    await fs.writeFile(path.join(fixture.root, "native-failure.html"), await page.content()).catch(() => {});
    await page.screenshot({ path: path.join(fixture.root, "native-failure.png") }).catch(() => {});
  }
} finally {
  await fs.writeFile(path.join(fixture.root, "native-evidence.json"), JSON.stringify(evidence, null, 2));
  console.log(`Evidence: ${fixture.root}`);
  await browser?.close().catch(() => {});
  desktop?.kill(); a.close(); b.close();
}
