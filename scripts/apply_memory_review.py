"""Apply an explicitly reviewed project-memory plan through the versioned MCP API.
Creates a SQLite backup before opening the project with the rebuilt server.
No settings files in the user's profile are changed.
"""
import argparse
from contextlib import closing
import json
from pathlib import Path
import sqlite3
from check_context_efficiency import Client, body, scratch_directory


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True)
    parser.add_argument("--plan", required=True)
    parser.add_argument("--backup", required=True)
    args = parser.parse_args()
    plan = json.loads(Path(args.plan).read_text(encoding="utf-8"))
    source = Path(plan["projectFolder"]) / ".adashi/adashi.sqlite3"
    backup = Path(args.backup).resolve()
    if backup.exists():
        raise RuntimeError("Backup already exists; use a fresh path.")
    backup.parent.mkdir(parents=True, exist_ok=True)
    with closing(sqlite3.connect(source.resolve().as_uri() + "?mode=ro", uri=True)) as src:
        with closing(sqlite3.connect(backup)) as dst:
            src.backup(dst)
    with scratch_directory() as root:
        (root / "Adashi").mkdir()
        (root / "Adashi/settings.json").write_text(json.dumps({
            "window": {"width": 1440, "height": 940, "x": None, "y": None},
            "projects": [{"id": plan["projectId"], "name": plan["projectName"], "folder": plan["projectFolder"]}],
            "lastActiveProjectId": plan["projectId"], "ruleTemplates": [],
        }), encoding="utf-8")
        client = Client(args.binary, root)
        try:
            def call(name, **arguments):
                return body(client.request("tools/call", {"name": name,
                    "arguments": {"projectId": plan["projectId"], **arguments}}))
            before = call("adashi_get_memory", includeSuperseded=True)
            assert before["memory"]["memoryVersion"] == plan["expectedVersion"], "Summary changed; review again."
            retained = {n["noteId"]: n for n in before["memory"]["notes"]}
            assert set(plan["supersededNoteIds"]) <= retained.keys()
            limits = before["memory"]["limits"]
            assert len(plan["memory"]) <= limits["summaryChars"], "Summary exceeds its limit."
            assert len(retained) <= limits["notes"], "Retained note count exceeds its limit."
            assert len(plan["memory"]) + sum(len(n["body"]) for n in retained.values()) <= limits["totalChars"], "Cleanup would evict retained notes; shorten the summary first."
            updated = call("adashi_update_memory", **{k: plan[k] for k in
                           ("operationId", "expectedVersion", "memory", "supersededNoteIds")})
            after = call("adashi_get_memory", includeSuperseded=True)
            assert updated["memory"]["memory"] == plan["memory"]
            assert not (set(n["noteId"] for n in updated["memory"]["notes"]) & set(plan["supersededNoteIds"]))
            assert retained.keys() <= {n["noteId"] for n in after["memory"]["notes"]}, "A retained note is missing after cleanup."
            for note in after["memory"]["notes"]:
                original = retained.get(note["noteId"])
                if original and note["noteId"] in plan["supersededNoteIds"]:
                    for field in ("body", "createdAt", "operationId", "runId", "taskId"):
                        assert original.get(field) == note.get(field)
                    assert note["supersededByVersion"] == updated["memory"]["memoryVersion"]
            print(json.dumps({"projectId": plan["projectId"],
                "memoryVersion": updated["memory"]["memoryVersion"],
                "resolvedNotes": len(plan["supersededNoteIds"]), "backup": str(backup)}))
        finally:
            client.close()


if __name__ == "__main__":
    main()
