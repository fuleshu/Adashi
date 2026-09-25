"""Real merges of independently created resources and edits, including fresh clones."""
import json
import uuid
from .common import Peer, raw_error, together, write_json
from .conflicts import assert_blocked, resolve_ours


def additions(suite):
    with suite.pair("additions") as (a, b):
        base_ids = {json.loads(p.read_bytes())["identity"] for p in (b.folder / ".adashi/text/records").rglob("*.json")}
        def add(peer, label):
            task = peer.mutate("adashi_tasks", operation="create", title="New " + label, designSpecificationLinks=[{"designExternalId": "app"}])["task"]
            job = peer.mutate("adashi_qa", operation="create_job", name="QA " + label, command="echo " + label, taskIds=[task["id"]], tags=[label])["job"]
            return task, job
        (ta, ja), (tb, jb) = together(lambda: add(a, "Alice"), lambda: add(b, "Bob"))
        assert ta["id"] == tb["id"] and ta["number"] == tb["number"]
        assert ja["id"] == jb["id"] and ja["number"] == jb["number"]
        additions = [json.loads(p.read_bytes()) for p in (b.folder / ".adashi/text/records").rglob("*.json")
                     if json.loads(p.read_bytes())["identity"] not in base_ids]
        maxima = {}
        for p in (a.folder / ".adashi/text/records").rglob("*.json"):
            record = json.loads(p.read_bytes())
            if isinstance(record["data"].get("id"), int):
                maxima[record["collection"]] = max(maxima.get(record["collection"], 0), record["data"]["id"])
        a.commit("Alice creates task and linked QA")
        b.commit("Bob creates task and linked QA")
        a.merge_from(b, "additions-b")
        assert_blocked(a, suite.seed_tasks[0], "duplicate")
        corrected = {}
        for record in additions:
            collection, data = record["collection"], record["data"]
            if isinstance(data.get("id"), int):
                maxima[collection] = maxima.get(collection, 0) + 1
                corrected[(collection, data["id"])] = maxima[collection]
                data["id"] = maxima[collection]
                if collection in ("agent_tasks", "qa_jobs"):
                    data["number"] += 1
        for record in additions:
            data = record["data"]
            if record["collection"] == "resource_versions":
                collection = {"task": "agent_tasks", "qa.job": "qa_jobs"}.get(data["resource_kind"])
                key = (collection, int(data["resource_id"])) if collection else None
                if key in corrected:
                    data["resource_id"] = str(corrected[key])
            write_json(a.folder / ".adashi/text/records" / record["collection"] / (record["identity"] + ".json"), record)
        tb["id"], tb["number"] = corrected[("agent_tasks", tb["id"])], tb["number"] + 1
        jb["id"], jb["number"] = corrected[("qa_jobs", jb["id"])], jb["number"] + 1
        a.commit("Explicitly reconcile numeric IDs in all added records, retaining UUID references")
        for task, job in ((ta, ja), (tb, jb)):
            current = a.call("adashi_tasks", operation="get", taskId=task["id"])["task"]
            assert current["number"] == task["number"] and current["designSpecificationLinks"][0]["designExternalId"] == "app"
            current_job = a.call("adashi_qa", operation="get_job", qaJobId=job["id"])["job"]
            assert current_job["number"] == job["number"] and [link["taskId"] for link in current_job["taskLinks"]] == [task["id"]]
        clean = suite.clone("reconstructed", a.folder)
        try:
            before = clean.tracked()
            assert clean.call("adashi_tasks", operation="get", taskId=ta["id"])["task"]["title"] == ta["title"]
            assert [link["taskId"] for link in clean.call("adashi_qa", operation="get_job", qaJobId=jb["id"])["job"]["taskLinks"]] == [tb["id"]]
            assert clean.tracked() == before
            assert not (clean.folder / ".adashi/adashi.sqlite3").exists()
            tracked = b"\n".join(before.values())
            assert not any("/mutation_operations/" in name for name in before)
            assert b'"fingerprint"' not in tracked and b'"result_json"' not in tracked
            assert str(suite.root).encode() not in tracked and str(suite.root).replace("\\", "\\\\").encode() not in tracked
        finally:
            clean.close()
        suite.passed("all sequential numeric collisions are explicit; reviewed correction preserves UUID task/QA links and clone reconstruction", taskIds=[ta["id"], tb["id"]], jobIds=[ja["id"], jb["id"]])


def edits(suite):
    with suite.pair("disjoint") as (a, b):
        together(lambda: a.update_task(0, title="Alice title"), lambda: b.update_task(1, description="Bob description"))
        a.commit("Alice task edit"); b.commit("Bob independent task edit")
        a.merge_from(b, "disjoint-b")
        assert a.task(0)["title"] == "Alice title" and a.task(1)["description"] == "Bob description"
        suite.passed("disjoint resource edits merge without conflicts")
    with suite.pair("fields") as (a, b):
        old = a.task()
        together(lambda: a.update_task(title="Alice field"), lambda: b.update_task(description="Bob field"))
        a.commit("Title field"); b.commit("Description field")
        b.publish("fields-b")
        suite.git(a.folder, "fetch", "origin", "fields-b")
        result = suite.git(a.folder, "merge", "--no-edit", "FETCH_HEAD", check=False)
        if result.returncode:
            # The shared updated_at field can make an otherwise independent API
            # edit adjacent to the title. Resolve explicitly, never in the driver.
            assert_blocked(a, old)
            resolve_ours(a)
            path, record = a.record("agent_tasks", id=old["id"])
            record["data"].update(title="Alice field", description="Bob field")
            write_json(path, record)
            a.commit("Review adjacent timestamp conflict and preserve both fields")
        merged = a.task()
        assert merged["title"] == "Alice field" and merged["description"] == "Bob field"
        raw_error(a.raw("adashi_tasks", operation="update", taskId=old["id"], expectedVersion=old["version"], operationId=str(uuid.uuid4()), title="Stale replacement"), "resource.conflict")
        suite.passed("same-resource API fields preserve both edits with explicit recovery when timestamps conflict", gitConflict=bool(result.returncode))
    with suite.pair("clean-fields") as (a, b):
        old = a.task()
        for peer, fields in ((a, {"title": "Alice clean field"}), (b, {"description": "Bob clean field"})):
            path, record = peer.record("agent_tasks", id=old["id"])
            record["data"].update(fields)
            write_json(path, record)
            peer.commit("Independent separated fields")
        a.merge_from(b, "clean-fields-b")
        assert a.task()["title"] == "Alice clean field" and a.task()["description"] == "Bob clean field"
        raw_error(a.raw("adashi_tasks", operation="update", taskId=old["id"], expectedVersion=old["version"], operationId=str(uuid.uuid4()), title="Stale replacement"), "resource.conflict")
        suite.passed("clean same-resource field merge changes the content guard without raw counter changes")
    with suite.pair("multiline") as (a, b):
        original_source = next(d["source"] for d in a.scope()["diagrams"] if d["key"] == "flow")
        # Edit separate line chunks in the actual canonical Mermaid artifact.
        for peer, original, replacement in ((a, "First", "Alice"), (b, "Last", "Bob")):
            path, record = peer.record("diagrams", key="flow")
            record["data"]["source"]["lines"] = [line.replace(original, replacement) for line in record["data"]["source"]["lines"]]
            write_json(path, record)
            peer.commit("Independent Mermaid line " + replacement)
        a.merge_from(b, "multiline-b")
        source = next(d["source"] for d in a.scope()["diagrams"] if d["key"] == "flow")
        assert source == original_source.replace("First", "Alice").replace("Last", "Bob")
        suite.passed("multiline artifacts retain both independent edits and exact newline content")


def local_processes_and_reads(suite):
    with suite.pair("reads") as (a, b):
        second = Peer(suite, a.folder, suite.root / "reads-second-settings")
        try:
            together(lambda: a.update_task(0, title="Local process A"), lambda: second.update_task(1, title="Local process B"))
            a.commit("Two processes share a checkout")
            before = a.tracked()
            mtimes = {p: (a.folder / p).stat().st_mtime_ns for p in before}
            for _ in range(5):
                together(lambda: a.task(), lambda: second.scope())
            assert a.tracked() == before
            assert {p: (a.folder / p).stat().st_mtime_ns for p in before} == mtimes
            task = a.task(); operation = str(uuid.uuid4())
            args = dict(operation="update", operationId=operation, taskId=task["id"], expectedVersion=task["version"], title=task["title"])
            first = a.call("adashi_tasks", **args)
            after_noop = a.canonical()
            changed = [p for p in set(before) | set(after_noop) if p.startswith(".adashi/text/") and before.get(p) != after_noop.get(p)]
            assert changed == [], changed
            assert a.tracked() == before
            assert {p: (a.folder / p).stat().st_mtime_ns for p in before} == mtimes
            # Both fresh no-ops and retries leave every tracked byte untouched.
            assert second.call("adashi_tasks", **args) == first
            assert a.canonical() == after_noop
            restarted = Peer(suite, a.folder, suite.root / "reads-restarted-settings")
            try:
                assert restarted.call("adashi_tasks", **args) == first
                assert restarted.canonical() == after_noop
            finally:
                restarted.close()
            (a.folder / ".adashi/local/test-credential.txt").write_text("PRIVATE_CANARY_DO_NOT_TRACK", encoding="utf-8")
            assert suite.git(a.folder, "status", "--porcelain").stdout == ""
            assert "PRIVATE_CANARY_DO_NOT_TRACK" not in b"\n".join(a.tracked().values()).decode()
            suite.passed("two local processes, byte/mtime-stable fresh no-ops and reads, restart replay and no tracked request history", freshNoopTrackedChanges=len(changed))
        finally:
            second.close()


def legacy_request_cleanup(suite):
    with suite.pair("legacy-requests") as (a, unused):
        local_path = a.folder / ".adashi/local/state.json"
        # Create a real result locally, then arrange the representation used by v1.
        a.update_task(title="Legacy request result")
        task = a.task()
        local = json.loads(local_path.read_bytes())
        operation, result = next(iter(local["receipts"].items()))
        _, project = a.record("projects")
        identity = str(uuid.uuid4())
        history_path = a.folder / f".adashi/text/records/mutation_operations/{identity}.json"
        write_json(history_path, {"schemaVersion": 1, "collection": "mutation_operations", "identity": identity, "deleted": False,
                                 "data": {"project_id": {"ref": project["identity"]}, "operation_id": operation,
                                          "result_json": json.dumps(result), "created_at": "2026-09-23 12:00:00"}})
        write_json(a.folder / ".adashi/text/format.json", {"schemaVersion": 1, "relationalSchema": 14})
        local["receipts"] = {}
        write_json(local_path, local)
        a.commit("Older project with tracked application request history")
        before = a.canonical()
        assert a.task()["title"] == task["title"] and a.canonical() == before
        a.update_task(title=task["title"])
        assert not history_path.exists()
        assert json.loads((a.folder / ".adashi/text/format.json").read_bytes())["schemaVersion"] == 2
        after = a.canonical()
        for name, data in before.items():
            if name != ".adashi/text/format.json" and "/mutation_operations/" not in name:
                assert after[name] == data
        status = suite.git(a.folder, "status", "--porcelain").stdout
        assert " D .adashi/text/records/mutation_operations/" in status, status
        a.commit("Remove application request history from project files")
        assert not any("/mutation_operations/" in p for p in a.tracked())
        a.update_task(title=task["title"])
        assert suite.git(a.folder, "status", "--porcelain").stdout == ""
        suite.passed("legacy request-history files are deleted by the format upgrade; later fresh no-ops keep Git clean")
