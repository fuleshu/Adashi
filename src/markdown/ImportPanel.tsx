import React from "react";
import { invoke } from "@tauri-apps/api/core";
import { marked } from "marked";
import { readDraft, writeDraft } from "./drafts";
import type { DocumentView, ImportPreview } from "./types";

/** Parse Markdown links without rendering or fetching assets. Relative targets require review. */
function relativeLinks(body: string): string[] {
  const links = new Set<string>();
  const add = (href:string) => {if (href && !/^(https?:|mailto:|#)/i.test(href)) links.add(href);};
  marked.walkTokens(marked.lexer(body), token => {
    if ("href" in token && typeof token.href === "string") add(token.href);
    if (token.type === "html") for (const match of token.raw.matchAll(/(?:href|src)\s*=\s*["']([^"']+)["']/gi)) add(match[1]);
  });
  return [...links];
}

/** Preview first, then stage an ordinary guarded draft. Neither step rewrites the source file. */
export function ImportPanel({projectId,onSelect,onClose}:{projectId:string;onSelect:(id:string)=>void;onClose:()=>void}) {
  const [sourcePath,setSourcePath] = React.useState("");
  const [preview,setPreview] = React.useState<ImportPreview|null>(null);
  const [title,setTitle] = React.useState("");
  const [identity,setIdentity] = React.useState("");
  const [reviewed,setReviewed] = React.useState(false);
  const [error,setError] = React.useState("");
  const [busy,setBusy] = React.useState(false);
  const links = React.useMemo(()=>relativeLinks(preview?.document.body ?? ""),[preview]);
  async function browse() {
    try {const selected=await invoke<string|null>("pick_markdown_import"); if(selected){setSourcePath(selected);setPreview(null);}}
    catch(reason){setError(String(reason));}
  }
  async function inspect() {
    setBusy(true);setError("");setPreview(null);setReviewed(false);
    try {const result=await invoke<ImportPreview>("preview_markdown_import",{projectId,sourcePath});setPreview(result);setTitle(result.document.title);setIdentity(result.document.externalId);}
    catch(reason){setError(String(reason));} finally{setBusy(false);}
  }
  async function stage() {
    if(!preview)return;
    setBusy(true);setError("");
    try {
      const existing=await invoke<DocumentView>("get_markdown_document",{projectId,externalId:identity});
      if(existing.document.document || readDraft(projectId,identity)) throw new Error("This identity already has a document or draft. Open it instead of importing again.");
      writeDraft(projectId,{base:null,document:{...preview.document,externalId:identity,title},importSource:preview.source});
      onSelect(identity);onClose();
    } catch(reason){setError(String(reason));}finally{setBusy(false);}
  }
  return <section className="import-panel" aria-label="Import Markdown design">
    <h3>Import Markdown design</h3>
    <label>Source file<input aria-label="Markdown source path" title="Select or enter the path of one existing Markdown design" value={sourcePath} onChange={event=>{setSourcePath(event.target.value);setPreview(null);}} disabled={busy}/></label>
    <div className="document-actions"><button type="button" onClick={()=>void browse()} disabled={busy}>Choose file</button><button type="button" onClick={()=>void inspect()} disabled={busy||!sourcePath}>Preview import</button><button type="button" onClick={onClose} disabled={busy}>Cancel import</button></div>
    {error && <p role="alert">{error}</p>}
    {preview && <>
      <p>Source: {preview.source.sourcePath}. The original file will be retained.</p>
      <label>Title<input aria-label="Import title" title="Title of the new official design" value={title} onChange={event=>setTitle(event.target.value)}/></label>
      <label>Design identity<input aria-label="Import identity" title="Stable identity, independent of source path and title" value={identity} onChange={event=>setIdentity(event.target.value)}/></label>
      <p>Associations: none initially. Review and add architecture or document links in the editor before saving.</p>
      <pre aria-label="Complete import preview">{preview.document.body}</pre>
      {preview.duplicates.length>0 && <p role="alert">Already imported or duplicate content: {preview.duplicates.join(", ")}. Open the existing document.</p>}
      {links.length>0 && <><p role="alert">Unresolved relative or unsupported links/assets; no files will be copied or links rewritten:</p><ul>{links.map(link=><li key={link}><code>{link}</code></li>)}</ul><label><input type="checkbox" title="Keep these targets as written and review them in the editor" checked={reviewed} onChange={event=>setReviewed(event.target.checked)}/>Keep these links for review in the editor</label></>}
      <button type="button" disabled={busy||!title.trim()||!identity.trim()||preview.duplicates.length>0||(links.length>0&&!reviewed)} onClick={()=>void stage()}>Review in editor</button>
    </>}
  </section>;
}
