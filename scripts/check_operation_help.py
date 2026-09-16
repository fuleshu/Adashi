"""On-demand help followed by first-attempt writes in the disposable stdio fixture."""
from copy import deepcopy
import json

from check_context_efficiency import Client, body, scratch_directory, size


def help_call(client, **arguments):
    # Deliberately no projectName: operation help must work before project setup.
    return client.request("tools/call", {"name": "adashi_help", "arguments": arguments})


def verify_projectless_help(binary):
    with scratch_directory() as root:
        client = Client(binary, root)
        try:
            help_result = body(help_call(client, tool="adashi_qa", operation="create_job"))
            assert "command" in help_result["requiredParameters"]
            assert not list(root.rglob("*.sqlite3"))
            assert not list(root.rglob("settings.json"))
        finally:
            client.close()


def metrics(client):
    tools = client.request("tools/list", {})["result"]["tools"]
    result = {"toolCount": len(tools),
              "toolDescriptionBytes": sum(len(t["description"].encode("utf-8")) for t in tools),
              "toolCatalogBytes": size(tools)}
    for intend in ("general", "design", "implementation"):
        start = body(client.call("adashi_rules", operation="get_rule_injections", intend=intend, hook="run.start"))
        result[intend + "StartupPromptBytes"] = len(start["injectionPrompt"].encode("utf-8"))
    return result


def verify_help(client):
    # No intentional failed write is used to learn a contract.
    tools = client.request("tools/list", {})["result"]["tools"]
    help_tool = next(t for t in tools if t["name"] == "adashi_help")
    assert help_tool["annotations"]["readOnlyHint"] is True
    assert help_tool["annotations"]["destructiveHint"] is False
    checked = 0
    for tool in tools:
        catalog = body(help_call(client, tool=tool["name"]))
        for operation in catalog.get("operations", [None]):
            selection = {"tool": tool["name"]}
            if operation:
                selection["operation"] = operation
            contract = body(help_call(client, **selection))
            assert contract == body(help_call(client, **selection))
            assert set(contract["requiredParameters"]) <= contract["exampleArguments"].keys()
            assert contract["parameterSchema"]["additionalProperties"] is False
            checked += 1

    descriptions = body(help_call(client, tool="adashi_design", operation="set_element_descriptions"))
    assert "updates" in descriptions["requiredParameters"]
    assert "changes" not in descriptions["parameterSchema"]["properties"]
    document = next(d for d in body(client.call("adashi_design", operation="get_by_ids", ids=["1"]))["documents"]
                    if d["document"].get("externalId") == "1")
    update = deepcopy(descriptions["exampleArguments"])
    update.update(projectName="Fixture", readTokens=[{"documentId": document["documentId"], "readToken": document["readToken"]}],
                  updates=[{"externalId": "1", "description": "Isolated operation-help fixture system"}])
    assert body(client.call("adashi_design", **update))["stored"]

    selected = body(help_call(client, tool="adashi_design", operation="save", changeTypes=["upsert_uml"]))
    variants = selected["parameterSchema"]["$defs"]["DesignChange"]["oneOf"]
    assert [v["properties"]["op"]["const"] for v in variants] == ["upsert_uml"]
    assert "MockupManifest" not in selected["parameterSchema"]["$defs"]
    assert "merge" in json.dumps(selected).lower()
    full = body(help_call(client, tool="adashi_design", operation="save"))
    assert size(selected) < size(full)
    arguments = deepcopy(selected["exampleArguments"])
    arguments["projectName"] = "Fixture"
    arguments["changes"][0]["attachedToExternalId"] = "1"
    saved = body(client.call("adashi_design", **arguments))
    assert saved["ok"] and saved["stored"]
    scope = body(client.call("adashi_design", operation="get_by_ids", ids=["new-flow"]))
    assert any(d["key"] == "new-flow" for d in scope["diagrams"])

    # A schema-valid mockup example must also pass SVG validation on its first save.
    mockup = deepcopy(body(help_call(client, tool="adashi_design", operation="save", changeTypes=["upsert_mockup"]))["exampleArguments"])
    mockup.update(projectName="Fixture", operationId="help-mockup-001")
    mockup["changes"][0]["attachedToExternalId"] = "1"
    assert body(client.call("adashi_design", **mockup))["stored"]

    create = body(help_call(client, tool="adashi_qa", operation="create_job"))["exampleArguments"]
    create.update(projectName="Fixture", command="echo adashi-help-first-call-ok")
    job = body(client.call("adashi_qa", **create))["job"]
    run = body(help_call(client, tool="adashi_qa", operation="run_jobs"))["exampleArguments"]
    run.update(projectName="Fixture", query={"jobIds": [job["id"]]})
    evidence = body(client.call("adashi_qa", **run))
    assert evidence["run"]["status"] == "passed", evidence
    assert "adashi-help-first-call-ok" in json.dumps(evidence)

    for arguments, expected in [
        ({"tool": "wrong"}, "availableTools"),
        ({"tool": "adashi_qa", "operation": "wrong"}, "availableOperations"),
        ({"tool": "adashi_design", "operation": "save", "changeTypes": ["wrong"]}, "availableChangeTypes"),
        ({"tool": "adashi_design", "operation": "save", "changeTypes": []}, "availableChangeTypes"),
    ]:
        failure = help_call(client, **arguments)
        assert "error" not in failure, failure
        assert failure["result"]["isError"] is True
        assert expected in failure["result"]["structuredContent"]["details"]

    for intend in ("general", "design", "implementation"):
        start = body(client.call("adashi_rules", operation="get_rule_injections", intend=intend, hook="run.start"))
        assert all(s["kind"] not in ("protocol", "fixedPrompt") for s in start["sections"])
        assert "expectedRevision" not in start["injectionPrompt"]
        assert "adashi_" not in start["injectionPrompt"]
    return {"contractsChecked": checked, "firstWrites": "Description update, UML, mockup, QA creation and QA execution passed",
            "fullDesignHelpBytes": size(full), "umlOnlyHelpBytes": size(selected)}
