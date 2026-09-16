"""Exercise repairable MCP errors and corrected writes over real stdio in an isolated project.

Optional --history reads Aworkit's event database in read-only mode and replays only the
failed Adashi arguments into this disposable fixture, never into the user's projects.
"""
import argparse
from copy import deepcopy
import json
from pathlib import Path
import sqlite3

from check_context_efficiency import Client, ROOT, body, scratch_directory
from check_design_hash_guard import verify_hash_guard
from check_operation_help import metrics, verify_help, verify_projectless_help


def failure(response, fragment=None):
    assert "error" not in response, response  # Must not become a JSON-RPC transport error.
    result = response["result"]
    assert result.get("isError") is True, result
    report = result["structuredContent"]
    assert json.loads(result["content"][0]["text"]) == report
    assert report["parameterSchema"]["type"] == "object"
    assert "projectName" in report["requiredParameters"]
    assert "uncertain" not in report["message"]
    if fragment:
        assert fragment in json.dumps(report), (fragment, report)
    return report


def replay_history(client, history, chat):
    revision = body(client.call("adashi_tasks", operation="list"))["revision"]
    requests = {}
    replayed = []
    connection = sqlite3.connect(Path(history).resolve().as_uri() + "?mode=ro", uri=True)
    try:
        rows = connection.execute(
            "SELECT sequence,kind,payload FROM semantic_events WHERE chat_id=? "
            "AND kind IN ('tool.requested','span.failed') ORDER BY sequence", (chat,))
        for sequence, kind, payload in rows:
            event = json.loads(payload)
            if kind == "tool.requested" and "/adashi_" in event.get("capabilityId", ""):
                requests[event["callId"]] = (sequence, event["input"])
            elif kind == "span.failed" and event.get("callId") in requests:
                request_sequence, request = requests[event["callId"]]
                arguments = deepcopy(request["arguments"])
                arguments["projectName"] = "Fixture"
                report = failure(client.call(request["name"], **arguments))
                replayed.append({"request": request_sequence, "failure": sequence,
                                 "tool": request["name"], "message": report["message"]})
    finally:
        connection.close()
    assert body(client.call("adashi_tasks", operation="list"))["revision"] == revision
    return replayed


def verify(client):
    revision = body(client.call("adashi_tasks", operation="list"))["revision"]
    change = {"op": "upsert_element", "externalId": "repair-container", "name": "Repair fixture",
              "elementType": "Container", "parentExternalId": "1"}
    args = {"operation": "save", "changeIntent": "Error contract fixture", "changes": [change]}
    report = failure(client.call("adashi_design", **args), "operationId")
    assert report["missingParameters"] == ["operationId"]
    missing_op = deepcopy(args)
    del missing_op["changes"][0]["op"]
    report = failure(client.call("adashi_design", **missing_op), "missing field `op`")
    assert report["parameterIssues"][0]["path"] == "changes[0]"
    invalid = deepcopy(args)
    invalid["changes"][0]["op"] = "invented_operation"
    failure(client.call("adashi_design", **invalid), "upsert_element")
    invalid = deepcopy(args)
    invalid["changes"][0]["externalId"] = 42
    failure(client.call("adashi_design", **invalid), "expected a string")
    failure(client.call("adashi_tasks", operation="update", taskId=1), "expectedVersion")
    failure(client.call("adashi_tasks", operation="finish", completedAt=None), "unknown field")
    failure(client.call("adashi_tasks", operation="invented"), "unknown variant")
    failure(client.call("adashi_qa", operation="run_jobs", operationId="missing-query"), "query")
    failure(client.call("adashi_qa", operation="run_jobs", query={"jobIds": [1]}), "operationId")
    assert body(client.call("adashi_tasks", operation="list"))["revision"] == revision
    task = body(client.call("adashi_tasks", operation="create", operationId="fixture-task", title="Fixture task"))["task"]
    task = body(client.call("adashi_tasks", operation="update", operationId="fixture-active", taskId=task["id"], expectedVersion=task["version"], state="active"))["task"]
    task = body(client.call("adashi_tasks", operation="finish", operationId="fixture-finish", taskId=task["id"], expectedVersion=task["version"], completionMemo="Fixture passed"))["task"]
    assert task["state"] == "finished"
    job = body(client.call("adashi_qa", operation="create_job", operationId="fixture-job", name="Fixture job", command="echo adashi-error-contract-ok"))["job"]
    run = body(client.call("adashi_qa", operation="run_jobs", operationId="fixture-run", query={"jobIds": [job["id"]]}))
    assert "adashi-error-contract-ok" in json.dumps(run), run
    assert '"status": "passed"' in json.dumps(run), run
    return {"errors": "detailed parameter and schema diagnostics passed",
            "tasks": "create, update and finish passed", "qa": "create and run passed"}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", default=str(ROOT / "src-tauri/target/release/adashi-mcp.exe"))
    parser.add_argument("--history")
    parser.add_argument("--chat", default="chat.23796a15a4174a9dd2bb23a4729044e6e0b7b15a")
    parser.add_argument("--output", default=str(ROOT / "target/mcp-error-contract.json"))
    options = parser.parse_args()
    verify_projectless_help(options.binary)
    with scratch_directory() as root:
        settings_dir = root / "Adashi"
        settings_dir.mkdir()
        (settings_dir / "settings.json").write_text(json.dumps({
            "window": {"width": 1000, "height": 700, "x": None, "y": None},
            "projects": [{"id": "fixture", "name": "Fixture", "folder": str(root / "project")}],
            "lastActiveProjectId": "fixture", "ruleTemplates": []}), encoding="utf-8")
        client = Client(options.binary, root)
        try:
            body(client.call("adashi_tasks", operation="list"))  # Initialize isolated project.
            replayed = replay_history(client, options.history, options.chat) if options.history else []
            result = {"replayedFailures": replayed, "metrics": metrics(client),
                      "operationHelp": verify_help(client), "verification": verify(client),
                      "hashGuard": verify_hash_guard(client, options.binary, root)}
        finally:
            client.close()
    output = Path(options.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(result, indent=2), encoding="utf-8")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
