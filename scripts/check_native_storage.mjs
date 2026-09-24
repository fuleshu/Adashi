/** Native WebView2/MCP integration test. Does not send OS mouse/keyboard input.
 * Build frontend + desktop first. Requires Playwright (or ADASHI_PLAYWRIGHT_PACKAGE).
 * Creates its own settings, database, WebView profile and desktop/MCP processes.
 * Leaves evidence under target/native-storage; never opens the user's projects.
 */
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import fs from "node:fs/promises";
import { createRequire } from "node:module";
import net from "node:net";
import path from "node:path";
import readline from "node:readline";
import { fileURLToPath } from "node:url";
import { setTimeout as delay } from "node:timers/promises";

const { chromium } = createRequire(import.meta.url)(process.env.ADASHI_PLAYWRIGHT_PACKAGE || "playwright");
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
let textBackend = process.env.ADASHI_TEST_BACKEND === "text";
const checkMigration = process.env.ADASHI_TEST_MIGRATION === "1";
const fixture = path.join(root, "target/native-storage", randomUUID());
const project = path.join(fixture, "project");
const binary = path.resolve(process.argv[2] || path.join(root, "src-tauri/target/debug/adashi.exe"));
const mcpBinary = path.join(path.dirname(binary), "adashi-mcp.exe");
const env = { ...process.env, LOCALAPPDATA: fixture, XDG_CONFIG_HOME: fixture,
  WEBVIEW2_USER_DATA_FOLDER: path.join(fixture, "webview") };
const settings = { window: { width: 1440, height: 940, x: null, y: null },
  projects: [{ id: "fixture", name: "Native storage fixture", folder: project }],
  lastActiveProjectId: "fixture", ruleTemplates: [],
  architectureProjection: { enabled: false, fileName: "AGENTS.md" } };
if (checkMigration) settings.projects.push({ id: "independent", name: "Independent project", folder: path.join(fixture, "independent") });
if (textBackend) {
  await fs.mkdir(path.join(project, ".adashi"), { recursive: true });
  await fs.writeFile(path.join(project, ".adashi/storage.json"), JSON.stringify({ schemaVersion: 1, backend: { kind: "text" } }));
}
for (const directory of ["Adashi", "adashi"]) {
  await fs.mkdir(path.join(fixture, directory), { recursive: true });
  await fs.writeFile(path.join(fixture, directory, "settings.json"), JSON.stringify(settings));
}

const mcp = spawn(mcpBinary, [], { env, windowsHide: true, stdio: ["pipe", "pipe", "pipe"] });
const pending = new Map();
let sequence = 0;
readline.createInterface({ input: mcp.stdout }).on("line", line => {
  const response = JSON.parse(line);
  const request = pending.get(response.id);
  if (!request) return;
  pending.delete(response.id);
  clearTimeout(request.timeout);
  request.resolve(response);
});
let mcpErrors = "";
mcp.stderr.on("data", data => { mcpErrors += data; });
function request(method, params) {
  const id = ++sequence;
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => { pending.delete(id); reject(new Error(`MCP timed out: ${method}\n${mcpErrors}`)); }, 20000);
    pending.set(id, { resolve, reject, timeout });
    mcp.stdin.write(JSON.stringify({ jsonrpc: "2.0", id, method, params }) + "\n");
  });
}
async function call(name, args) {
  const response = await request("tools/call", { name, arguments: { projectName: "fixture", ...args } });
  assert.ok(!response.error, JSON.stringify(response));
  assert.ok(!response.result.isError, JSON.stringify(response));
  return response.result.structuredContent || JSON.parse(response.result.content[0].text);
}
const taskIds = [];
const getTask = async id => (await call("adashi_tasks", { operation: "get", taskId: taskIds[id - 1] })).task;
async function updateTask(id, title) {
  const task = await getTask(id);
  return (await call("adashi_tasks", { operation: "update", taskId: task.id, expectedVersion: task.version,
    operationId: randomUUID(), title })).task;
}
async function until(check, message) {
  const start = Date.now();
  while (Date.now() - start < 20000) {
    if (await check()) return;
    await delay(200);
  }
  throw new Error(`Timed out: ${message}`);
}
async function fingerprint() {
  if (textBackend) {
    const base = path.join(project, ".adashi/text");
    const files = (await fs.readdir(base, { recursive: true, withFileTypes: true })).filter(entry => entry.isFile());
    const result = {};
    for (const entry of files) {
      const filename = path.join(entry.parentPath || entry.path, entry.name);
      const stat = await fs.stat(filename, { bigint: true });
      result[path.relative(base, filename)] = { sha256: createHash("sha256").update(await fs.readFile(filename)).digest("hex"), bytes: String(stat.size), mtimeNs: String(stat.mtimeNs) };
    }
    return result;
  }
  const filename = path.join(project, ".adashi/adashi.sqlite3");
  const stat = await fs.stat(filename, { bigint: true });
  return { sha256: createHash("sha256").update(await fs.readFile(filename)).digest("hex"),
    bytes: String(stat.size), mtimeNs: String(stat.mtimeNs) };
}
const evidence = { fixture, binary, backend: textBackend ? "text" : "sqlite", checks: [] };
function passed(check) { evidence.checks.push(check); console.log(`PASS ${check}`); }
let browser, desktop, page;
try {
  await request("initialize", { protocolVersion: "2025-11-25", capabilities: {}, clientInfo: { name: "native-storage-check", version: "1" } });
  mcp.stdin.write(JSON.stringify({ jsonrpc: "2.0", method: "notifications/initialized" }) + "\n");
  for (const title of ["Desktop edit fixture", "Peer edit fixture"]) {
    taskIds.push((await call("adashi_tasks", { operation: "create", operationId: randomUUID(), title })).task.id);
  }
  if (checkMigration) await call("adashi_tasks", { projectName: "independent", operation: "create", operationId: randomUUID(), title: "Other project remains SQLite" });
  await call("adashi_rules", { operation: "create", operationId: randomUUID(), name: "Markdown fixture",
    enabled: true, intend: "implementation", hook: "task.start", prompt: "Original Markdown" });
  const initial = await getTask(1);
  await call("adashi_tasks", { operation: "update", taskId: initial.id, expectedVersion: initial.version,
    operationId: randomUUID(), state: "active" });
  const baseline = await fingerprint();
  const server = net.createServer();
  await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
  const port = server.address().port;
  await new Promise(resolve => server.close(resolve));
  desktop = spawn(binary, [], { cwd: root, env: { ...env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}` },
    windowsHide: true, stdio: "ignore" });
  await until(async () => {
    if (desktop.exitCode !== null) throw new Error(`Desktop exited: ${desktop.exitCode}`);
    try { return (await fetch(`http://127.0.0.1:${port}/json/version`)).ok; } catch { return false; }
  }, "fixture WebView2 debug endpoint");
  browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
  await until(async () => {
    page = browser.contexts().flatMap(context => context.pages()).find(candidate => /tauri\.localhost|^tauri:\/\//.test(candidate.url()));
    return Boolean(page);
  }, "native Tauri page");
  page.setDefaultTimeout(15000);
  const pageErrors = [];
  page.on("pageerror", error => pageErrors.push(error.stack || String(error)));
  await page.getByRole("button", { name: "Tasks", exact: true }).click();
  await page.getByRole("button", { name: new RegExp(`Task Id ${taskIds[0]} Desktop edit fixture`) }).click();
  const title = page.locator(".task-editor-form label").filter({ has: page.locator("span", { hasText: /^Title$/ }) }).locator("input");
  await title.waitFor();
  await delay(2500); // Observe at least one normal desktop revision poll.
  assert.deepEqual(await fingerprint(), baseline);
  passed("native startup and polling preserve database bytes and mtime");

  await title.fill("Desktop draft survived external refresh");
  await updateTask(2, "MCP update visible while draft open");
  await page.getByRole("button", { name: new RegExp(`Task Id ${taskIds[1]} MCP update visible while draft open`) }).waitFor();
  assert.equal(await title.inputValue(), "Desktop draft survived external refresh");
  assert.equal((await getTask(1)).title, "Desktop edit fixture");
  await title.press("Tab");
  await until(async () => (await getTask(1)).title === "Desktop draft survived external refresh", "desktop save visible to MCP");
  passed("disjoint MCP refresh preserves unsaved desktop draft and desktop save reaches MCP");

  await title.fill("Unsaved conflicting title");
  const remote = await updateTask(1, "Protected MCP title");
  await page.getByRole("heading", { name: "Protected MCP title", exact: true }).waitFor();
  assert.equal(await title.inputValue(), "Unsaved conflicting title");
  await title.press("Tab");
  await page.getByRole("button", { name: "Close error dialog" }).waitFor();
  assert.equal((await getTask(1)).version, remote.version);
  assert.equal((await getTask(1)).title, remote.title);
  await page.getByRole("button", { name: "Close error dialog" }).click();
  assert.equal(await title.inputValue(), "Unsaved conflicting title");
  await page.screenshot({ path: path.join(fixture, "native-conflict.png"), fullPage: true });
  await page.getByRole("button", { name: "Use current value", exact: true }).click();
  assert.equal(await title.inputValue(), remote.title);
  await title.fill("Edit after explicit reload");
  await title.press("Tab");
  await until(async () => (await getTask(1)).title === "Edit after explicit reload", "save after explicit conflict resolution");
  passed("same-task stale edit is rejected; draft retained; explicit reload permits a new edit");

  const memo = page.getByRole("textbox", { name: "Memo", exact: true });
  await memo.fill("My pending completion");
  const completing = await getTask(1);
  await call("adashi_tasks", { operation: "finish", taskId: completing.id, expectedVersion: completing.version,
    operationId: randomUUID(), completionMemo: "Peer completion", createdFiles: [], changedFiles: [] });
  await page.locator(".task-editor-form select").getByRole("option", { name: "finished", selected: true }).waitFor({ state: "attached" });
  assert.equal(await memo.inputValue(), "My pending completion");
  await page.getByRole("button", { name: "Finish", exact: true }).click();
  await page.getByRole("button", { name: "Close error dialog" }).waitFor();
  assert.equal((await getTask(1)).completionMemo, "Peer completion");
  await page.getByRole("button", { name: "Close error dialog" }).click();
  await page.getByRole("button", { name: "Use current value", exact: true }).click();
  assert.equal(await memo.inputValue(), "Peer completion");
  passed("completion form retains original version and rejects concurrent completion overwrite");

  await page.getByRole("button", { name: "Rules", exact: true }).click();
  const editor = page.locator(".rules-editor-panel .CodeMirror");
  await editor.waitFor();
  const markdown = editor.locator("textarea");
  // CodeMirror requires real DOM key events rather than filling its hidden source textarea.
  await editor.click();
  await markdown.press("ControlOrMeta+A");
  await markdown.pressSequentially("My unsaved Markdown");
  let rule = (await call("adashi_rules", { operation: "list" })).rules.find(record => record.name === "Markdown fixture");
  await call("adashi_rules", { operation: "update", ruleId: rule.id, expectedVersion: rule.version,
    operationId: randomUUID(), name: rule.name, enabled: rule.enabled, intend: rule.intend, hook: rule.hook,
    prompt: "Peer Markdown" });
  await page.getByRole("button", { name: "Use current value", exact: true }).waitFor();
  assert.ok((await editor.innerText()).includes("My unsaved Markdown"));
  await page.getByRole("textbox", { name: "Name", exact: true }).click();
  await page.getByRole("button", { name: "Close error dialog" }).waitFor();
  rule = (await call("adashi_rules", { operation: "list" })).rules.find(record => record.name === "Markdown fixture");
  assert.equal(rule.prompt, "Peer Markdown");
  await page.getByRole("button", { name: "Close error dialog" }).click();
  await page.getByRole("button", { name: "Use current value", exact: true }).click();
  assert.ok((await editor.innerText()).includes("Peer Markdown"));
  passed("Markdown editor mounts, retains dirty content and rejects a stale save");

  await page.getByRole("button", { name: "Memory", exact: true }).click();
  await page.locator(".memory-rule-panel > summary").click();
  await page.locator(".CodeMirror:visible").first().waitFor();
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.locator(".CodeMirror:visible").first().waitFor();
  await page.getByRole("button", { name: "QA", exact: true }).click();
  await page.getByRole("button", { name: "Design", exact: true }).click();
  await page.frameLocator('iframe[title="Structurizr C4 diagram"]').locator("svg").waitFor({ state: "attached" });
  await delay(250);
  assert.deepEqual(pageErrors, []);
  passed("rules, memory, settings, QA and restored design views render without JavaScript errors");
  if (checkMigration) {
    const { verifyMigration } = await import("./check_native_storage_migration.mjs");
    await verifyMigration({ page, call, request, getTask, updateTask, taskIds, until, passed, project, fixture, evidence });
    textBackend = true;
  }
  evidence.finalTasks = [await getTask(1), await getTask(2)];
  evidence.database = await fingerprint();
  evidence.success = true;
} catch (error) {
  evidence.error = String(error.stack || error);
  if (page) {
    await fs.writeFile(path.join(fixture, "failure.html"), await page.content()).catch(() => {});
    await page.screenshot({ path: path.join(fixture, "failure.png") }).catch(() => {});
  }
  process.exitCode = 1;
  console.error(evidence.error);
} finally {
  await fs.writeFile(path.join(fixture, "evidence.json"), JSON.stringify(evidence, null, 2));
  console.log(`Evidence: ${fixture}`);
  await browser?.close().catch(() => {});
  desktop?.kill();
  mcp.stdin.end();
  mcp.kill();
  for (const request of pending.values()) clearTimeout(request.timeout);
}
