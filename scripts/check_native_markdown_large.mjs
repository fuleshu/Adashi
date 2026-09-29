/** Full MCP -> native editor -> MCP -> nearby generated discovery path for a large body. */
import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import {randomUUID} from "node:crypto";

export async function verifyLargeMarkdown({page,call,until,passed,fixture,project}) {
  const invoke=(command,args)=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
  const body="# Large native design\n\n"+"Grüße 日本語: preserve this complete line and the whole design.\n\n".repeat(8000)+"```rust\nlet tail_is_present = true;\n```\n";
  const ancestor=(await call("adashi_design",{operation:"get_overview"})).elements.find(e=>e.elementType==="Software System");
  const changes=[{op:"upsert_markdown",externalId:"large-native",title:"Large native design",body,designLinks:[{targetType:"element",designExternalId:ancestor.externalId}]},{op:"upsert_binding",designExternalId:"large-native",targetType:"file",target:"src/native/design.rs"}];
  await call("adashi_design",{operation:"save",operationId:randomUUID(),changeIntent:"Large native acceptance",changes});
  await invoke("set_project_architecture_projection",{projectId:"fixture",enabled:true,fileName:"AGENTS.md",markdownDirectory:"docs/adashi"});
  await page.getByRole("button",{name:"Design",exact:true}).click();
  await page.locator(".design-level-tabs").getByRole("tab",{name:"Documents",exact:true}).click();
  const start=Date.now();
  await page.getByRole("navigation",{name:"Design documents"}).getByRole("button",{name:"Large native design",exact:true}).click();
  const editor=page.getByRole("region",{name:"Edit design document",exact:true});
  await editor.locator(".CodeMirror").waitFor();
  const openMs=Date.now()-start;
  assert.equal(await editor.locator(".CodeMirror").evaluate(el=>el.CodeMirror.getValue()),body);
  assert.ok(openMs<10000,`Large document open took ${openMs}ms`);
  // The document opens on its rendered preview; editing the tail needs the source view.
  await editor.getByTitle(/^Preview/).click();
  const appendStart=Date.now();
  await editor.locator(".CodeMirror").evaluate(el=>{const cm=el.CodeMirror;cm.setCursor(cm.lineCount()-1,0);cm.focus();});
  await page.keyboard.type("Native tail edit");
  const editMs=Date.now()-appendStart;
  assert.ok(editMs<5000,`Large document edit took ${editMs}ms`);
  await editor.getByTitle(/^Preview/).click();
  await editor.locator(".editor-preview").filter({hasText:"Native tail edit"}).waitFor();
  await editor.getByTitle(/^Preview/).click();
  await editor.getByRole("button",{name:"Save document",exact:true}).click();
  const read=async()=>(await call("adashi_design",{operation:"get_documents",ids:["markdown:large-native"]})).documents[0];
  await until(async()=>(await read()).document.body.endsWith("Native tail edit"),"native large save reaches MCP without truncation");
  const saved=await read();
  assert.equal(saved.document.body,body+"Native tail edit");
  const short=await fs.readFile(path.join(project,"src/native/AGENTS.md"),"utf8");
  const relative=short.match(/\]\(([^)]+design-[a-f0-9]+\.md)\)/)[1];
  const full=path.resolve(project,"src/native",relative);
  assert.ok((await fs.readFile(full,"utf8")).endsWith(saved.document.body));
  const before=await fs.stat(full,{bigint:true});
  await page.getByRole("button",{name:"Settings",exact:true}).click();
  await page.getByRole("button",{name:"Regenerate",exact:true}).click();
  assert.equal((await fs.stat(full,{bigint:true})).mtimeNs,before.mtimeNs);
  await fs.writeFile(path.join(fixture,"large-document-timings.json"),JSON.stringify({bytes:Buffer.byteLength(body),openMs,editMs},null,2));
  passed(`large MCP/native/short/full-file roundtrip (${Buffer.byteLength(body)} bytes; open ${openMs}ms; edit ${editMs}ms), preview, stable regeneration`);
}
