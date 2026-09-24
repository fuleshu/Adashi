/** Isolated Git, MCP and WebView2 process support for the native Git scenarios. */
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs/promises";
import net from "node:net";
import path from "node:path";
import readline from "node:readline";
import { setTimeout as delay } from "node:timers/promises";

export async function until(check, message) {
  const start = Date.now();
  while (Date.now() - start < 25000) {
    if (await check()) return;
    await delay(200);
  }
  throw new Error(`Timed out: ${message}`);
}

export function git(fixture, evidence, folder, args, conflict = false) {
  assert.ok(path.resolve(folder).startsWith(path.resolve(fixture.root) + path.sep));
  const result = spawnSync("git", ["-c", "commit.gpgsign=false", "-c", `core.hooksPath=${path.join(fixture.root, "empty-hooks")}`, "-C", folder, ...args],
    { encoding: "utf8", windowsHide: true, timeout: 30000 });
  evidence.gitCommands.push({ folder, args, exitCode: result.status, output: result.stdout + result.stderr });
  assert.equal(result.status !== 0, conflict, result.error?.stack || result.stdout + result.stderr);
  return result.stdout.trim();
}

export function client(binary, settings) {
  const process = spawn(binary, [], { env: { ...globalThis.process.env, LOCALAPPDATA: settings, XDG_CONFIG_HOME: settings }, windowsHide: true, stdio: ["pipe", "pipe", "pipe"] });
  const pending = new Map();
  let sequence = 0, errors = "";
  process.stderr.on("data", chunk => { errors += chunk; });
  readline.createInterface({ input: process.stdout }).on("line", line => {
    const response = JSON.parse(line), request = pending.get(response.id);
    if (!request) return;
    clearTimeout(request.timeout);
    pending.delete(response.id);
    request.resolve(response);
  });
  function request(method, params) {
    const id = ++sequence;
    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => { pending.delete(id); reject(new Error(`MCP timeout: ${method}\n${errors}`)); }, 25000);
      pending.set(id, { resolve, reject, timeout });
      process.stdin.write(JSON.stringify({ jsonrpc: "2.0", id, method, params }) + "\n");
    });
  }
  async function raw(name, args) {
    return request("tools/call", { name, arguments: { projectName: "fixture", ...args } });
  }
  return { process, raw,
    async initialize() {
      await request("initialize", { protocolVersion: "2025-11-25", capabilities: {}, clientInfo: { name: "native-git-check", version: "1" } });
      process.stdin.write(JSON.stringify({ jsonrpc: "2.0", method: "notifications/initialized" }) + "\n");
    },
    async call(name, args) {
      const response = await raw(name, args);
      assert.ok(!response.error && !response.result.isError, JSON.stringify(response));
      return response.result.structuredContent || JSON.parse(response.result.content[0].text);
    },
    close() { process.stdin.end(); process.kill(); },
  };
}

export async function startDesktop(fixture, binary) {
  const server = net.createServer();
  await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
  const port = server.address().port;
  await new Promise(resolve => server.close(resolve));
  const desktop = spawn(binary, [], { windowsHide: true, stdio: "ignore", env: {
    ...process.env, LOCALAPPDATA: fixture.settingsA, XDG_CONFIG_HOME: fixture.settingsA,
    WEBVIEW2_USER_DATA_FOLDER: path.join(fixture.root, "native-webview"),
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`,
  } });
  try {
    await until(async () => {
      if (desktop.exitCode !== null) throw new Error(`Desktop exited: ${desktop.exitCode}`);
      try { return (await fetch(`http://127.0.0.1:${port}/json/version`)).ok; } catch { return false; }
    }, "native WebView2 endpoint");
    return { desktop, endpoint: `http://127.0.0.1:${port}` };
  } catch (error) { desktop.kill(); throw error; }
}

export async function canonical(folder) {
  const root = path.join(folder, ".adashi/text"), result = {};
  for (const item of await fs.readdir(root, { recursive: true, withFileTypes: true })) {
    if (!item.isFile() || !item.name.endsWith(".json")) continue;
    const filename = path.join(item.parentPath || item.path, item.name);
    result[path.relative(root, filename)] = await fs.readFile(filename, "utf8");
  }
  return result;
}

export function assertError(response, pattern) {
  assert.ok(response.error || response.result?.isError, JSON.stringify(response));
  if (pattern) assert.match(JSON.stringify(response), pattern);
}
