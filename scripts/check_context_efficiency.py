"""Repeatable stdio regression and before/after size check against one saved fixture.
Uses an isolated LOCALAPPDATA; never opens the user's configured project databases.
Run before and after with the same --fixture and --output paths.
"""
import argparse
from contextlib import closing, contextmanager
import json
import os
from pathlib import Path
import queue
import shutil
import sqlite3
import subprocess
import threading
import uuid

ROOT = Path(__file__).resolve().parents[1]


@contextmanager
def scratch_directory():
    parent = (ROOT / "target/context-efficiency").resolve()
    parent.mkdir(parents=True, exist_ok=True)
    root = parent / str(uuid.uuid4())
    root.mkdir()
    try:
        yield root
    finally:
        # Delete only the unique fixture directory created in this workspace.
        assert root.resolve().parent == parent
        shutil.rmtree(root)


class Client:
    def __init__(self, binary, settings_root):
        env = dict(os.environ, LOCALAPPDATA=str(settings_root))
        self.process = subprocess.Popen(
            [str(Path(binary).resolve())], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, env=env, text=True, encoding="utf-8",
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        self.queue = queue.Queue()
        self.thread = threading.Thread(target=self._read, daemon=True)
        self.thread.start()
        self.sequence = 0
        self.request("initialize", {"protocolVersion": "2025-11-25",
                     "capabilities": {}, "clientInfo": {"name": "adashi-contract-check", "version": "2"}})
        self.send({"jsonrpc": "2.0", "method": "notifications/initialized"})

    def _read(self):
        for line in self.process.stdout:
            self.queue.put(line)

    def send(self, message):
        self.process.stdin.write(json.dumps(message, ensure_ascii=False) + "\n")
        self.process.stdin.flush()

    def request(self, method, params):
        self.sequence += 1
        self.send({"jsonrpc": "2.0", "id": self.sequence, "method": method, "params": params})
        while True:
            line = self.queue.get(timeout=30)
            response = json.loads(line)
            if response.get("id") == self.sequence:
                return response

    def call(self, name, **arguments):
        return self.request("tools/call", {"name": name, "arguments": {"projectName": "Fixture", **arguments}})

    def close(self):
        self.process.stdin.close()
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.terminate()
            self.process.wait(timeout=5)


def body(response):
    assert "error" not in response, response
    result = response["result"]
    assert not result.get("isError"), result
    return result.get("structuredContent") or json.loads(result["content"][0]["text"])


def size(value):
    return len(json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode("utf-8"))


def populate(db_path, fixture):
    with closing(sqlite3.connect(db_path)) as db, db:
        project_id = db.execute("SELECT id FROM projects LIMIT 1").fetchone()[0]
        workspace_id = db.execute("SELECT id FROM design_workspaces LIMIT 1").fetchone()[0]
        db.execute("DELETE FROM agent_tasks")
        db.execute("DELETE FROM rules")
        db.execute("DELETE FROM project_memory_notes")
        db.execute("DELETE FROM c4_elements")
        db.execute("UPDATE project_memory SET memory_body=?", (fixture["summary"],))
        db.execute("INSERT INTO rules(project_id,name,enabled,intend,hook,prompt) VALUES(?,'Required',1,'implementation','run.start',?)",
                   (project_id, fixture["rule"]))
        for i in range(fixture["tasks"]):
            db.execute("INSERT INTO agent_tasks(project_id,number,title,description,state,completion_memo,created_files,changed_files) VALUES(?,?,?,?,?,?,?,?)",
                       (project_id, i + 1, f"Task {i + 1}", fixture["description"],
                        "confirmed" if i == 0 else "finished", fixture["description"],
                        json.dumps([f"file-{j}.rs" for j in range(12)]), "[]"))
        for i in range(fixture["notes"]):
            db.execute("INSERT INTO project_memory_notes(project_id,note_id,operation_id,run_id,body) VALUES(?,?,?,?,?)",
                       (project_id, f"note-{i}", f"op-{i}", f"run-{i}", fixture["handover"]))
        for i in range(fixture["elements"]):
            db.execute("INSERT INTO c4_elements(workspace_id,external_id,parent_external_id,element_type,name,description) VALUES(?,?,?,?,?,?)",
                       (workspace_id, f"element-{i}", None if i == 0 else "element-0",
                        "Software System" if i == 0 else "Component", f"Component {i}", fixture["description"]))


def verify(client, measurements, fixture):
    tools = client.request("tools/list", {})["result"]["tools"]
    by_name = {tool["name"]: tool for tool in tools}
    list_schema = by_name["adashi_tasks"]["inputSchema"]
    assert '"open"' in json.dumps(list_schema) and '"confirmed"' in json.dumps(list_schema)
    assert body(client.call("adashi_tasks", operation="list", states=[]))["filteredTotal"] == 0
    for args in ({"states": ["invalid"]}, {"states": ["OPEN"]}, {"limit": 0},
                 {"limit": 101}, {"cursor": "invalid"}):
        response = client.call("adashi_tasks", operation="list", **args)
        assert "error" in response or response["result"].get("isError"), response
    ids, cursor = [], None
    while True:
        page = body(client.call("adashi_tasks", operation="list", limit=7, **({"cursor": cursor} if cursor else {})))
        assert page["filteredTotal"] == fixture["tasks"]
        assert len(page["tasks"]) <= 7
        for task in page["tasks"]:
            assert set(task) == {"id", "number", "title", "titleTruncated", "state", "version"}
            ids.append(task["id"])
        if not page["hasMore"]:
            assert page["nextCursor"] is None
            break
        cursor = page["nextCursor"]
    assert len(ids) == len(set(ids)) == fixture["tasks"]
    detail = body(client.call("adashi_tasks", operation="get", taskId=ids[0]))
    assert detail["task"]["description"] == fixture["description"]
    first = body(client.call("adashi_tasks", operation="list", limit=1))
    mismatch = client.call("adashi_tasks", operation="list", states=["finished"], cursor=first["nextCursor"])
    assert "error" in mismatch or mismatch["result"].get("isError")

    for intend in ("general", "design", "implementation"):
        for hook in ("run.start", "task.start", "task.end", "run.end"):
            result = body(client.call("adashi_rules", operation="get_rule_injections", intend=intend, hook=hook))
            assert result["contractVersion"] == 2
            assert "generatedContext" not in result and "memoryRule" not in result
            prompt = result["injectionPrompt"].encode("utf-8")
            for section in result["sections"]:
                assert prompt[section["startByte"]:section["endByte"]].decode("utf-8")
            assert all("prompt" not in rule for rule in result["rules"])
            assert fixture["handover"] not in result["injectionPrompt"]
            if hook != "run.start":
                assert result["status"] == "empty" and result["injectionPrompt"] == ""
            if intend == "implementation" and hook == "run.start":
                assert result["injectionPrompt"].count(fixture["rule"]) == 1
            again = body(client.call("adashi_rules", operation="get_rule_injections", intend=intend, hook=hook))
            assert result == again
    operational = body(client.call("adashi_rules", operation="get_rule_injections", intend="general", hook="run.start", memoryContext="protocolOnly"))
    assert [s["kind"] for s in operational["sections"]] == ["protocol"]
    memory = body(client.call("adashi_memory", operation="get", runId="run-3"))
    assert memory["matchedNotes"] == 1 and memory["memory"]["notes"][0]["noteId"] == "note-3"
    summary = "Current fixture constraint: preserve the public API."
    old_version = memory["memory"]["memoryVersion"]
    reviewed = body(client.call("adashi_memory", operation="update", operationId="review", expectedVersion=old_version,
                               memory=summary, supersededNoteIds=["note-3"]))
    assert all(n["noteId"] != "note-3" for n in reviewed["memory"]["notes"])
    historical = body(client.call("adashi_memory", operation="get", runId="run-3", includeSuperseded=True))
    assert historical["memory"]["notes"][0]["supersededByVersion"] == old_version + 1
    stale = client.call("adashi_tasks", operation="list", cursor=first["nextCursor"])
    assert "error" in stale or stale["result"].get("isError")
    start = body(client.call("adashi_rules", operation="get_rule_injections", intend="general", hook="run.start"))
    assert start["injectionPrompt"].count(summary) == 1
    large_summary = "A complete current constraint. " * 100
    body(client.call("adashi_memory", operation="update", operationId="large-summary",
                     expectedVersion=old_version + 1, memory=large_summary))
    bounded = body(client.call("adashi_rules", operation="get_rule_injections", intend="general", hook="run.start"))
    assert large_summary not in bounded["injectionPrompt"]
    assert "omitted in full" in bounded["injectionPrompt"]
    full = body(client.call("adashi_memory", operation="get"))
    assert full["memory"]["memory"] == large_summary
    custom_protocol = "Required custom protocol 🦀. " * 200
    body(client.call("adashi_memory", operation="update_rule", operationId="custom-protocol",
                     expectedVersion=full["memory"]["protocolVersion"], rule=custom_protocol))
    required = body(client.call("adashi_rules", operation="get_rule_injections", intend="general", hook="run.start", memoryContext="protocolOnly"))
    assert required["injectionPrompt"] == custom_protocol.strip()
    # Lifecycle validity is independent of optional rule presence and context budgets.
    response = client.call("adashi_rules", operation="get_rule_injections", intend="general", hook="run.invalid")
    assert "error" in response or response["result"].get("isError")
    measurements["checks"] = "passed"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True)
    parser.add_argument("--fixture", default=str(ROOT / "tests/fixtures/context-efficiency.json"))
    parser.add_argument("--output", required=True)
    parser.add_argument("--verify", action="store_true")
    parser.add_argument("--model-projector", help="Optional executable using a supported client's actual model-result projection")
    args = parser.parse_args()
    fixture = json.loads(Path(args.fixture).read_text(encoding="utf-8"))
    with scratch_directory() as temporary:
        root = Path(temporary)
        project = root / "project"
        project.mkdir()
        (root / "Adashi").mkdir()
        (root / "Adashi/settings.json").write_text(json.dumps({
            "window": {"width": 1440, "height": 940, "x": None, "y": None},
            "projects": [{"id": "fixture", "name": "Fixture", "folder": str(project)}],
            "lastActiveProjectId": "fixture", "ruleTemplates": [],
        }), encoding="utf-8")
        client = Client(args.binary, root)
        try:
            body(client.call("adashi_memory", operation="get"))
        finally:
            client.close()
        populate(project / ".adashi/adashi.sqlite3", fixture)
        client = Client(args.binary, root)
        try:
            cases = {
                "general_start": ("adashi_rules", {"operation": "get_rule_injections", "intend": "general", "hook": "run.start"}),
                "design_start": ("adashi_rules", {"operation": "get_rule_injections", "intend": "design", "hook": "run.start"}),
                "implementation_start": ("adashi_rules", {"operation": "get_rule_injections", "intend": "implementation", "hook": "run.start"}),
                "open_tasks": ("adashi_tasks", {"operation": "list", "states": ["open"]}),
                "all_tasks": ("adashi_tasks", {"operation": "list"}),
            }
            measurements = {}
            for label, (name, params) in cases.items():
                response = client.call(name, **params)
                payload = body(response)
                measurements[label] = {"rpcBytes": size(response), "structuredBytes": size(payload)}
                if args.model_projector:
                    projected = json.loads(subprocess.run([args.model_projector],
                        input=json.dumps(response["result"], ensure_ascii=False), text=True,
                        encoding="utf-8", capture_output=True, check=True, timeout=30).stdout)
                    assert projected["structuredContent"] == payload
                    assert projected["content"] == []
                    measurements[label]["modelBytes"] = size(projected)
            if args.verify:
                verify(client, measurements, fixture)
            Path(args.output).parent.mkdir(parents=True, exist_ok=True)
            Path(args.output).write_text(json.dumps(measurements, indent=2) + "\n", encoding="utf-8")
            print(json.dumps(measurements, indent=2))
        finally:
            client.close()


if __name__ == "__main__":
    main()
