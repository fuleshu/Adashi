"""Two real MCP processes and an isolated fixture for native desktop acceptance.

Default run verifies disjoint/conflicting writes and QA execution replay, leaving
two tasks for the native UI. --fixture ROOT --phase external updates the peer
task while a desktop draft is open; --phase verify checks the desktop save.
Never reads the user's settings or projects.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import os
from pathlib import Path
import sys
import uuid

sys.dont_write_bytecode = True
from check_context_efficiency import Client, ROOT, body


def fingerprint(path):
    return {"sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
            "bytes": path.stat().st_size, "mtimeNs": path.stat().st_mtime_ns}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True)
    parser.add_argument("--backend", choices=["sqlite", "text"], default="sqlite")
    parser.add_argument("--fixture")
    parser.add_argument("--phase", choices=["prepare", "external", "verify"], default="prepare")
    options = parser.parse_args()
    root = Path(options.fixture).resolve() if options.fixture else ROOT / "target/storage-parity" / str(uuid.uuid4())
    assert root.is_relative_to(ROOT / "target"), root
    folder = root / "project"
    os.environ["XDG_CONFIG_HOME"] = str(root)
    if options.phase == "prepare":
        if options.backend == "text":
            (folder / ".adashi").mkdir(parents=True, exist_ok=True)
            (folder / ".adashi/storage.json").write_text(json.dumps({"schemaVersion": 1, "backend": {"kind": "text"}}), encoding="utf-8")
        settings = {"window": {"width": 1440, "height": 940, "x": None, "y": None},
                    "projects": [{"id": "fixture", "name": "Storage parity fixture", "folder": str(folder)}],
                    "lastActiveProjectId": "fixture", "ruleTemplates": [],
                    "architectureProjection": {"enabled": False, "fileName": "AGENTS.md"}}
        for name in ("Adashi", "adashi"):
            (root / name).mkdir(parents=True, exist_ok=True)
            (root / name / "settings.json").write_text(json.dumps(settings), encoding="utf-8")
    clients = [Client(options.binary, root), Client(options.binary, root)]
    def call(peer, tool, **kwargs):
        return clients[peer].call(tool, projectName="fixture", **kwargs)
    def get(peer, task):
        return body(call(peer, "adashi_tasks", operation="get", taskId=task))["task"]
    def update(peer, task, operation, title):
        return call(peer, "adashi_tasks", operation="update", operationId=operation,
                    taskId=task["id"], expectedVersion=task["version"], title=title)
    try:
        if options.phase == "prepare":
            tasks = [body(call(0, "adashi_tasks", operation="create", operationId=f"task-{i}",
                          title=title))["task"] for i, title in enumerate(("Desktop edit fixture", "MCP edit fixture"))]
            with ThreadPoolExecutor(max_workers=2) as pool:
                results = list(pool.map(lambda p: update(p, tasks[p], f"disjoint-{p}", tasks[p]["title"] + " ready"), range(2)))
            assert all(body(r)["task"]["version"] != tasks[i]["version"] for i, r in enumerate(results))
            stale = get(0, tasks[1]["id"])
            with ThreadPoolExecutor(max_workers=2) as pool:
                results = list(pool.map(lambda p: update(p, stale, f"conflict-{p}", f"MCP conflict winner {p}"), range(2)))
            errors = [r for r in results if r.get("error") or r.get("result", {}).get("isError")]
            assert len(errors) == 1 and "resource.conflict" in json.dumps(errors[0]), results
            job = body(call(0, "adashi_qa", operation="create_job", operationId="qa-definition",
                            name="Exactly once fixture", command="Add-Content -LiteralPath 'qa-executions.txt' -Value 'once'; Write-Output 'parity-ok'" if os.name == "nt" else "echo once >> qa-executions.txt; echo parity-ok"))["job"]
            def run(peer):
                return body(call(peer, "adashi_qa", operation="run_jobs", operationId="same-run", query={"jobIds": [job["id"]]}))
            with ThreadPoolExecutor(max_workers=2) as pool:
                runs = list(pool.map(run, range(2)))
            final = run(0)
            assert "parity-ok" in json.dumps(final) and '"status": "passed"' in json.dumps(final), final
            assert (folder / "qa-executions.txt").read_text(encoding="utf-8-sig").splitlines() == ["once"]
            evidence = {"fixtureRoot": str(root), "disjointWrites": 2, "sameVersionConflictWinners": 1,
                        "qaConcurrentRetries": 2, "qaExecutions": 1,
                        "backend": options.backend,
                        "tasks": [get(0, t["id"]) for t in tasks],
                        "storageBaseline": {str(p.relative_to(folder)): fingerprint(p) for p in (folder / ".adashi/text").rglob("*.json")} if options.backend == "text" else fingerprint(folder / ".adashi/adashi.sqlite3")}
        elif options.phase == "external":
            result = body(update(0, get(0, 2), "native-external-edit", "MCP update visible while draft open"))
            evidence = {"peerEdit": result, "desktopTask": get(0, 1)}
        else:
            first, second = get(0, 1), get(0, 2)
            assert first["title"] == "Desktop draft survived external refresh", first
            assert second["title"] == "MCP update visible while draft open", second
            evidence = {"desktopSavedTask": first, "mcpPeerTask": second,
                        "database": fingerprint(folder / ".adashi/adashi.sqlite3")}
        (root / f"{options.phase}-evidence.json").write_text(json.dumps(evidence, indent=2), encoding="utf-8")
        print(json.dumps(evidence, indent=2))
    finally:
        for client in clients:
            client.close()


if __name__ == "__main__":
    main()
