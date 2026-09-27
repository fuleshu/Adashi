"""Actual MCP lookup, ordering, filtering and stale-number regression."""
from .common import body, raw_error, write_json


def task_numbers(suite):
    with suite.pair("task-numbers") as (a, _):
        original = a.task()
        observed = a.call("adashi_tasks", operation="list")
        new = a.mutate("adashi_tasks", operation="create", title="Backdated imported task")["task"]
        path, record = a.record("agent_tasks", id=new["id"])
        record["data"]["created_at"] = "2000-01-01 00:00:00"
        write_json(path, record)
        before = a.canonical()
        raw_error(a.raw("adashi_tasks", operation="get", taskNumber=1, expectedRevision=observed["revision"]), "tasks.stale_number")
        for args in ({}, {"taskNumber": 0}, {"taskNumber": -1}, {"taskNumber": 999},
                     {"taskNumber": 1, "taskId": original["id"]}):
            raw_error(a.raw("adashi_tasks", operation="get", **args))
        raw_error(a.raw("adashi_tasks", operation="update", taskNumber=1, title="Must not write"), "get only")
        assert a.canonical() == before
        current = a.call("adashi_tasks", operation="get", taskNumber=1)["task"]
        assert current["id"] == new["id"] and current["number"] == 1
        unchanged = a.task()
        assert (unchanged["id"], unchanged["number"], unchanged["version"]) == (original["id"], 2, original["version"])
        active = a.mutate("adashi_tasks", operation="update", taskId=current["id"], expectedVersion=current["version"], state="active")["task"]
        finished = a.mutate("adashi_tasks", operation="finish", taskId=active["id"], expectedVersion=active["version"], completionMemo="Numbering fixture")["task"]
        closed = a.mutate("adashi_tasks", operation="close", taskId=finished["id"], expectedVersion=finished["version"])["task"]
        listing = a.call("adashi_tasks", operation="list")
        assert [t["number"] for t in listing["tasks"]] == [2, 3]
        assert a.call("adashi_tasks", operation="get", taskNumber=1)["task"]["id"] == closed["id"]
        pages, cursor = [], None
        while True:
            page = a.call("adashi_tasks", operation="list", states=["todo", "active", "finished", "closed"], limit=1, cursor=cursor)
            pages.extend(page["tasks"])
            cursor = page["nextCursor"]
            if not cursor:
                break
        assert [t["number"] for t in pages] == [1, 2, 3]
        assert pages[0]["id"] == closed["id"]
        a.mutate("adashi_tasks", operation="delete", taskId=closed["id"], expectedVersion=closed["version"])
        assert a.call("adashi_tasks", operation="get", taskNumber=1)["task"]["id"] == original["id"]
        assert a.task()["version"] == original["version"]
        help_result = body(a.client.request("tools/call", {"name": "adashi_help", "arguments": {"tool": "adashi_tasks", "operation": "get"}}))
        assert "taskNumber" in help_result["parameterSchema"]["properties"]
        assert len(help_result["parameterSchema"]["oneOf"]) == 2
        suite.passed("creation-order task numbers survive filtering/pagination, reject stale lookups and never address mutations")
