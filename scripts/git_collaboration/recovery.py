"""Kill an actual MCP writer during publication, then recover through a new client."""
import json
import time
import uuid
from .common import Peer
from .conflicts import assert_blocked


def interrupted(suite):
    for interfere in (False, True):
        name = "interrupted-external" if interfere else "interrupted"
        with suite.pair(name) as (writer, unused):
            old = writer.task()
            changes = [{"op": "upsert_element", "externalId": f"crash-{i}", "parentExternalId": "app",
                        "elementType": "Component", "name": f"Recovered {i}", "description": "Complete batch"}
                       for i in range(200)]
            args = dict(operation="save", projectName="fixture", operationId=str(uuid.uuid4()),
                        changeIntent="Interrupt owned fixture process during real publication", changes=changes, readTokens=[])
            client = writer.client
            client.sequence += 1
            client.send({"jsonrpc": "2.0", "id": client.sequence, "method": "tools/call",
                         "params": {"name": "adashi_design", "arguments": args}})
            pending = writer.folder / ".adashi/local/text-transaction.json"
            deadline = time.monotonic() + 30
            while not pending.exists() and time.monotonic() < deadline:
                if not client.queue.empty():
                    raise AssertionError("Mutation finished before interruption: " + client.queue.get())
                time.sleep(0.001)
            assert pending.exists(), "No prepared publication observed"
            journal = json.loads(pending.read_bytes())
            canonical = [f for f in journal["files"] if not f["path"].startswith("$local/")]
            # Wait for at least one canonical replacement; the remaining hundreds
            # give a bounded, observable interruption window without a test backdoor.
            while time.monotonic() < deadline:
                count = sum((writer.folder / ".adashi/text" / f["path"]).exists() for f in canonical if f["before"] is None)
                if count:
                    break
                time.sleep(0.001)
            client.process.kill()
            client.process.wait(timeout=10)
            assert pending.exists(), "Writer completed before it could be interrupted"
            applied = sum((writer.folder / ".adashi/text" / f["path"]).exists()
                          and (writer.folder / ".adashi/text" / f["path"]).read_bytes() == f["after"].encode() for f in canonical)
            assert 0 < applied < len(canonical), (applied, len(canonical))
            recovery = Peer(suite, writer.folder, suite.root / (name + "-recovery-settings"))
            try:
                if interfere:
                    entry = next(f for f in canonical if f["path"].startswith("records/c4_elements/"))
                    path = writer.folder / ".adashi/text" / entry["path"]
                    path.write_bytes(b"External edit must survive\n")
                    journal_before = pending.read_bytes()
                    assert_blocked(recovery, old, "external edit")
                    assert pending.read_bytes() == journal_before and path.read_bytes() == b"External edit must survive\n"
                    # The fixture author explicitly chooses the prepared after image.
                    path.write_bytes(entry["after"].encode())
                result = recovery.call("adashi_design", **args)
                after = recovery.canonical()
                assert recovery.call("adashi_design", **args) == result
                assert recovery.canonical() == after and not pending.exists()
                names = {r["data"].get("external_id") for _, r in recovery.records("c4_elements") if not r["deleted"]}
                assert all(f"crash-{i}" in names for i in range(200))
                recovery.scope("crash-199")
                suite.passed("interrupted real writer recovers exactly once" + (" after preserving an external edit" if interfere else " before exposing data"),
                             publishedBeforeKill=applied, preparedCanonicalFiles=len(canonical))
            finally:
                recovery.close()


def prepare_native(suite):
    with suite.pair("native") as (a, b):
        from .common import write_json
        write_json(suite.root / "native-fixture.json", {
            "root": str(suite.root), "binary": suite.binary, "base": suite.base,
            "repoA": str(a.folder), "repoB": str(b.folder), "settingsA": str(a.settings), "settingsB": str(b.settings),
            "tasks": suite.seed_tasks, "job": suite.seed_job,
        })
