/** Native geometry checks: an attached SVG can still live in a collapsed grid row. */
import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import { randomUUID } from "node:crypto";

export async function verifyDesignLayout({ page, call, until, passed, fixture }) {
  const measurements = [];
  async function check(label) {
    // The index shows the tree plus its unconnected artifacts, or a flat listing of one kind.
    const listing = page.locator(".design-index-panel > .design-index-body, .design-index-panel > .design-list").first();
    assert.ok((await listing.boundingBox()).height >= 120,
      `${label}: design index must retain a usable scroll area`);
    const geometry = await page.locator(".design-main").evaluate(main => {
      const rect = selector => {
        const { top, bottom, width, height } = main.querySelector(selector).getBoundingClientRect();
        return { top, bottom, width, height };
      };
      return { main: main.getBoundingClientRect().toJSON(), breadcrumbs: rect(".design-breadcrumbs"),
        viewer: rect(".design-viewer-panel"), resizer: rect(".panel-resizer"), source: rect(".source-panel") };
    });
    measurements.push({ label, ...geometry });
    assert.ok(geometry.viewer.height > 250, `${label}: diagram collapsed to ${geometry.viewer.height}px`);
    assert.ok(geometry.breadcrumbs.height < 80, `${label}: breadcrumbs absorbed diagram space`);
    assert.ok(geometry.resizer.height >= 12 && geometry.resizer.height <= 14, `${label}: stretched resize handle`);
    assert.ok(geometry.viewer.bottom <= geometry.resizer.top + 1);
    assert.ok(geometry.source.top >= geometry.resizer.bottom - 1);
    assert.ok(geometry.source.height >= 120 && geometry.source.bottom <= geometry.main.bottom + 1);
  }

  await check("C4 without Markdown links");
  const root = (await call("adashi_design", { operation: "get_overview" })).elements.find(e => e.elementType === "Software System");
  await call("adashi_design", { operation: "save", operationId: randomUUID(), changeIntent: "Native layout fixture", changes: [
    { op: "upsert_uml", key: "layout-sequence", title: "Layout sequence", language: "mermaid", diagramType: "sequence",
      attachedToExternalId: root.externalId, source: "sequenceDiagram\n participant User\n participant Adashi\n User->>Adashi: Open design\n Adashi-->>User: Render complete diagram\n" },
    { op: "upsert_markdown", externalId: "layout-document", title: "Layout document", body: "# Layout document\n\nThe document panel remains usable.\n",
      designLinks: [{ targetType: "element", designExternalId: root.externalId }] },
  ] });
  await page.locator(".design-level-tabs").getByRole("tab", { name: "Documents", exact: true }).click();
  await page.getByRole("navigation", { name: "Design documents" }).getByRole("button", { name: "Layout document", exact: true }).waitFor();
  await page.locator(".design-level-tabs").getByRole("tab", { name: "Tree View", exact: true }).click();
  await page.locator(`[data-design-entity-id="${root.externalId}"]`).click();
  await page.locator(".document-artifacts").getByRole("button", { name: "Layout document", exact: true }).waitFor();
  for (const viewport of [{ width: 1120, height: 900 }, { width: 1920, height: 1080 }, { width: 1440, height: 940 }]) {
    await page.setViewportSize(viewport);
    // Tree View lists the whole tree; the shown element decides the C4 level of the diagram.
    await page.locator(".design-level-tabs").getByRole("tab", { name: "Tree View", exact: true }).click();
    await page.locator(`[data-design-entity-id="${root.externalId}"]`).click();
    await page.frameLocator('iframe[title="Structurizr C4 diagram"]').locator("svg").waitFor({ state: "attached" });
    await check(`C4 linked ${viewport.width}`);
    await page.locator(".design-level-tabs").getByRole("tab", { name: "UML Artifacts", exact: true }).click();
    await page.locator(".mermaid-content svg").waitFor({ state: "visible" });
    await check(`UML linked ${viewport.width}`);
    await page.locator(".design-viewer-panel").scrollIntoViewIfNeeded();
    await page.screenshot({ path: path.join(fixture, `layout-uml-${viewport.width}.png`), fullPage: true });
  }
  const source = page.locator(".design-main .source-panel");
  const before = await source.boundingBox();
  const handle = await page.getByRole("button", { name: "Resize source view", exact: true }).boundingBox();
  await page.mouse.move(handle.x + handle.width / 2, handle.y + handle.height / 2);
  await page.mouse.down();
  await page.mouse.move(handle.x + handle.width / 2, handle.y - 60, { steps: 5 });
  await page.mouse.up();
  await until(async () => (await source.boundingBox()).height > before.height + 30, "source pane resizes independently");
  await check("UML after source resize");
  await page.locator(".document-artifacts").getByRole("button", { name: "Layout document", exact: true }).click();
  await page.getByRole("region", { name: "Edit design document", exact: true }).waitFor();
  await page.locator(".document-workspace").waitFor();
  assert.ok((await page.locator(".document-workspace").boundingBox()).height > 400);
  // A document keeps the tree breadcrumb on top, the index on the left and its own inspector on the right.
  assert.equal(await page.locator(".design-breadcrumbs").isVisible(), true);
  assert.equal(await page.locator(".design-index-panel").isVisible(), true);
  await page.locator(".design-inspector-panel").getByRole("heading", { name: "Layout document", exact: true }).waitFor();
  await page.screenshot({ path: path.join(fixture, "layout-documents.png"), fullPage: true });
  await page.locator(".design-breadcrumbs").getByRole("button", { name: "System Context", exact: true }).click();
  await check("return from Documents");
  await fs.writeFile(path.join(fixture, "layout-measurements.json"), JSON.stringify(measurements, null, 2));
  passed("native C4/UML geometry with/without Markdown links, compact/wide window sizes, source resizing and Documents navigation");
}
