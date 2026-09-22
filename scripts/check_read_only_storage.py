"""Verify real MCP reads do not change database bytes or mtime; keep a native UI fixture.

python scripts/check_read_only_storage.py --binary src-tauri/target/debug/adashi-mcp.exe
The isolated settings and project are kept under target/read-only-storage.
"""
import argparse
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import sys
import uuid

sys.dont_write_bytecode = True
from check_context_efficiency import Client, ROOT, body


def fingerprint(path):
    return {"sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
            "modifiedNs": path.stat().st_mtime_ns, "bytes": path.stat().st_size}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True)
    options = parser.parse_args()
    root = ROOT / "target/read-only-storage" / str(uuid.uuid4())
    folder = root / "project"
    settings = {"window": {"width": 1440, "height": 940, "x": None, "y": None},
                "projects": [{"id": "fixture", "name": "Fixture", "folder": str(folder)}],
                "lastActiveProjectId": "fixture", "ruleTemplates": []}
    # Client sets LOCALAPPDATA for Windows; XDG_CONFIG_HOME isolates Linux as well.
    os.environ["XDG_CONFIG_HOME"] = str(root)
    for name in ("Adashi", "adashi"):
        (root / name).mkdir(parents=True, exist_ok=True)
        (root / name / "settings.json").write_text(json.dumps(settings), encoding="utf-8")
    client = Client(options.binary, root)
    path = folder / ".adashi/adashi.sqlite3"
    try:
        body(client.call("adashi_tasks", operation="list"))
        with closing(sqlite3.connect(path)) as db, db:
            # Simulate a pre-upgrade database pulled from another computer. Its
            # single path cannot safely be attributed to this computer.
            db.execute("DROP TABLE project_computers")
            db.execute("PRAGMA user_version=0")
            db.execute("DELETE FROM schema_migrations WHERE version=13")
            db.execute("UPDATE projects SET repository_path='/another/computer/project'")
            db.execute("INSERT INTO agent_tasks(project_id,number,title,state) VALUES(1,1,'Read-only fixture task','todo')")
            db.execute("INSERT INTO rules(project_id,name,enabled,intend,hook,prompt) VALUES(1,'Fixture rule',1,'implementation','run.start','Keep project data stable when reading.')")
            db.execute("INSERT INTO resource_intents(project_id,agent_run_id,resource_kind,resource_id,expires_at) VALUES(1,'expired','task','1','2000-01-01')")
            db.execute("INSERT INTO qa_jobs(project_id,number,name,command) VALUES(1,1,'Fixture QA','echo fixture')")
            db.execute("INSERT INTO ui_mockups(project_id,external_id,title,attached_to_external_id,viewport_width,viewport_height,accepted_svg) VALUES(1,'preview','Preview','1',100,60,?)",
                       ('<svg xmlns="http://www.w3.org/2000/svg" width="100" height="60"><rect width="100" height="60" fill="#336699"/></svg>',))
        # Finish initialization/backfill before taking the stable baseline.
        body(client.call("adashi_memory", operation="get"))
        with closing(sqlite3.connect(path)) as db:
            assert db.execute("PRAGMA integrity_check").fetchone()[0] == "ok"
            assert not db.execute("PRAGMA foreign_key_check").fetchall()
            assert db.execute("SELECT repository_path FROM projects").fetchone()[0] == "/another/computer/project"
            assert db.execute("SELECT repository_path FROM project_computers").fetchone()[0] == str(folder)
        baseline = fingerprint(path)
        requests = [
            ("adashi_memory", {"operation": "get"}),
            ("adashi_tasks", {"operation": "list"}),
            ("adashi_tasks", {"operation": "get", "taskId": 1}),
            ("adashi_rules", {"operation": "list"}),
            ("adashi_qa", {"operation": "list_jobs"}),
            ("adashi_qa", {"operation": "list_runs"}),
            ("adashi_design", {"operation": "get_overview"}),
            ("adashi_design", {"operation": "get_scope", "elementId": "1"}),
            ("adashi_grep", {"pattern": "Fixture"}),
            ("adashi_intents", {"operation": "list"}),
        ]
        for hook in ("run.start", "task.start", "task.end", "run.end"):
            requests.append(("adashi_rules", {"operation": "get_rule_injections",
                                              "intend": "implementation", "hook": hook}))
        for cycle in range(3):
            for tool, args in requests:
                body(client.call(tool, **args))
                assert fingerprint(path) == baseline, (cycle, tool, args, fingerprint(path), baseline)
    finally:
        client.close()
    assert fingerprint(path) == baseline
    evidence = {"fixtureRoot": str(root), "database": str(path),
                "callsVerified": 3 * len(requests), "baseline": baseline}
    (root / "evidence.json").write_text(json.dumps(evidence, indent=2), encoding="utf-8")
    print(json.dumps(evidence, indent=2))


if __name__ == "__main__":
    main()
