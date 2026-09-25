/** Native settings controls and MCP-triggered Markdown output in an isolated fixture. */
import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import { createHash, randomUUID } from "node:crypto";

export async function verifyMarkdownProjection({ page, call, until, passed, project, fixture }) {
  const body = "# Native specification\n\nGrüße 日本語\n```rust\nlet exact = true;\n```\n";
  await call("adashi_design", { operation: "save", operationId: randomUUID(), changeIntent: "Native projection fixture", changes: [{ op: "upsert_markdown", externalId: "native-prose", title: "Native prose", body, designLinks: [] }] });
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  const directory = page.getByRole("textbox", { name: "Generated Markdown directory", exact: true });
  await directory.fill("specifications/adashi");
  await page.getByRole("button", { name: "Save output settings", exact: true }).click();
  const enabled = page.getByRole("checkbox", { name: "Write architecture projections into this project" });
  if (await enabled.isChecked()) {
    await enabled.click();
    await until(async () => !(await enabled.isChecked()), "reset fixture projection before enable check");
  }
  await enabled.click();
  await until(() => enabled.isChecked(), "projection enabled after asynchronous settings save");
  const filename = "design-" + createHash("sha256").update("native-prose").digest("hex") + ".md";
  const output = path.join(project, "specifications/adashi", filename);
  await until(async () => (await fs.readFile(output, "utf8").catch(() => "")).endsWith(body), "native output configuration creates full Markdown");
  const before = await fs.stat(output, { bigint: true });
  await page.getByRole("button", { name: "Regenerate", exact: true }).click();
  assert.equal((await fs.stat(output, { bigint: true })).mtimeNs, before.mtimeNs);
  const document = (await call("adashi_design", { operation: "get_documents", ids: ["markdown:native-prose"] })).documents[0];
  const saved = await call("adashi_design", { operation: "save", operationId: randomUUID(), changeIntent: "Observe MCP refresh", changes: [{ op: "upsert_markdown", ...document.document, body: body + "MCP edit\n" }], readTokens: [{ documentId: document.documentId, readToken: document.readToken }] });
  assert.equal(saved.projection.state, "current");
  assert.ok((await fs.readFile(output, "utf8")).endsWith("MCP edit\n"));
  await fs.writeFile(output, "External drift", "utf8");
  await page.getByRole("button", { name: "Regenerate", exact: true }).click();
  await until(async () => (await fs.readFile(output, "utf8")).endsWith("MCP edit\n"), "native retry repairs drift");
  await page.screenshot({ path: path.join(fixture, "native-markdown-projection.png"), fullPage: true });
  await enabled.click();
  await until(async () => !(await enabled.isChecked()), "projection disabled after asynchronous settings save");
  await until(async () => !(await fs.stat(output).catch(() => null)), "opt-out removes owned output");
  passed("native output settings, complete Markdown, stable mtimes, MCP refresh, drift repair and opt-out");
}
