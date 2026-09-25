"""Visible textual conflicts and syntactically clean semantic conflicts.

Resolution is explicit fixture-author action through Git; the application must
never repair, discard or overwrite these conflicting records on its own.
"""
import json
import uuid
from .common import raw_error, write_json, correct_task_numbers


def assert_blocked(peer, old, reason=None):
    before = peer.canonical()
    error = raw_error(peer.raw("adashi_tasks", operation="get", taskId=old["id"]), reason)
    raw_error(peer.raw("adashi_tasks", operation="update", taskId=old["id"], expectedVersion=old["version"], operationId=str(uuid.uuid4()), title="Must remain blocked"))
    assert peer.canonical() == before
    return error


def resolve_ours(peer):
    names = peer.suite.git(peer.folder, "diff", "--name-only", "--diff-filter=U").stdout.splitlines()
    assert names
    peer.suite.git(peer.folder, "checkout", "--ours", "--", *names)
    return names


def textual(suite):
    with suite.pair("overlap") as (a, b):
        old = a.task()
        a.update_task(title="Alice overlap")
        b.update_task(title="Bob overlap")
        b.update_task(1, description="Bob independent work survives conflict")
        a.commit("Alice overlapping edit"); b.commit("Bob overlapping and independent edits")
        a.merge_from(b, "overlap-b", conflict=True)
        error = assert_blocked(a, old)
        assert any(b"<<<<<<<" in data for data in a.canonical().values())
        paths = resolve_ours(a)
        path, record = a.record("agent_tasks", id=old["id"])
        record["data"]["title"] = "Reviewed Alice overlap and Bob overlap"
        write_json(path, record)
        a.commit("Human-reviewed combined title; retain both users' other work")
        assert a.task()["title"] == record["data"]["title"]
        assert a.task(1)["description"] == "Bob independent work survives conflict"
        suite.passed("overlapping edits expose markers, block both read/write and recover after explicit resolution", conflictPaths=paths, error=error)
    with suite.pair("delete-edit") as (a, b):
        old = a.task(1)
        a.mutate("adashi_tasks", operation="delete", taskId=old["id"], expectedVersion=old["version"])
        b.update_task(1, title="Bob edits the deleted task")
        a.commit("Alice deletes task"); b.commit("Bob edits task")
        a.merge_from(b, "delete-edit-b", conflict=True)
        assert_blocked(a, old)
        names = suite.git(a.folder, "diff", "--name-only", "--diff-filter=U").stdout.splitlines()
        suite.git(a.folder, "checkout", "--theirs", "--", *names)
        # Keep Bob's edited task and its original ordered design links explicitly.
        suite.git(a.folder, "restore", "--source=FETCH_HEAD", "--", ".adashi/text/records/task_design_specification_links")
        a.commit("Review deletion conflict: retain Bob's task and links")
        assert a.task(1)["title"] == "Bob edits the deleted task"
        assert len(a.task(1)["designSpecificationLinks"]) == 3
        suite.passed("delete/edit conflict preserves the edit and restores explicitly chosen references")


def semantic(suite):
    with suite.pair("delete-reference") as (a, b):
        old = a.task(1)
        a.mutate("adashi_tasks", operation="delete", taskId=old["id"], expectedVersion=old["version"])
        job = b.mutate("adashi_qa", operation="create_job", name="New reference", command="echo ref", taskIds=[old["id"]])["job"]
        a.commit("Delete target"); b.commit("Reference target independently")
        a.merge_from(b, "delete-reference-b")
        error = assert_blocked(a, old, "missing/deleted")
        path = a.folder / ".adashi/text/records/agent_tasks"
        restored = next(p for p in path.glob("*.json") if json.loads(p.read_bytes())["data"].get("id") == old["id"])
        suite.git(a.folder, "restore", "--source=FETCH_HEAD", "--", restored.relative_to(a.folder).as_posix(), ".adashi/text/records/task_design_specification_links")
        a.commit("Review reference conflict: restore referenced task")
        assert a.call("adashi_qa", operation="get_job", qaJobId=job["id"])["job"]["taskLinks"][0]["taskId"] == old["id"]
        suite.passed("clean Git delete/reference merge is rejected semantically and recoverable", error=error)
    with suite.pair("numbers") as (a, b):
        tasks = []
        for peer, label in ((a, "Alice"), (b, "Bob")):
            task = peer.mutate("adashi_tasks", operation="create", title=label + " collision")["task"]
            if label == "Bob":
                _, task_record = peer.record("agent_tasks", id=task["id"])
                _, version_record = peer.record("resource_versions", resource_kind="task", resource_id=str(task["id"]))
                task["id"] += 1
                correct_task_numbers(peer, task_record["identity"], version_record["identity"], task["id"], task["number"])
            tasks.append(task)
            path, record = peer.record("agent_tasks", id=task["id"])
            record["data"]["number"] = 987654321
            write_json(path, record)
            assert peer.call("adashi_tasks", operation="get", taskId=task["id"])["task"]["number"] == 987654321
            peer.commit("Independent display label " + label)
        a.merge_from(b, "numbers-b")
        error = assert_blocked(a, a.suite.seed_tasks[0], "UNIQUE")
        path, record = a.record("agent_tasks", id=tasks[1]["id"])
        record["data"]["number"] = 987654322
        write_json(path, record)
        a.commit("Explicitly disambiguate labels; preserve immutable API IDs")
        assert a.call("adashi_tasks", operation="get", taskId=tasks[1]["id"])["task"]["number"] == 987654322
        suite.passed("syntactically clean duplicate display-number merge fails without automatic reassignment", error=error)


def ordering(suite):
    with suite.pair("ordered") as (a, b):
        links = [{"designExternalId": key} for key in ("third", "app", "peer")]
        a.update_task(designSpecificationLinks=links)
        b.update_task(1, title="Bob independent ordered-list neighbor")
        a.commit("Explicitly reorder links"); b.commit("Independent task edit")
        a.merge_from(b, "ordered-b")
        assert [link["designExternalId"] for link in a.task()["designSpecificationLinks"]] == ["third", "app", "peer"]
        suite.passed("explicit ordered lists survive Git merges with independent work")
    with suite.pair("order-collision") as (a, b):
        old = a.task()
        for peer, target in ((a, "app"), (b, "third")):
            _, task_record = peer.record("agent_tasks", id=old["id"])
            path, link = peer.record("task_design_specification_links", task_id={"ref": task_record["identity"]}, design_external_id=target)
            link["data"]["sort_order"] = 3
            write_json(path, link)
            peer.task()  # each branch is independently well-defined
            peer.commit("Move " + target + " to ordering slot 3")
        a.merge_from(b, "order-collision-b")
        error = assert_blocked(a, old, "ordering")
        path, link = a.record("task_design_specification_links", task_id={"ref": task_record["identity"]}, design_external_id="third")
        link["data"]["sort_order"] = 4
        write_json(path, link)
        a.commit("Review ordered-list collision")
        assert [link["designExternalId"] for link in a.task()["designSpecificationLinks"]] == ["peer", "app", "third"]
        suite.passed("clean merge with duplicate ordering positions fails until explicitly reordered", error=error)
