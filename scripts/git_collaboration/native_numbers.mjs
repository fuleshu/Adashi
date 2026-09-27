/** Verify numbering changes in the actual WebView while preserving selection. */
import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import { randomUUID } from "node:crypto";

export async function checkTaskNumbers(fixture, peer, page, passed) {
  const get = async id => (await peer.call("adashi_tasks", { operation: "get", taskId: id })).task;
  const mutate = async args => (await peer.call("adashi_tasks", { ...args, operationId: randomUUID() })).task;
  const selected = await get(fixture.tasks[0].id);
  await page.getByRole("button", { name: "Tasks", exact: true }).click();
  await page.getByRole("button", { name: new RegExp(`Task #${selected.number} ${selected.title}`) }).click();
  const added = await mutate({ operation: "create", title: "Earlier imported task" });
  const directory = path.join(fixture.repoA, ".adashi/text/records/agent_tasks");
  for (const filename of await fs.readdir(directory)) {
    const file = path.join(directory, filename);
    const record = JSON.parse(await fs.readFile(file, "utf8"));
    if (record.data.id !== added.id) continue;
    record.data.created_at = "2000-01-01 00:00:00";
    await fs.writeFile(file, JSON.stringify(record, null, 2) + "\n");
  }
  await page.getByRole("button", { name: /Task #1 Earlier imported task/ }).waitFor();
  await page.getByText("Task #2", { exact: true }).waitFor();
  assert.deepEqual(await page.locator(".task-list-item strong").filter({ hasText: /^Task #/ }).allTextContents(), [
    "Task #1 Earlier imported task", `Task #2 ${selected.title}`, `Task #3 ${(await get(fixture.tasks[1].id)).title}`,
  ]);
  const numbered = await peer.call("adashi_tasks", { operation: "get", taskNumber: 2 });
  assert.equal(numbered.task.id, selected.id);
  assert.equal(numbered.task.version, selected.version);
  await page.screenshot({ path: path.join(fixture.root, "native-task-numbers.png"), fullPage: true });
  const current = await get(added.id);
  const active = await mutate({ operation: "update", taskId: current.id, expectedVersion: current.version, state: "active" });
  const finished = await mutate({ operation: "finish", taskId: active.id, expectedVersion: active.version, completionMemo: "Native numbering verification" });
  const closed = await mutate({ operation: "close", taskId: finished.id, expectedVersion: finished.version });
  await page.getByRole("button", { name: /Task #1 Earlier imported task/ }).waitFor({ state: "hidden" });
  assert.deepEqual((await peer.call("adashi_tasks", { operation: "list" })).tasks.map(t => t.number), [2, 3]);
  await page.getByText("Task #2", { exact: true }).waitFor();
  await peer.call("adashi_tasks", { operation: "delete", operationId: randomUUID(), taskId: closed.id, expectedVersion: closed.version });
  await page.getByText("Task #1", { exact: true }).waitFor();
  assert.equal((await peer.call("adashi_tasks", { operation: "get", taskNumber: 1 })).task.id, selected.id);
  passed("native creation-order renumbering preserves selected task identity and agrees with filtered MCP lists");
}
