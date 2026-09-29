/** Real Tauri document commands and native reference navigation; both backend fixtures reuse it. */
import assert from "node:assert/strict";
import path from "node:path";
import { randomUUID } from "node:crypto";

export async function verifyMarkdownDocuments({ page, call, until, passed, fixture }) {
  const invoke = (command, args) => page.evaluate(({command,args}) => window.__TAURI_INTERNALS__.invoke(command,args), {command,args});
  const get = () => invoke("get_markdown_document", {projectId:"fixture",externalId:"desktop-spec"});
  const save = (document, readToken = null) => invoke("save_markdown_document", {input:{projectId:"fixture",operationId:randomUUID(),document,readToken}});
  await page.locator(".design-level-tabs").getByRole("tab", { name: "Documents", exact: true }).click();
  await page.getByText("No Markdown designs yet.", {exact:true}).waitFor();
  const body = "# Desktop specification\n\nGrüße 日本語\n```rust\nlet exact = true;\n```\n";
  const document = {externalId:"desktop-spec",title:"Desktop specification",body,designLinks:[]};
  assert.ok((await save(document)).saved.stored);
  assert.equal((await get()).document.document.body,body);
  assert.equal((await invoke("list_markdown_documents", {projectId:"fixture",query:{limit:25}})).documents[0].externalId,document.externalId);
  await page.getByRole("navigation", {name:"Design documents"}).getByRole("button", {name:document.title,exact:true}).click();
  await page.getByRole("heading", {name:document.title,exact:true}).first().waitFor();
  const old=(await get()).document;
  await save({...document,title:"Renamed desktop specification"},old.readToken);
  await page.getByRole("heading", {name:"Renamed desktop specification",exact:true}).first().waitFor();
  assert.equal((await call("adashi_design", {operation:"get_documents",ids:["markdown:desktop-spec"]})).documents[0].document.title,"Renamed desktop specification");
  await assert.rejects(save({...document,body:"Stale overwrite"},old.readToken), /out_of_date/);
  const task=(await call("adashi_tasks", {operation:"create",operationId:randomUUID(),title:"Native Markdown task"})).task;
  await page.getByRole("button", {name:"Tasks",exact:true}).click();
  await page.getByRole("button", {name:new RegExp(`Task #${task.number} Native Markdown task`)}).click();
  const active=page.locator(".workspace-tab:not([hidden])");
  await active.locator(".task-link-search input").fill("Renamed desktop");
  await active.locator(".task-link-results button").filter({hasText:"Renamed desktop specification"}).click();
  await page.getByRole("button", {name:"Open Renamed desktop specification in Design",exact:true}).click();
  await page.getByRole("heading", {name:"Renamed desktop specification",exact:true}).first().waitFor();
  await page.getByRole("button", {name:"task: Native Markdown task",exact:true}).click();
  await until(async () => (await active.locator(".task-list-item.active").innerText()).includes("Native Markdown task"), "backlink selects exact task");
  const job=(await call("adashi_qa", {operation:"create_job",operationId:randomUUID(),name:"Native Markdown QA",command:"echo ok"})).job;
  await page.getByRole("button", {name:"QA",exact:true}).click();
  await active.locator(".task-list-item").filter({hasText:"Native Markdown QA"}).click();
  await active.getByPlaceholder("Search design", {exact:true}).fill("Renamed desktop");
  await active.locator(".task-link-results button").filter({hasText:"Renamed desktop specification"}).click();
  await active.getByTitle("Open in Design", {exact:true}).click();
  await page.getByRole("heading", {name:"Renamed desktop specification",exact:true}).first().waitFor();
  const current=(await get()).document;
  await assert.rejects(invoke("delete_markdown_document", {input:{projectId:"fixture",operationId:randomUUID(),externalId:"desktop-spec",readToken:current.readToken}}));
  const refs=(await get()).backlinks;
  assert.ok(refs.some(r=>r.sourceKind==="qa.job" && r.sourceId===String(job.id)));
  assert.ok(refs.some(r=>r.sourceKind==="task" && r.sourceId===String(task.id)));
  await page.screenshot({path:path.join(fixture,"native-markdown-documents.png"),fullPage:true});
  passed("desktop document commands, full canonical reads, rename refresh, task/QA pickers, direct links, backlinks and guarded deletion");
}
