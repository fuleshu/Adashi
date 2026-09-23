"""Exercise the shared storage resolver through real, isolated stdio MCP clients.

python scripts/check_storage_core.py --binary src-tauri/target/debug/adashi-mcp.exe
Keeps the fixture/evidence under target/storage-core; never opens user projects.
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


def expect_error(response, code):
    assert "error" in response or response.get("result", {}).get("isError"), response
    assert code in json.dumps(response), response


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True)
    options = parser.parse_args()
    root = ROOT / "target/storage-core" / str(uuid.uuid4())
    projects = [{"id": name, "name": name, "folder": str(root / name)}
                for name in ("legacy", "explicit", "text", "server", "invalid")]
    settings = {"window": {"width": 1440, "height": 940, "x": None, "y": None},
                "projects": projects, "lastActiveProjectId": "legacy", "ruleTemplates": []}
    for name in ("Adashi", "adashi"):
        (root / name).mkdir(parents=True, exist_ok=True)
        (root / name / "settings.json").write_text(json.dumps(settings), encoding="utf-8")
    descriptors = {
        "explicit": {"schemaVersion": 1, "backend": {"kind": "sqlite"}},
        "text": {"schemaVersion": 1, "backend": {"kind": "text"}},
        "server": {"schemaVersion": 1, "backend": {"kind": "serverSql", "connectionProfile": "team", "namespace": "test"}},
        "invalid": {"schemaVersion": 1, "backend": {"kind": "sqlite", "ignored": True}},
    }
    for project, descriptor in descriptors.items():
        directory = root / project / ".adashi"
        directory.mkdir(parents=True)
        (directory / "storage.json").write_text(json.dumps(descriptor), encoding="utf-8")
    os.environ["XDG_CONFIG_HOME"] = str(root)
    settings_paths = [root / name / "settings.json" for name in ("Adashi", "adashi")]
    settings_before = {str(path): fingerprint(path) for path in settings_paths}
    clients = []
    concurrent_reads = 0
    try:
        for _ in range(2):
            clients.append(Client(options.binary, root))
        for project in ("legacy", "explicit"):
            created = body(clients[0].call("adashi_rules", projectName=project, operation="create",
                           operationId=f"{project}-rule", name=f"{project} rule", enabled=True,
                           intend="implementation", hook="task.start", prompt="Fixture rule"))
            rules = body(clients[1].call("adashi_rules", projectName=project, operation="list"))["rules"]
            assert len(rules) == 1 and rules[0]["id"] == created["ruleId"], rules
        assert not (root / "legacy/.adashi/storage.json").exists()
        databases = {name: root / name / ".adashi/adashi.sqlite3" for name in ("legacy", "explicit")}
        before = {name: fingerprint(path) for name, path in databases.items()}

        def read_repeatedly(client):
            for _ in range(10):
                for project in databases:
                    result = body(client.call("adashi_rules", projectName=project, operation="list"))
                    assert result["rules"][0]["name"] == f"{project} rule"
                    body(client.call("adashi_tasks", projectName=project, operation="list"))
            return 40

        with ThreadPoolExecutor(max_workers=2) as executor:
            concurrent_reads = sum(executor.map(read_repeatedly, clients))
        assert {name: fingerprint(path) for name, path in databases.items()} == before

        rejected = []
        for project, code in (("text", "storage.backend_unavailable"),
                              ("server", "storage.backend_unavailable"),
                              ("invalid", "storage.invalid_configuration")):
            descriptor_path = root / project / ".adashi/storage.json"
            original = fingerprint(descriptor_path)
            for client in clients:
                expect_error(client.call("adashi_tasks", projectName=project, operation="list"), code)
                expect_error(client.call("adashi_rules", projectName=project, operation="list"), code)
            assert fingerprint(descriptor_path) == original
            assert sorted(path.name for path in descriptor_path.parent.iterdir()) == ["storage.json"]
            rejected.append(project)

        # A present invalid descriptor must not fall back to an existing database.
        legacy_descriptor = root / "legacy/.adashi/storage.json"
        legacy_descriptor.write_text("{", encoding="utf-8")
        expect_error(clients[0].call("adashi_rules", projectName="legacy", operation="list"),
                     "storage.invalid_configuration")
        assert fingerprint(databases["legacy"]) == before["legacy"]
        legacy_descriptor.write_text(json.dumps(descriptors["explicit"]), encoding="utf-8")
        assert len(body(clients[1].call("adashi_rules", projectName="legacy", operation="list"))["rules"]) == 1
        assert fingerprint(databases["legacy"]) == before["legacy"]
        assert {str(path): fingerprint(path) for path in settings_paths} == settings_before
    finally:
        for client in clients:
            client.close()
    evidence = {"fixtureRoot": str(root), "concurrentReadCalls": concurrent_reads,
                "rejectedBackendsOrDescriptors": rejected,
                "databaseReadsUnchanged": True, "settingsReadsUnchanged": True,
                "legacyAndExplicitSqliteShareExistingData": True}
    (root / "evidence.json").write_text(json.dumps(evidence, indent=2), encoding="utf-8")
    print(json.dumps(evidence, indent=2))


if __name__ == "__main__":
    main()
