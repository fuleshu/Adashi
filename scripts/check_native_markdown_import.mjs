/** Explicit legacy-file adoption through the actual native import/editor workflow. */
import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import {createHash,randomUUID} from "node:crypto";

export async function verifyMarkdownImport({page,call,until,passed,fixture,project}) {
  const invoke=(command,args)=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
  const source=path.join(project,"legacy-design.md");
  const body="# Legacy specification\r\n\r\nGrüße 日本語\r\n```rust\r\nlet exact = true;\r\n```\r\n\r\n![Diagram](assets/diagram.png)\r\n[Related][spec]\r\n\r\n[spec]: ../related.md\r\n";
  await fs.writeFile(source,body,"utf8");
  const sourceBefore=await fs.readFile(source);
  await page.getByRole("button",{name:"Design",exact:true}).click();
  await page.locator(".design-level-tabs").getByRole("tab",{name:"Documents",exact:true}).click();
  await page.getByRole("button",{name:"Import document",exact:true}).click();
  const importPanel=page.getByRole("region",{name:"Import Markdown design",exact:true});
  await importPanel.getByRole("textbox",{name:"Markdown source path",exact:true}).fill(source);
  await importPanel.getByRole("button",{name:"Preview import",exact:true}).click();
  await importPanel.getByText("assets/diagram.png",{exact:true}).waitFor();
  await importPanel.getByText("../related.md",{exact:true}).waitFor();
  assert.ok(await importPanel.getByRole("button",{name:"Review in editor",exact:true}).isDisabled());
  await importPanel.getByRole("checkbox",{name:"Keep these links for review in the editor",exact:true}).check();
  await importPanel.getByRole("textbox",{name:"Import identity",exact:true}).fill("adopted-spec");
  await importPanel.getByRole("button",{name:"Review in editor",exact:true}).click();
  const editor=page.getByRole("region",{name:"Edit design document",exact:true});
  await editor.getByRole("textbox",{name:"Document title",exact:true}).waitFor();
  // The editor can display normalized line endings; an unchanged body must retain original bytes.
  await invoke("set_project_architecture_projection",{projectId:"fixture",enabled:true,fileName:"AGENTS.md",markdownDirectory:"docs/adashi"});
  const output=path.join(project,"docs/adashi",`design-${createHash("sha256").update("adopted-spec").digest("hex")}.md`);
  await fs.mkdir(path.dirname(output),{recursive:true});
  await fs.writeFile(output,"Unowned document collision");
  await editor.getByRole("button",{name:"Save document",exact:true}).click();
  await editor.getByText(/Saved; generated files need retry:/).waitFor();
  const read=async()=>(await call("adashi_design",{operation:"get_documents",ids:["markdown:adopted-spec"]})).documents[0];
  assert.equal((await read()).document.body,body);
  assert.deepEqual(await fs.readFile(source),sourceBefore);
  assert.equal(await fs.readFile(output,"utf8"),"Unowned document collision");
  await fs.unlink(output); // Only this test-created file in the isolated fixture.
  const current=await read();
  await call("adashi_design",{operation:"save",operationId:randomUUID(),changeIntent:"Complete imported design",readTokens:[{documentId:current.documentId,readToken:current.readToken}],changes:[{op:"upsert_markdown",...current.document,title:"Adopted specification"}]});
  assert.ok((await fs.readFile(output,"utf8")).endsWith(body));
  await page.getByRole("button",{name:"Import document",exact:true}).click();
  await importPanel.getByRole("textbox",{name:"Markdown source path",exact:true}).fill(source);
  await importPanel.getByRole("button",{name:"Preview import",exact:true}).click();
  await importPanel.getByText(/Already imported or duplicate content: adopted-spec/).waitFor();
  assert.ok(await importPanel.getByRole("button",{name:"Review in editor",exact:true}).isDisabled());
  await importPanel.getByRole("button",{name:"Cancel import",exact:true}).click();
  const sourcePreview=await invoke("preview_markdown_import",{projectId:"fixture",sourcePath:source});
  await assert.rejects(invoke("save_markdown_document",{input:{projectId:"fixture",operationId:randomUUID(),readToken:null,document:current.document,importSource:sourcePreview.source}}));
  await assert.rejects(invoke("preview_markdown_import",{projectId:"fixture",sourcePath:output}),/generated Adashi output/);
  // Selected task adoption preserves explanatory text and the original ordered links.
  const description="Implement legacy-design.md; keep the rollback explanation.";
  await call("adashi_design",{operation:"save",operationId:randomUUID(),changeIntent:"Existing task design",changes:[{op:"upsert_markdown",externalId:"earlier-spec",title:"Earlier specification",body:"Existing design",designLinks:[]}]});
  const existing={targetType:"markdown",designExternalId:"earlier-spec"};
  const task=(await call("adashi_tasks",{operation:"create",operationId:randomUUID(),title:"Implement adopted design",description,designSpecificationLinks:[existing]})).task;
  const link={targetType:"markdown",designExternalId:"adopted-spec"};
  const updated=(await call("adashi_tasks",{operation:"update",operationId:randomUUID(),taskId:task.id,expectedVersion:task.version,designSpecificationLinks:[existing,link]})).task;
  assert.equal(updated.description,description);
  assert.deepEqual(updated.designSpecificationLinks.map(({targetType,designExternalId})=>({targetType,designExternalId})),[existing,link]);
  await call("adashi_qa",{operation:"create_job",operationId:randomUUID(),name:"Verify adopted design",command:"echo ok",designSpecificationLinks:[link]});
  await page.getByRole("button",{name:"Tasks",exact:true}).click();
  await page.getByRole("button",{name:new RegExp(`Task #${task.number} Implement adopted design`)}).click();
  await page.getByRole("button",{name:"Open Adopted specification in Design",exact:true}).click();
  await page.getByRole("heading",{name:"Adopted specification",exact:true}).first().waitFor();
  await page.getByRole("button",{name:"qa.job: Verify adopted design",exact:true}).waitFor();
  assert.deepEqual(await fs.readFile(source),sourceBefore);
  const otherSource=path.join(project,"second-import.md");
  await fs.writeFile(otherSource,"# Second imported design\n");
  const otherPreview=await invoke("preview_markdown_import",{projectId:"fixture",sourcePath:otherSource});
  await fs.writeFile(otherSource,"Source changed after preview\n");
  const input={projectId:"fixture",operationId:randomUUID(),readToken:null,document:otherPreview.document,importSource:otherPreview.source};
  await assert.rejects(invoke("save_markdown_document",{input}),/changed since preview/);
  const refreshed=await invoke("preview_markdown_import",{projectId:"fixture",sourcePath:otherSource});
  const imported=await invoke("save_markdown_document",{input:{...input,operationId:randomUUID(),document:refreshed.document,importSource:refreshed.source}});
  assert.equal(imported.sourcePath,path.normalize(otherSource));
  assert.equal(imported.document.document.body,"Source changed after preview\n");
  await page.screenshot({path:path.join(fixture,"native-markdown-import.png"),fullPage:true});
  passed("native explicit import, complete CRLF/Unicode/fences, unresolved links, duplicate rejection, source preservation, output collision diagnostics and reviewed task/QA adoption");
}
