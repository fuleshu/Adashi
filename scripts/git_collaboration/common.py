"""Isolated real-Git fixtures and independent stdio MCP clients.

All repositories, private settings and evidence stay below target/git-collaboration.
Git commands never operate on the source checkout or contact a network remote.
"""
from concurrent.futures import ThreadPoolExecutor
from contextlib import contextmanager
import json
from pathlib import Path
import subprocess
import sys
import uuid

sys.dont_write_bytecode = True
from check_context_efficiency import Client, ROOT, body


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2) + "\n", encoding="utf-8", newline="\n")


def raw_error(response, contains=None):
    assert response.get("error") or response.get("result", {}).get("isError"), response
    if contains:
        assert contains in json.dumps(response), response
    return response


class Peer:
    def __init__(self, suite, folder, settings):
        self.suite, self.folder, self.settings = suite, folder, settings
        config = {"window": {"width": 1440, "height": 940, "x": None, "y": None},
                  "projects": [{"id": "fixture", "name": "Git collaboration fixture", "folder": str(folder)}],
                  "lastActiveProjectId": "fixture", "ruleTemplates": [],
                  "architectureProjection": {"enabled": False, "fileName": "AGENTS.md"}}
        for name in ("Adashi", "adashi"):
            write_json(settings / name / "settings.json", config)
        self.client = Client(suite.binary, settings)

    def raw(self, tool, **args):
        return self.client.call(tool, **{"projectName": "fixture", **args})

    def call(self, tool, **args):
        return body(self.raw(tool, **args))

    def mutate(self, tool, **args):
        return self.call(tool, operationId=str(uuid.uuid4()), **args)

    def task(self, index=0):
        return self.call("adashi_tasks", operation="get", taskId=self.suite.seed_tasks[index]["id"])["task"]

    def update_task(self, index=0, **values):
        task = self.task(index)
        return self.mutate("adashi_tasks", operation="update", taskId=task["id"], expectedVersion=task["version"], **values)["task"]

    def scope(self, element="app"):
        return self.call("adashi_design", operation="get_scope", elementId=element, childrenDepth=0, includeAncestors=False)

    def save(self, changes, documents=()):
        return self.mutate("adashi_design", operation="save", changeIntent="Isolated Git collaboration verification", changes=changes,
                           readTokens=[{k: d[k] for k in ("documentId", "readToken")} for d in documents])

    def edit_element(self, external_id="app", **fields):
        doc = next(d for d in self.scope(external_id)["documents"] if d["documentId"] == "element:" + external_id)
        self.save([{"op": "upsert_element", **doc["document"], **fields}], [doc])
        return doc

    def records(self, collection):
        return [(p, json.loads(p.read_bytes())) for p in sorted((self.folder / ".adashi/text/records" / collection).glob("*.json"))]

    def record(self, collection, **fields):
        return next((p, r) for p, r in self.records(collection) if not r["deleted"] and all(r["data"].get(k) == v for k, v in fields.items()))

    def commit(self, message):
        self.suite.git(self.folder, "add", "--all")
        self.suite.git(self.folder, "commit", "-m", message)
        return self.suite.git(self.folder, "rev-parse", "HEAD").stdout.strip()

    def publish(self, branch):
        self.suite.git(self.folder, "push", "origin", f"HEAD:refs/heads/{branch}")

    def merge_from(self, other, branch, conflict=False):
        other.publish(branch)
        self.suite.git(self.folder, "fetch", "origin", branch)
        result = self.suite.git(self.folder, "merge", "--no-edit", "FETCH_HEAD", check=False)
        assert (result.returncode != 0) == conflict, result.stdout + result.stderr
        return result

    def tracked(self):
        names = self.suite.git(self.folder, "ls-files", "-z").stdout.split("\0")
        return {p: (self.folder / p).read_bytes() for p in names if p}

    def canonical(self):
        return {p.relative_to(self.folder).as_posix(): p.read_bytes() for p in (self.folder / ".adashi/text").rglob("*.json")}

    def close(self):
        if self.client.process.poll() is None:
            self.client.close()


class Suite:
    def __init__(self, binary):
        self.binary = str(Path(binary).resolve())
        self.root = ROOT / "target/git-collaboration" / str(uuid.uuid4())
        self.root.mkdir(parents=True)
        (self.root / "empty-hooks").mkdir()
        self.remote = self.root / "origin.git"
        self.evidence = {"root": str(self.root), "binary": self.binary, "gitCommands": [], "checks": []}
        self.seed_tasks = []
        self.git(self.root, "init", "--bare", "--initial-branch=main", str(self.remote))
        seed = self.root / "seed"
        self.git(self.root, "init", "--initial-branch=main", str(seed))
        self.configure(seed)
        self.git(seed, "remote", "add", "origin", str(self.remote))
        write_json(seed / ".adashi/storage.json", {"schemaVersion": 1, "backend": {"kind": "text"}})
        (seed / ".gitignore").write_text("/.adashi/*\n!/.adashi/storage.json\n!/.adashi/text/\n!/.adashi/text/**\n/.adashi/text/**/.adashi-publish-*.tmp\n", encoding="utf-8")
        (seed / ".gitattributes").write_text("*.json text eol=lf\n", encoding="utf-8")
        peer = Peer(self, seed, self.root / "settings-seed")
        try:
            # The real MCP startup initializes the same storage used by both clients.
            peer.call("adashi_tasks", operation="list")
            peer.save([{"op": "upsert_element", "externalId": key, "parentExternalId": "1", "elementType": "Container", "name": key.title(), "description": "Shared base"} for key in ("app", "peer", "third")]
                      + [{"op": "upsert_uml", "key": "flow", "title": "Flow", "language": "mermaid", "diagramType": "flow", "attachedToExternalId": "app", "source": "flowchart TD\n A[First]\n B[Middle]\n C[Last]\n A --> B\n B --> C\n"}])
            self.seed_tasks = [peer.mutate("adashi_tasks", operation="create", title=title, description="Shared description",
                                         designSpecificationLinks=[{"designExternalId": key} for key in ("app", "peer", "third")])["task"]
                               for title in ("Git desktop task", "Disjoint task")]
            self.seed_job = peer.mutate("adashi_qa", operation="create_job", name="Base QA", command="echo base", tags=["base"], taskIds=[self.seed_tasks[0]["id"]])["job"]
            self.base = peer.commit("Shared text fixture")
            peer.publish("main")
        finally:
            peer.close()

    def git(self, cwd, *args, check=True):
        cwd = Path(cwd).resolve()
        assert cwd.is_relative_to(self.root.resolve()), cwd
        result = subprocess.run(["git", "-c", "commit.gpgsign=false", "-c", f"core.hooksPath={self.root / 'empty-hooks'}", "-C", str(cwd), *args],
                                text=True, encoding="utf-8", capture_output=True, timeout=30, creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        self.evidence["gitCommands"].append({"cwd": str(cwd.relative_to(self.root)), "args": list(args), "exitCode": result.returncode,
                                             "output": (result.stdout + result.stderr)[-6000:]})
        if check:
            assert result.returncode == 0, result.stdout + result.stderr
        return result

    def configure(self, folder):
        for key, value in (("user.name", "Adashi Git verification"), ("user.email", "fixture@example.invalid"), ("core.autocrlf", "false"), ("core.safecrlf", "false")):
            self.git(folder, "config", key, value)

    def clone(self, name, source=None):
        folder = self.root / name
        self.git(self.root, "clone", "--no-local", str(source or self.remote), str(folder))
        self.configure(folder)
        return Peer(self, folder, self.root / (name.replace("/", "-") + "-settings"))

    @contextmanager
    def pair(self, name):
        a, b = self.clone(name + "-a"), self.clone(name + "-b")
        try:
            yield a, b
        finally:
            a.close()
            b.close()
            self.flush()

    def passed(self, name, **data):
        self.evidence["checks"].append({"name": name, **data})
        self.flush()
        print("PASS", name, flush=True)

    def flush(self):
        write_json(self.root / "evidence.json", self.evidence)


def together(*actions):
    with ThreadPoolExecutor(max_workers=len(actions)) as pool:
        futures = [pool.submit(action) for action in actions]
        return [f.result() for f in futures]
