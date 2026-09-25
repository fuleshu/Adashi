"""Real independent-clone Markdown edits, stale guards and deletion tombstones."""
from .common import raw_error, together
from .conflicts import assert_blocked


def document(peer, identity="markdown-a"):
    return peer.call("adashi_design", operation="get_documents", ids=["markdown:" + identity])["documents"][0]


def edit(peer, identity, body):
    doc = document(peer, identity)
    peer.save([{"op":"upsert_markdown", **doc["document"], "body":body}], [doc])


def markdown(suite):
    with suite.pair("markdown-references") as (a,b):
        doc=document(a)
        links=[{"targetType":"element","designExternalId":"app"},{"targetType":"markdown","designExternalId":"markdown-b"}]
        a.save([{"op":"upsert_markdown",**doc["document"],"designLinks":links},
                {"op":"upsert_binding","designExternalId":"markdown-a","targetType":"file","target":"src/design.rs"}],[doc])
        task=a.call("adashi_tasks",operation="get",taskId=suite.seed_tasks[0]["id"])["task"]
        refs=[{k:link[k] for k in ("targetType","designExternalId")} for link in task["designSpecificationLinks"]]+[{"targetType":"markdown","designExternalId":"markdown-a"}]
        a.mutate("adashi_tasks",operation="update",taskId=task["id"],expectedVersion=task["version"],designSpecificationLinks=refs)
        job=a.call("adashi_qa",operation="get_job",qaJobId=suite.seed_job["id"])["job"]
        a.mutate("adashi_qa",operation="update_job",qaJobId=job["id"],expectedVersion=job["version"],designSpecificationLinks=[refs[-1]])
        edit(b,"markdown-b","Independent linked document edit\n")
        a.commit("Markdown associations, binding and task QA references");b.commit("Linked document body")
        a.merge_from(b,"markdown-references-b")
        assert document(a)["document"]["designLinks"]==links
        scope=a.call("adashi_design",operation="get_scope",elementId="markdown-a")
        assert {r["sourceKind"] for r in scope["backlinks"]}>={"task","qa.job","binding"}
        assert a.call("adashi_tasks",operation="get",taskId=task["id"])["task"]["description"]==task["description"]
        assert document(a,"markdown-b")["document"]["body"]=="Independent linked document edit\n"
        fresh=suite.clone("markdown-references-fresh",a.folder)
        try:
            assert document(fresh)["document"]["designLinks"]==links
            assert len(fresh.call("adashi_design",operation="get_scope",elementId="markdown-a")["backlinks"])==3
        finally:fresh.close()
        suite.passed("Markdown associations, bindings and task QA links survive independent-clone merge and fresh checkout")
    with suite.pair("markdown-disjoint") as (a, b):
        old = document(a)
        together(lambda: edit(a, "markdown-a", "# Alice\n\n```rust\nlet x = 1;\n```\n"),
                 lambda: edit(b, "markdown-b", "# Bob\n\n日本語 Grüße\n"))
        a.commit("Alice Markdown"); b.commit("Bob Markdown")
        a.merge_from(b, "markdown-disjoint-b")
        assert document(a)["document"]["body"].startswith("# Alice")
        assert document(a, "markdown-b")["document"]["body"] == "# Bob\n\n日本語 Grüße\n"
        raw_error(a.raw("adashi_design", operation="save", operationId="stale-markdown", changeIntent="Stale guard check",
                        changes=[{"op":"upsert_markdown", **old["document"], "body":"Lost update"}],
                        readTokens=[{k:old[k] for k in ("documentId","readToken")}]), "out_of_date")
        suite.passed("independent Markdown documents merge exactly and stale tokens reject overwrites")
    with suite.pair("markdown-overlap") as (a, b):
        together(lambda: edit(a,"markdown-a","Alice overlap\n"), lambda: edit(b,"markdown-a","Bob overlap\n"))
        a.commit("Alice overlapping Markdown"); b.commit("Bob overlapping Markdown")
        a.merge_from(b,"markdown-overlap-b",conflict=True)
        assert_blocked(a, suite.seed_tasks[0])
        suite.passed("overlapping Markdown edits remain visible Git conflicts and block writes")
    with suite.pair("markdown-delete") as (a, b):
        doc=document(a)
        a.save([{"op":"delete_markdown","externalId":"markdown-a"}],[doc])
        edit(b,"markdown-b","Independent surviving prose\n")
        a.commit("Delete Markdown with tombstone"); b.commit("Edit retained prose")
        a.merge_from(b,"markdown-delete-b")
        assert document(a)["document"] is None
        assert document(a,"markdown-b")["document"]["body"] == "Independent surviving prose\n"
        clean=suite.clone("markdown-delete-clean",a.folder)
        try:
            assert document(clean)["document"] is None
        finally:
            clean.close()
        suite.passed("Markdown deletion tombstones survive merge and fresh clone without resurrection")
