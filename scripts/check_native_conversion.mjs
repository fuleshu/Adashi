/** Reproduce long conversions in an isolated native WebView using a source backup.
 * ADASHI_CONVERSION_SOURCE points to a migration's source folder. No source files
 * are modified; settings, processes, conversion and evidence belong to the fixture.
 */
import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import { randomUUID } from "node:crypto";
import { createRequire } from "node:module";
import { spawn } from "node:child_process";
import { startDesktop, until } from "./git_collaboration/native_helpers.mjs";

const { chromium } = createRequire(import.meta.url)(process.env.ADASHI_PLAYWRIGHT_PACKAGE || "playwright");
const root = path.resolve("target/native-conversion", randomUUID());
const project = path.join(root, "project");
const source = process.env.ADASHI_CONVERSION_SOURCE;
assert.ok(source, "Set ADASHI_CONVERSION_SOURCE to a migration source backup");
await fs.cp(source, project, { recursive: true });
const settings = { window: { width: 1440, height: 940, x: null, y: null },
  projects: [{ id: "fixture", name: "Conversion regression", folder: project }],
  lastActiveProjectId: "fixture", ruleTemplates: [],
  architectureProjection: { enabled: false, fileName: "AGENTS.md" } };
for (const name of ["Adashi", "adashi"]) {
  await fs.mkdir(path.join(root, name), { recursive: true });
  await fs.writeFile(path.join(root, name, "settings.json"), JSON.stringify(settings));
}
const evidence = { root, checks: [] };
const binary = path.resolve(process.argv[2] || "src-tauri/target/debug/adashi.exe");
const { desktop, endpoint } = await startDesktop({ root, settingsA: root }, binary);
let browser, page;
try {
  browser = await chromium.connectOverCDP(endpoint);
  await until(async () => {
    page = browser.contexts().flatMap(c => c.pages()).find(p => /tauri\.localhost|^tauri:\/\//.test(p.url()));
    return !!page;
  }, "native page");
  page.setDefaultTimeout(60000);
  const pageErrors = [];
  page.on("pageerror", error => pageErrors.push(String(error)));
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.evaluate(() => {
    window.conversionAlerts = [];
    new MutationObserver(() => {
      const text = [...document.querySelectorAll('[role="alert"], #error-dialog-message')].map(e => e.textContent).join("\n");
      if (text && window.conversionAlerts.at(-1)?.text !== text) window.conversionAlerts.push({ text, at: Date.now() });
    }).observe(document.body, { childList: true, subtree: true, characterData: true });
  });
  const panel = page.getByRole("region", { name: "Project storage", exact: true });
  await panel.getByRole("combobox").selectOption("text");
  await panel.getByRole("button", { name: "Review conversion", exact: true }).click();
  await panel.getByRole("heading", { name: "SQLite database → Git text files" }).waitFor();
  const started = Date.now();
  await panel.getByRole("button", { name: "Convert and switch", exact: true }).click();
  await panel.getByText("Converted and switched to Git text files.", { exact: true }).waitFor();
  await until(() => panel.getByRole("button", { name: "Review conversion", exact: true }).isEnabled(), "conversion refresh finished");
  evidence.durationMs = Date.now() - started;
  evidence.alerts = await page.evaluate(() => window.conversionAlerts);
  const descriptor = JSON.parse(await fs.readFile(path.join(project, ".adashi/storage.json"), "utf8"));
  assert.equal(descriptor.backend.kind, "text");
  const activation = JSON.parse(await fs.readFile(path.join(project, ".adashi/local/migrations", descriptor.generation, "activation.json"), "utf8"));
  for (const [name, item] of Object.entries(activation.files)) {
    const actual = await fs.readFile(path.join(project, name)).catch(e => { if (e.code === "ENOENT") return null; throw e; });
    assert.deepEqual(actual, item.after === null ? null : Buffer.from(item.after, "base64"), name);
  }
  await assert.rejects(fs.stat(path.join(project, ".adashi/local/storage-migration.json")), { code: "ENOENT" });
  evidence.checks.push(`All ${Object.keys(activation.files).length} activated files match the verified conversion`);
  console.log(`PASS ${evidence.checks.at(-1)}`);
  await page.screenshot({ path: path.join(root, "converted.png"), fullPage: true });
  if (process.env.ADASHI_REPRODUCE_TIMEOUT !== "1") {
    assert.deepEqual(pageErrors, []);
    assert.deepEqual(evidence.alerts, [], "conversion and background reads must succeed");
    assert.equal(await page.getByRole("button", { name: "Close error dialog" }).count(), 0);
    evidence.checks.push("Long native conversion produces no background timeout or disappearing error");
    console.log(`PASS ${evidence.checks.at(-1)}`);

    // Deny replacement of the old SQLite destination using a real Windows handle.
    // The native button must report rollback while its text source stays usable.
    if (process.platform === "win32") {
      const lock = spawn(process.env.ADASHI_PYTHON || "C:\\Python313\\python.exe", ["-u", "-c", `
import ctypes,sys
k=ctypes.WinDLL('kernel32',use_last_error=True)
k.CreateFileW.argtypes=[ctypes.c_wchar_p,ctypes.c_uint32,ctypes.c_uint32,ctypes.c_void_p,ctypes.c_uint32,ctypes.c_uint32,ctypes.c_void_p]
k.CreateFileW.restype=ctypes.c_void_p
k.CloseHandle.argtypes=[ctypes.c_void_p]
h=k.CreateFileW(sys.argv[1],0x80000000,1,None,3,0,None)
assert h != ctypes.c_void_p(-1).value, ctypes.get_last_error()
print('locked',flush=True)
sys.stdin.readline()
k.CloseHandle(h)
`, path.join(project, ".adashi/adashi.sqlite3")], { windowsHide: true, stdio: ["pipe", "pipe", "pipe"] });
      try {
        await new Promise((resolve, reject) => {
          lock.stdout.once("data", resolve);
          lock.once("error", reject);
          lock.once("exit", code => reject(new Error(`lock helper exited: ${code}`)));
        });
        await panel.getByRole("combobox").selectOption("sqlite");
        await panel.getByRole("button", { name: "Review conversion", exact: true }).click();
        await page.waitForFunction(() => document.querySelector(".storage-preview, .storage-error"));
        assert.equal(await panel.locator(".storage-error").count(), 0, await panel.innerText());
        await panel.getByRole("checkbox", { name: "Preserve the existing destination in a backup and replace it" }).check();
        await panel.getByRole("button", { name: "Convert and switch", exact: true }).click();
        await panel.getByRole("alert").filter({ hasText: "rolled back" }).waitFor();
        assert.deepEqual(JSON.parse(await fs.readFile(path.join(project, ".adashi/storage.json"), "utf8")), descriptor);
        for (const [name, item] of Object.entries(activation.files)) {
          const actual = await fs.readFile(path.join(project, name)).catch(e => { if (e.code === "ENOENT") return null; throw e; });
          assert.deepEqual(actual, item.after === null ? null : Buffer.from(item.after, "base64"), name);
        }
        await assert.rejects(fs.stat(path.join(project, ".adashi/local/storage-migration.json")), { code: "ENOENT" });
        const dashboard = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke("get_dashboard", { projectId: "fixture" }));
        assert.ok(dashboard.tasks.length > 0);
        await page.screenshot({ path: path.join(root, "rolled-back.png"), fullPage: true });
        evidence.checks.push("Native conversion failure rolls back, retains its error and leaves the original text project usable");
        console.log(`PASS ${evidence.checks.at(-1)}`);
      } finally { lock.stdin.end("release\n"); }
    }
    assert.deepEqual(pageErrors, []);
  }
  console.log(JSON.stringify(evidence, null, 2));
} catch (error) {
  evidence.error = String(error);
  evidence.pageText = await page?.locator("body").innerText();
  await page?.screenshot({ path: path.join(root, "failure.png"), fullPage: true });
  throw error;
} finally {
  await fs.writeFile(path.join(root, "evidence.json"), JSON.stringify(evidence, null, 2));
  await browser?.close();
  desktop.kill();
}
