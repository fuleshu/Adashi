/** Actual EasyMDE toolbar, canonical persistence, conflict recovery and draft lifetimes. */
import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import {createHash,randomUUID} from "node:crypto";

export async function verifyMarkdownEditor({page,call,until,passed,fixture,project,backend}) {
  const invoke=(command,args)=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
  const panel=page.getByRole("region",{name:"Edit design document",exact:true});
  const body=()=>panel.locator(".CodeMirror").evaluate(el=>el.CodeMirror.getValue());
  const edit=content=>panel.locator(".CodeMirror").evaluate((el,content)=>{el.CodeMirror.setValue(content);el.CodeMirror.focus();},content);
  const save=()=>panel.getByRole("button",{name:"Save document",exact:true}).click();
  const saved=()=>until(async()=>await panel.getByRole("status").innerText()==="Saved" &&
    !(await panel.getByRole("textbox",{name:"Document title",exact:true}).isDisabled()),"save response applied and editor enabled");
  await page.locator(".design-level-tabs").getByRole("tab",{name:"Documents",exact:true}).click();
  await page.getByRole("button",{name:"New document",exact:true}).click();
  await panel.getByRole("textbox",{name:"Document title",exact:true}).fill("Edited native design");
  // A document opens on its rendered preview; the toolbar only edits after leaving preview.
  await until(()=>panel.locator(".editor-preview").isVisible(),"document opens on its rendered preview");
  await panel.getByTitle(/^Preview/).click();
  const original="# Native edit\n\nGrüße 日本語\n```rust\nlet exact = true;\n```\n";
  await edit(original);
  await panel.locator(".CodeMirror").evaluate(el=>el.CodeMirror.setSelection({line:2,ch:0},{line:2,ch:5}));
  await panel.getByTitle(/^Bold/).click();
  const formatted=await body();
  assert.ok(formatted.includes("**Grüße**"));
  await panel.getByTitle(/^Preview/).click();
  assert.equal(await panel.locator(".editor-preview strong").innerText(),"Grüße");
  await page.screenshot({path:path.join(fixture,"native-markdown-preview.png"),fullPage:true});
  await panel.getByTitle(/^Preview/).click();
  await invoke("set_project_architecture_projection",{projectId:"fixture",enabled:true,fileName:"AGENTS.md",markdownDirectory:"docs/adashi"});
  // Hold only the real save response so native keyboard input exercises the
  // busy state deterministically; the canonical backend still executes normally.
  await page.evaluate(()=>{
    const originalInvoke=window.__TAURI_INTERNALS__.invoke;
    let release;
    const held=new Promise(resolve=>{release=resolve;});
    window.__markdownReleaseSave=release;
    window.__TAURI_INTERNALS__.invoke=(command,args)=>{
      if(command!=="save_markdown_document") return originalInvoke(command,args);
      window.__TAURI_INTERNALS__.invoke=originalInvoke;
      return originalInvoke(command,args).then(async result=>{await held;return result;});
    };
  });
  await save();
  await until(()=>panel.locator(".CodeMirror").evaluate(el=>el.CodeMirror.getOption("readOnly")==="nocursor"),"editor is read-only while save is pending");
  await panel.locator(".CodeMirror").evaluate(el=>el.CodeMirror.focus());
  await page.keyboard.type("Must not edit during save");
  assert.equal(await body(),formatted,"in-flight save prevents an edit that its response could clear");
  await page.evaluate(()=>{window.__markdownReleaseSave();delete window.__markdownReleaseSave;});
  await saved();
  const summaries=(await call("adashi_design",{operation:"list_markdown",markdownQuery:{query:"Edited native design"}})).markdown.documents;
  assert.equal(summaries.length,1);
  const id=summaries[0].externalId;
  const read=async()=>(await call("adashi_design",{operation:"get_documents",ids:[`markdown:${id}`]})).documents[0];
  assert.equal((await read()).document.body,formatted);
  const output=path.join(project,"docs/adashi",`design-${createHash("sha256").update(id).digest("hex")}.md`);
  await until(async()=> (await fs.readFile(output,"utf8").catch(()=>"")).endsWith(formatted),"canonical editor body projected exactly");
  const local=formatted+"Local unsaved edit\n";
  await edit(local);
  const before=await read();
  await call("adashi_design",{operation:"save",operationId:randomUUID(),changeIntent:"Concurrent native conflict",readTokens:[{documentId:before.documentId,readToken:before.readToken}],changes:[{op:"upsert_markdown",...before.document,body:formatted+"Remote edit\n"}]});
  await panel.getByText(/The saved document changed/).waitFor();
  assert.equal(await body(),local);
  await save();
  await panel.getByText("Save failed; draft retained",{exact:true}).waitFor();
  assert.equal(await body(),local);
  await page.locator(".design-breadcrumbs").getByRole("button",{name:"System Context",exact:true}).click();
  await page.getByRole("navigation",{name:"Design documents"}).getByRole("button",{name:"Edited native design",exact:true}).click();
  assert.equal(await body(),local,"unmount preserves draft and original guard");
  await panel.getByRole("button",{name:"Reload document",exact:true}).click();
  assert.ok((await body()).endsWith("Remote edit\n"));
  await edit("<script>window.markdownExecuted=true</script><img src=x onerror='window.markdownExecuted=true'><a href='javascript:alert(1)'>unsafe</a>\n\n[safe](https://example.com)");
  await panel.getByTitle(/^Preview/).click();
  assert.equal(await panel.locator(".editor-preview script,.editor-preview img,.editor-preview [onerror],.editor-preview a[href^='javascript:']").count(),0);
  assert.equal(await page.evaluate(()=>window.markdownExecuted),undefined);
  await panel.getByTitle(/^Preview/).click();
  await panel.getByRole("button",{name:"Discard changes",exact:true}).click();
  assert.ok((await body()).endsWith("Remote edit\n"));
  await panel.getByRole("combobox",{name:"Associate design",exact:true}).selectOption({label:"markdown: Renamed desktop specification"});
  await panel.getByRole("button",{name:"Associate",exact:true}).click();
  await save();
  await until(async()=>(await read()).document.designLinks.length===1,"association saved");
  await saved();
  await panel.getByRole("button",{name:"Detach desktop-spec",exact:true}).click();
  await save();
  await until(async()=>(await read()).document.designLinks.length===0,"association detached");
  await saved();
  // Project navigation destroys the panel; the session draft remains keyed to its original project.
  await edit("Draft through project and backend changes");
  await page.locator(".project-switcher select").selectOption("independent");
  await page.locator(".project-switcher select").selectOption("fixture");
  await page.locator(".design-level-tabs").getByRole("tab",{name:"Documents",exact:true}).click();
  await page.getByRole("navigation",{name:"Design documents"}).getByRole("button",{name:"Edited native design",exact:true}).click();
  assert.equal(await body(),"Draft through project and backend changes");
  const target=backend==="sqlite"?"text":"sqlite";
  const plan=await invoke("preview_storage_migration",{projectId:"fixture",target});
  await invoke("migrate_project_storage",{projectId:"fixture",target,planToken:plan.planToken,archiveDestination:true});
  assert.equal(await body(),"Draft through project and backend changes");
  await save();
  await until(async()=>(await read()).document.body==="Draft through project and backend changes","guarded draft saved after content-preserving backend conversion");
  await saved();
  // A discarded dirty body must never be committed by cleanup or deletion.
  await edit("Must not resurrect");
  await panel.getByRole("button",{name:"Delete document",exact:true}).click();
  await panel.getByRole("button",{name:"Confirm deletion",exact:true}).click();
  await until(async()=>(await read()).document===null,"guarded document deletion");
  await page.getByRole("button",{name:"Tasks",exact:true}).click();
  assert.equal((await read()).document,null);
  assert.equal(await fs.stat(output).catch(()=>null),null);
  await page.screenshot({path:path.join(fixture,"native-markdown-editor.png"),fullPage:true});
  passed("native EasyMDE create, toolbar/preview safety, canonical MCP/output parity, stale-save recovery, discard/unmount/project/backend draft preservation and deletion without resurrection");
}
