"""Real stdio, two-client tests of the document hash contract in a disposable project.

Invoked by check_mcp_error_contract.py; never mutates a configured user project.
"""
from concurrent.futures import ThreadPoolExecutor
from contextlib import closing
from copy import deepcopy
import json
import sqlite3

from check_context_efficiency import Client, body


def verify_hash_guard(client, binary, root):
    sequence = 0

    def operation():
        nonlocal sequence
        sequence += 1
        return f"hash-contract-{sequence}"

    def read(*ids, peer=client):
        return body(peer.call("adashi_design", operation="get_documents", ids=list(ids)))["documents"]

    def tokens(documents):
        return [{"documentId": doc["documentId"], "readToken": doc["readToken"]} for doc in documents]

    def save(changes, documents=(), peer=client, operation_id=None):
        return peer.call("adashi_design", operation="save", operationId=operation_id or operation(),
                         changeIntent="Verify document hash concurrency", changes=changes,
                         readTokens=tokens(documents))

    def rejected(response, code=None):
        assert "error" not in response, response
        result = response["result"]
        assert result.get("isError") is True, result
        error = result["structuredContent"]
        assert json.loads(result["content"][0]["text"]) == error
        if code:
            assert error["code"] == code, error
            assert error["stored"] is False, error
        return error

    def edit(doc, **changes):
        return {"op": "upsert_element", **doc["document"], **changes}

    def revision():
        return body(client.call("adashi_tasks", operation="list"))["revision"]

    schema = next(tool for tool in client.request("tools/list", {})["result"]["tools"]
                  if tool["name"] == "adashi_design")["inputSchema"]
    assert "guard" not in schema["properties"]
    assert "readTokens" in schema["properties"]
    for intent in ("design", "implementation"):
        injection = body(client.call("adashi_rules", operation="get_rule_injections", intend=intent, hook="run.start"))
        assert any(section["id"] == "design.write-protocol" for section in injection["sections"])
        assert "Merge" in injection["injectionPrompt"] or "merge" in injection["injectionPrompt"]

    initial = [{"op": "upsert_element", "externalId": name, "name": name,
                "elementType": "Container", "parentExternalId": "1"} for name in ("hash-a", "hash-b")]
    saved = body(save(initial))  # First call creates both; no parent guard or token.
    assert saved["stored"] and len(saved["readTokens"]) == 2
    a, b = read("element:hash-a", "element:hash-b")
    assert a["readToken"].startswith("sha256:") and len(a["readToken"]) == 71
    for args in ({"operation": "get_by_ids", "ids": ["hash-a"]},
                 {"operation": "get_scope", "elementId": "hash-a"}):
        response = body(client.call("adashi_design", **args))
        assert a in response["documents"]
    assert "version" not in a["document"]  # Hash only canonical document content.

    # Independent document updates do not invalidate each other's read tokens.
    body(save([edit(b, description="independent edit")], [b]))
    assert read(a["documentId"])[0] == a
    first = body(save([edit(a, description="concurrent description")], [a]))
    latest = read(a["documentId"])[0]
    assert first["readTokens"] == tokens([latest])
    unchanged_revision = revision()
    stale = rejected(save([edit(a, name="agent intended name")], [a]), "out_of_date")
    assert stale["currentDocument"] == latest["document"]
    assert stale["readToken"] == latest["readToken"]
    assert stale["documentId"] == a["documentId"]
    assert "Merge" in stale["request"] and "Do not only replace" in stale["request"]
    assert revision() == unchanged_revision and read(a["documentId"])[0] == latest

    # Merge the intended name onto the returned document, preserving concurrent description.
    merged = {"documentId": stale["documentId"], "document": stale["currentDocument"], "readToken": stale["readToken"]}
    merge_id = operation()
    merge_args = [edit(merged, name="agent intended name")]
    merged_result = body(save(merge_args, [merged], operation_id=merge_id))
    assert body(save(merge_args, [merged], operation_id=merge_id)) == merged_result
    assert read(a["documentId"])[0]["document"]["description"] == "concurrent description"
    reused = rejected(save([edit(merged, name="different payload")], [merged], operation_id=merge_id))
    assert "different arguments" in reused["message"]
    rejected(save([edit(merged, name="second stale retry")], [merged]), "out_of_date")

    # No token can silently turn a create into an overwrite.
    missing = rejected(save([initial[0]]), "read_required")
    assert missing["currentDocument"] == read(a["documentId"])[0]["document"]

    # Whole mixed changeset rolls back before any create when one target is stale.
    unchanged_revision = revision()
    rejected(save([{**initial[0], "externalId": "must-not-exist"}, edit(a, name="stale")], [a]), "out_of_date")
    assert read("element:must-not-exist")[0]["document"] is None
    assert revision() == unchanged_revision
    current = read(a["documentId"])[0]
    invalid = rejected(save([edit(current, elementType="InvalidType")], [current]))
    assert "c4.invalid_element_type" in json.dumps(invalid)
    assert read(a["documentId"])[0] == current and revision() == unchanged_revision

    # Narrow description writes use the same document token and return the full conflict.
    narrow = {"operation": "set_element_descriptions", "operationId": operation(),
              "readTokens": tokens([current]), "updates": [{"externalId": "hash-a", "description": "narrow write"}]}
    body(client.call("adashi_design", **narrow))
    narrow["operationId"] = operation()
    stale_narrow = rejected(client.call("adashi_design", **narrow), "out_of_date")
    assert stale_narrow["currentDocument"]["name"] == "agent intended name"

    # UML and bindings are independently hashed; a relationship delete protects cascades.
    artifacts = [
        {"op": "upsert_relationship", "externalId": "hash-link", "sourceExternalId": "hash-a",
         "destinationExternalId": "hash-b", "description": "calls"},
        {"op": "upsert_uml", "key": "HashFlow", "title": "Hash flow", "language": "mermaid",
         "diagramType": "flow", "attachedToExternalId": "hash-link", "source": "flowchart TD\n A-->B"},
        {"op": "upsert_binding", "designExternalId": "HashFlow", "targetType": "file", "target": "hash.rs"},
    ]
    body(save(artifacts))
    related = read("relationship:hash-link", "uml:HashFlow", "binding:HashFlow|file|hash.rs")
    retrieval = body(client.call("adashi_design", operation="get_bindings", files=["hash.rs"]))
    assert all(doc in retrieval["documents"] for doc in related if doc["documentId"] != "relationship:hash-link")
    uml = next(doc for doc in related if doc["documentId"] == "uml:HashFlow")
    relationship = next(doc for doc in related if doc["documentId"] == "relationship:hash-link")
    deletion = [{"op": "delete_relationship", "externalId": "hash-link"}]
    missing_dependencies = rejected(save(deletion, [relationship]), "read_required")
    assert {item["documentId"] for item in missing_dependencies["conflicts"]} == {"uml:HashFlow", "binding:HashFlow|file|hash.rs"}
    body(save([{**artifacts[1], "source": "flowchart TD\n A-->C"}], [uml]))
    cascade = rejected(save(deletion, related), "out_of_date")
    assert cascade["currentDocument"]["source"] == "flowchart TD\n A-->C"
    related = read(*(doc["documentId"] for doc in related))
    deleted = body(save(deletion, related))
    assert len(deleted["readTokens"]) == 3
    assert all(doc["document"] is None for doc in read(*(doc["documentId"] for doc in related)))
    deleted_conflict = rejected(save([artifacts[1]], [uml]), "out_of_date")
    assert deleted_conflict["currentDocument"] is None and deleted_conflict["readToken"]

    # Full mockup hashes include draft content but exclude bookkeeping timestamps/versions.
    svg = '<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><g data-adashi-layer="base" data-adashi-id="base"><rect data-adashi-id="panel" width="64" height="64" fill="#fff"/></g></svg>'
    body(save([{"op": "upsert_mockup", "externalId": "hash-mockup", "title": "Hash mockup",
                "attachedToExternalId": "hash-a", "viewportWidth": 64, "viewportHeight": 64,
                "screen": "fixture", "mockupState": "default", "fidelity": "wireframe", "acceptedSvg": svg}]))
    mockup = read("mockup:hash-mockup")[0]
    assert mockup["document"]["acceptedSvg"] == svg
    with closing(sqlite3.connect(root / "project/.adashi/adashi.sqlite3")) as db, db:
        db.execute("UPDATE ui_mockups SET updated_at='2099-01-01' WHERE external_id='hash-mockup'")
        db.execute("UPDATE resource_versions SET version=version+1 WHERE resource_id='hash-mockup'")
    assert read("mockup:hash-mockup")[0] == mockup
    draft = svg.replace("#fff", "#eee")
    with closing(sqlite3.connect(root / "project/.adashi/adashi.sqlite3")) as db, db:
        db.execute("UPDATE ui_mockups SET working_svg=?,base_revision=accepted_revision,status='workingDraft' WHERE external_id='hash-mockup'", (draft,))
    stale_mockup = rejected(save([{"op": "delete_mockup", "externalId": "hash-mockup"}], [mockup]), "out_of_date")
    assert stale_mockup["currentDocument"]["workingSvg"] == draft
    assert stale_mockup["currentDocument"]["acceptedSvg"] == svg
    body(save([{"op": "delete_mockup", "externalId": "hash-mockup"}], read("mockup:hash-mockup")))

    # Two independent MCP processes contend for exactly the same read token.
    peer = Client(binary, root)
    try:
        assert body(save(merge_args, [merged], peer=peer, operation_id=merge_id)) == merged_result
        race_base = read("element:hash-b")[0]
        ids = [operation(), operation()]
        with ThreadPoolExecutor(max_workers=2) as pool:
            futures = [pool.submit(save, [edit(race_base, name=name)], [race_base], owner, op)
                       for owner, name, op in zip((client, peer), ("race winner one", "race winner two"), ids)]
            responses = [future.result() for future in futures]
        winners = [response for response in responses if not response["result"].get("isError")]
        losers = [response for response in responses if response["result"].get("isError")]
        assert len(winners) == len(losers) == 1, responses
        conflict = rejected(losers[0], "out_of_date")
        assert conflict["currentDocument"] == read("element:hash-b")[0]["document"]
    finally:
        peer.close()

    return {"creation": "first-call creates without guards",
            "conflicts": "full document, matching token, explicit merge, no writes",
            "transactions": "mixed rollback, cascades, deleted targets, concurrent processes",
            "retries": "merged retry and identical replay succeed; stale or changed retries reject",
            "retrieval": "canonical tokens and current lifecycle/schema instructions"}
