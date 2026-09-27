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
  await fs.mkdir(path.join(project, "Specifications/Adashi"), { recursive: true });
  await directory.fill("specifications/adashi");
  await page.getByRole("button", { name: "Save output settings", exact: true }).click();
  const enabled = page.getByRole("checkbox", { name: "Write architecture projections into this project" });
  if (await enabled.isChecked()) {
    await enabled.click();
    await until(async () => !(await enabled.isChecked()), "reset fixture projection before enable check");
  }
  const instructions = path.join(project, "agents.md");
  await fs.writeFile(instructions, "Handwritten project instructions\n");
  await enabled.click();
  await until(() => enabled.isChecked(), "projection enabled after asynchronous settings save");
  const filename = "design-" + createHash("sha256").update("native-prose").digest("hex") + ".md";
  const output = path.join(project, "Specifications/Adashi", filename);
  await until(async () => (await fs.readFile(output, "utf8").catch(() => "")).endsWith(body), "native output configuration creates full Markdown");
  const rootNames = (await fs.readdir(project)).filter(name => name.toLowerCase() === "agents.md");
  assert.deepEqual(rootNames, ["agents.md"]);
  const rootText = await fs.readFile(instructions, "utf8");
  assert.ok(rootText.startsWith("Handwritten project instructions\n"));
  assert.ok(rootText.includes("Specifications/Adashi/index.md"));
  await page.getByRole("textbox", { name: "Per-project architecture projection file name", exact: true }).fill("aGeNtS.MD");
  await page.getByRole("button", { name: "Save output settings", exact: true }).click();
  await until(async () => (await page.locator(".projection-file-name-row code").innerText()) === "aGeNtS.MD", "case-only file name setting is applied");
  assert.deepEqual((await fs.readdir(project)).filter(name => name.toLowerCase() === "agents.md"), ["agents.md"]);
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
  assert.equal(await fs.readFile(instructions, "utf8"), "Handwritten project instructions\n");
  passed("native mixed-case instruction/output lookup, complete Markdown, stable mtimes, MCP refresh, drift repair and opt-out preserving handwritten content");
}
