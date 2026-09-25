"""Real stdio Markdown contract acceptance on SQLite and text, in isolated projects."""
import argparse
import hashlib
import json
from pathlib import Path
import sys
import uuid
import jsonschema
import re
from concurrent.futures import ThreadPoolExecutor

sys.dont_write_bytecode = True
from check_context_efficiency import Client, ROOT, body


def fingerprint(folder, backend):
    base = folder / ".adashi"
    paths = sorted((base / "text").rglob("*.json")) if backend == "text" else [base / "adashi.sqlite3"]
    return {str(p.relative_to(base)): (hashlib.sha256(p.read_bytes()).hexdigest(), p.stat().st_mtime_ns) for p in paths}


def verify(binary, backend, root):
    folder = root / "project"
    (folder / ".adashi").mkdir(parents=True)
    if backend == "text":
        (folder / ".adashi/storage.json").write_text(json.dumps({"schemaVersion": 1, "backend": {"kind": "text"}}))
    settings = {"window": {"width": 1440, "height": 940, "x": None, "y": None}, "projects": [{"id": "fixture", "name": "Fixture", "folder": str(folder)}], "lastActiveProjectId": "fixture", "ruleTemplates": [], "architectureProjection": {"enabled": False, "fileName": "AGENTS.md"}}
    for name in ("Adashi", "adashi"):
        (root / name).mkdir(exist_ok=True)
        (root / name / "settings.json").write_text(json.dumps(settings))
    clients = [Client(binary, root), Client(binary, root)]  # Fresh processes, never installed/stale MCP.

    def raw(tool, /, peer=0, **args):
        return clients[peer].request("tools/call", {"name": tool, "arguments": {**({} if tool == "adashi_help" else {"projectName": "fixture"}), **args}})

    def call(tool="adashi_design", /, **args):
        return body(raw(tool, **args))

    def token(document):
        return {k: document[k] for k in ("documentId", "readToken")}

    def get(identity="prose", peer=0):
        return call(operation="get_documents", ids=["markdown:" + identity], peer=peer)["documents"][0]

    def save(changes, documents=(), **args):
        return call(operation="save", operationId=str(uuid.uuid4()), changeIntent="Markdown stdio acceptance", changes=changes, readTokens=[token(d) for d in documents], **args)

    def rejected(response, code=None):
        assert response.get("error") or response.get("result", {}).get("isError"), response
        if code:
            assert code in json.dumps(response), response

    try:
        tools = clients[0].request("tools/list", {})["result"]["tools"]
        design_schema = next(t["inputSchema"] for t in tools if t["name"] == "adashi_design")
        help_save = call("adashi_help", tool="adashi_design", operation="save", changeTypes=["upsert_markdown", "delete_markdown"])
        jsonschema.validate(help_save["exampleArguments"], help_save["parameterSchema"])
        jsonschema.validate(help_save["exampleArguments"], design_schema)
        assert "Markdown" in json.dumps(help_save)
        text = "# Specification\r\n\r\n日本語 Grüße 📝\r\n```rust\r\nlet exact = true;  \r\n```\r\n" + "Long prose\n" * 1500 + "DeepNeedle\n"
        create = {"projectName": "fixture", "operation": "save", "operationId": str(uuid.uuid4()), "changeIntent": "Create Markdown", "readTokens": [], "changes": [{"op": "upsert_markdown", "externalId": "prose", "title": "Prose", "body": text, "designLinks": []}, {"op": "upsert_markdown", "externalId": "other", "title": "Other", "body": "Unchanged", "designLinks": []}]}
        created = call(**create)
        assert created["stored"]
        assert call(**create) == created
        original = get()
        assert original["document"]["body"] == text
        overview = call(operation="get_overview")
        assert overview["markdown"]["totalCount"] == 2 and "body" not in overview["markdown"]["documents"][0]
        page = call(operation="list_markdown", markdownQuery={"limit": 1})
        assert page["markdown"]["nextAfterId"]
        assert len(call(operation="list_markdown", markdownQuery={"limit": 1, "afterId": page["markdown"]["nextAfterId"]})["markdown"]["documents"]) == 1
        assert call(operation="search", query="DeepNeedle", kinds=["markdown"])["hits"][0]["id"] == "prose"
        scope = call(operation="get_scope", elementId="prose")
        assert scope["documents"][0]["document"]["body"] == text
        help_scope = call("adashi_help", tool="adashi_design", operation="get_scope")
        jsonschema.validate(scope, help_scope["responseSchema"])
        save([{"op": "upsert_binding", "designExternalId": "prose", "targetType": "file", "target": "src/example.rs"}])
        bindings = call(operation="get_bindings", files=["src/example.rs"])
        assert any(d["documentId"] == "markdown:prose" for d in bindings["documents"])
        assert "design:prose" in json.dumps(call("adashi_grep", pattern="DeepNeedle file:src/example.rs"))
        links = [{"targetType": "markdown", "designExternalId": "prose"}]
        task = call("adashi_tasks", operation="create", operationId=str(uuid.uuid4()), title="Implement prose", designSpecificationLinks=links)["task"]
        job = call("adashi_qa", operation="create_job", operationId=str(uuid.uuid4()), name="Verify prose", command="echo ok", designSpecificationLinks=links)["job"]
        expanded = call("adashi_tasks", operation="get", taskId=task["id"], includeDesignScopes=True)
        assert expanded["designSpecifications"][0]["documents"][0]["document"]["body"] == text
        assert "documents" not in call("adashi_tasks", operation="get", taskId=task["id"])["designSpecifications"][0]
        before = fingerprint(folder, backend)
        get(); call(operation="get_overview"); call(operation="search", query="DeepNeedle"); call("adashi_grep", pattern="DeepNeedle")
        assert before == fingerprint(folder, backend), "reads changed canonical bytes/mtime"
        save([{"op": "upsert_markdown", **original["document"], "title": "Renamed", "body": text + "Peer edit\n"}], [original], peer=1)
        other = get("other")
        stale = raw("adashi_design", operation="save", operationId=str(uuid.uuid4()), changeIntent="Stale atomic batch", readTokens=[token(original), token(other)], changes=[{"op": "upsert_markdown", **other["document"], "body": "Must roll back"}, {"op": "upsert_markdown", **original["document"], "body": "Stale overwrite"}])
        rejected(stale, "out_of_date"); assert get("other")["document"]["body"] == "Unchanged"
        current = get()
        save([{"op": "upsert_markdown", **current["document"], "body": current["document"]["body"] + "Merged intent\n"}], [current])
        assert call("adashi_tasks", operation="get", taskId=task["id"])["task"]["designSpecificationLinks"][0]["title"] == "Renamed"
        assert call("adashi_qa", operation="list_jobs", query={"designExternalIds": ["prose"]})["jobs"]
        current = get()
        unchanged = call(operation="get_scope", elementId="prose")
        result = raw("adashi_design", operation="save", operationId=str(uuid.uuid4()), changeIntent="No-op parity", changes=[{"op": "upsert_markdown", **current["document"]}], readTokens=[token(current)])
        rejected(result, "save.no_changes")  # Preserve the existing MCP design no-op contract.
        assert call(operation="get_scope", elementId="prose") == unchanged
        rejected(raw("adashi_design", operation="save", operationId=str(uuid.uuid4()), changeIntent="Protected deletion", readTokens=[token(current)], changes=[{"op": "delete_markdown", "externalId": "prose"}]))
        for extra in ({"expectedRevision": 1}, {"guard": {}}):
            rejected(raw("adashi_design", **{**create, "operationId": str(uuid.uuid4()), **extra}))
        rejected(raw("adashi_design", operation="save", operationId=str(uuid.uuid4()), changeIntent="Closed fields", changes=[{"op": "upsert_markdown", **current["document"], "path": "docs/raw.md"}]))
        call("adashi_tasks", operation="update", operationId=str(uuid.uuid4()), taskId=task["id"], expectedVersion=task["version"], designSpecificationLinks=[])
        call("adashi_qa", operation="update_job", operationId=str(uuid.uuid4()), qaJobId=job["id"], expectedVersion=job["version"], designSpecificationLinks=[])
        binding = next(d for d in bindings["documents"] if d["documentId"].startswith("binding:"))
        save([{"op": "delete_binding", "designExternalId": "prose", "targetType": "file", "target": "src/example.rs"}, {"op": "delete_markdown", "externalId": "prose"}], [binding, current])
        assert get()["document"] is None
        # Publication failure is explicitly separate from an already successful save.
        settings["architectureProjection"]["enabledProjectIds"] = ["fixture"]
        for name in ("Adashi", "adashi"):
            (root / name / "settings.json").write_text(json.dumps(settings))
        output = folder / "docs/adashi" / ("design-" + hashlib.sha256(b"other").hexdigest() + ".md")
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text("Unowned collision", encoding="utf-8")
        other = get("other")
        saved = save([{"op": "upsert_markdown", **other["document"], "body": "Saved despite publication failure"}], [other])
        assert saved["stored"] and saved["projection"]["state"] == "error"
        assert get("other")["document"]["body"] == "Saved despite publication failure"
        assert output.read_text() == "Unowned collision"
        output.unlink()  # Only this script's isolated collision fixture.
        other = get("other")
        saved = save([{"op": "upsert_markdown", **other["document"], "body": "Published successfully"}], [other])
        assert saved["projection"]["state"] == "current" and output.read_text(encoding="utf-8").endswith("Published successfully")
        # Execute the exact published template examples after replacing only fixture identity,
        # operation id, and the opaque token explicitly marked as a retrieval placeholder.
        template = (ROOT / "agents_template.md").read_text(encoding="utf-8")
        examples = {name: json.loads(payload) for name, payload in re.findall(r'<!-- example:markdown-(\w+) -->\s*```json\s*(.*?)\s*```', template, re.S)}
        published_help = call("adashi_help", **examples["help"])
        create_example = {**examples["create"], "projectName": "fixture", "operationId": str(uuid.uuid4())}
        jsonschema.validate(create_example, published_help["parameterSchema"])
        assert call(**create_example)["stored"]
        read_example = {**examples["retrieve"], "projectName": "fixture"}
        retrieved = call(**read_example)["documents"][0]
        update_example = {**examples["update"], "projectName": "fixture", "operationId": str(uuid.uuid4()), "readTokens": [token(retrieved)]}
        jsonschema.validate(update_example, published_help["parameterSchema"])
        assert call(**update_example)["stored"]
        assert call(**read_example)["documents"][0]["document"]["body"] == update_example["changes"][0]["body"]
        workflow_help = call("adashi_help", tool="adashi_rules", operation="get_rule_injections")
        assert workflow_help["agentWorkflow"].replace("\r\n", "\n") == template
        assert (folder / "docs/adashi/agent-workflow.md").read_text(encoding="utf-8").endswith(template)
        assert "docs/adashi/agent-workflow.md" in (folder / "AGENTS.md").read_text(encoding="utf-8")
        startup = call("adashi_rules", operation="get_rule_injections", intend="implementation", hook="run.start")
        index = next(section for section in startup["sections"] if section["id"] == "design.index")
        index_body = startup["injectionPrompt"].encode()[index["startByte"]:index["endByte"]].decode()
        assert len(index_body.encode()) <= 3000 and "Markdown" in index_body and "markdown:<id>" in index_body
        assert "Show feedback beside the field" not in index_body
        save([{"op": "upsert_binding", "designExternalId": "checkout-design", "targetType": "file", "target": "src/spec.rs"}])
        short = (folder / "src/AGENTS.md").read_text(encoding="utf-8")
        relative = re.search(r'\]\(([^)]+design-[a-f0-9]+\.md)\)', short).group(1)
        full_file = (folder / "src" / relative).resolve()
        exported = full_file.read_text(encoding="utf-8")
        discovered_id = json.loads(re.search(r'^> Design ID: (.+)$', exported, re.M).group(1))
        fresh = get(discovered_id)
        assert exported.endswith(fresh["document"]["body"])
        save([{"op": "upsert_markdown", **fresh["document"], "body": fresh["document"]["body"] + "Discovered through folder short.\n"}], [fresh])
        assert full_file.read_text(encoding="utf-8").endswith("Discovered through folder short.\n")
        # Independent MCP processes contend on the same canonical token, not a mocked adapter.
        save([{"op":"upsert_markdown","externalId":"race","title":"Concurrent design","body":"Base","designLinks":[]}])
        race = get("race")
        def race_write(peer):
            return raw("adashi_design", peer=peer, operation="save", operationId=str(uuid.uuid4()), changeIntent="Concurrent guarded writer", readTokens=[token(race)], changes=[{"op":"upsert_markdown",**race["document"],"body":f"Writer {peer}"}])
        with ThreadPoolExecutor(max_workers=2) as pool:
            results = list(pool.map(race_write, [0,1]))
        winners = [r for r in results if not r.get("error") and not r.get("result",{}).get("isError")]
        assert len(winners) == 1, results
        loser = next(r for r in results if r not in winners)
        rejected(loser,"out_of_date")
        assert get("race")["document"]["body"] in ("Writer 0","Writer 1")
        rejected(raw("adashi_design",operation="save",operationId=str(uuid.uuid4()),changeIntent="Stale deletion",readTokens=[token(race)],changes=[{"op":"delete_markdown","externalId":"race"}]),"out_of_date")
        latest = get("race")
        deleted = {"operation":"save","operationId":str(uuid.uuid4()),"changeIntent":"Guarded deletion","readTokens":[token(latest)],"changes":[{"op":"delete_markdown","externalId":"race"}]}
        saved_delete = call(**deleted)
        assert call(peer=1,**deleted) == saved_delete and get("race")["document"] is None
        rejected(raw("adashi_design",operation="save",operationId=str(uuid.uuid4()),changeIntent="Stale resurrection",readTokens=[token(latest)],changes=[{"op":"upsert_markdown",**latest["document"]}]),"out_of_date")
        print(f"PASS {backend}: schemas, full reads, discovery, links, conflicts, replay, no-ops, deletion and write-free reads")
    finally:
        for client in clients:
            client.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", default=str(ROOT / "src-tauri/target/debug/adashi-mcp.exe"))
    args = parser.parse_args()
    root = ROOT / "target/markdown-mcp" / str(uuid.uuid4())
    for backend in ("sqlite", "text"):
        verify(args.binary, backend, root / backend)
    print("Evidence:", root)
