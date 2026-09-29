import React from "react";
import { invoke } from "@tauri-apps/api/core";
import { MarkdownEditor } from "./MarkdownEditor";
import { discardDraft, readDraft, writeDraft, type DocumentDraft } from "./drafts";
import type { DesignOption, DocumentSnapshot, DocumentSaved, MarkdownDocument } from "./types";

/** A coherent title/body/association draft retains the token of its original full snapshot. */
export function DocumentEditor({ projectId, externalId, snapshot, options, onSaved, onDeleted, onDraftChange }: {
  projectId: string; externalId: string; snapshot: DocumentSnapshot | null; options: DesignOption[];
  onSaved: (snapshot: DocumentSnapshot) => void; onDeleted: (message?:string) => void; onDraftChange: () => void;
}) {
  const [draft, setDraft] = React.useState(() => readDraft(projectId, externalId));
  const [busy, setBusy] = React.useState(false);
  const [status, setStatus] = React.useState("");
  const [confirmDelete, setConfirmDelete] = React.useState(false);
  const [association, setAssociation] = React.useState("");
  const document = draft?.document ?? snapshot?.document;
  const conflict = Boolean(draft?.base && snapshot && draft.base.readToken !== snapshot.readToken);
  /** Session-local draft edits stay local; only what the parent must re-read notifies it. */
  function edit(change: Partial<MarkdownDocument>) {
    if (!document) return;
    const next: DocumentDraft = { ...draft, base: draft ? draft.base : snapshot, document: { ...document, ...change }, error: undefined };
    writeDraft(projectId, next); setDraft(next); setStatus("Unsaved changes");
    // The document inspector shows the associations, so a link change must reach it; typing a body
    // character must not, because that would re-read the whole document on every keystroke.
    if (change.designLinks !== undefined) onDraftChange();
  }
  function discard() {
    discardDraft(projectId, externalId); setDraft(undefined); setStatus("Reloaded"); onDraftChange();
    if (!snapshot?.document) onDeleted();
  }
  async function save() {
    if (!draft || busy) return;
    if (draft.base?.document && JSON.stringify(draft.document) === JSON.stringify(draft.base.document)) {
      discard(); return;
    }
    setBusy(true); setStatus("Saving…");
    try {
      const result = await invoke<DocumentSaved>("save_markdown_document", { input: {
        projectId, operationId: crypto.randomUUID(), readToken: draft.base?.readToken ?? null, document: draft.document, importSource:draft.importSource ?? null,
      }});
      // A committed save stays successful even when its derived files could not be refreshed.
      if (result.document) {
        discardDraft(projectId, externalId); setDraft(undefined); onSaved(result.document); onDraftChange();
      }
      setStatus(result.refreshError ? `Saved; reload failed: ${result.refreshError}` : result.projectionError ? `Saved; generated files need retry: ${result.projectionError}` : draft.importSource ? `Imported from ${draft.importSource.sourcePath}. Original file retained.` : "Saved");
    } catch (reason) {
      const failed = { ...draft, error: String(reason) };
      writeDraft(projectId, failed); setDraft(failed); setStatus("Save failed; draft retained");
    } finally { setBusy(false); }
  }
  async function remove() {
    if (!snapshot || busy) return;
    setBusy(true);
    try {
      const result = await invoke<DocumentSaved>("delete_markdown_document", { input: {
        projectId, operationId: crypto.randomUUID(), externalId, readToken: draft?.base?.readToken ?? snapshot.readToken,
      }});
      discardDraft(projectId, externalId); setDraft(undefined); onDraftChange();
      onDeleted(result.projectionError ? `Deleted; generated files need retry: ${result.projectionError}` : undefined);
    } catch (reason) { setStatus(`Delete failed: ${String(reason)}`); }
    finally { setBusy(false); setConfirmDelete(false); }
  }
  if (!document) return <p role="status">This document no longer exists.</p>;
  return <section className="document-editor" aria-label="Edit design document" onKeyDown={event => {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") { event.preventDefault(); void save(); }
  }}>
    <label>Title<input aria-label="Document title" title="Design document title" value={document.title} disabled={busy} onChange={event => edit({title:event.target.value})} /></label>
    {draft?.importSource && <p>Import source: {draft.importSource.sourcePath}. Review the content and associations, then save. The original file stays unchanged.</p>}
    <fieldset disabled={busy} className="document-body-field">
      <legend>Content</legend>
      <MarkdownEditor value={document.body} disabled={busy} onChange={body => edit({body})} onBlur={() => {}} placeholder="Write the design in Markdown…" startInPreview minHeight="280px" height="clamp(320px, 55vh, 720px)" />
    </fieldset>
    <section aria-label="Edit associations"><h4>Associations</h4>
      <ol>{document.designLinks.map((link, index) => <li key={`${link.targetType}:${link.designExternalId}`}>
        {options.find(option => option.targetType === link.targetType && option.designExternalId === link.designExternalId)?.title ?? link.designExternalId}
        <button type="button" disabled={busy || index === 0} title="Move association earlier" aria-label={`Move ${link.designExternalId} earlier`} onClick={() => {
          const links = [...document.designLinks]; [links[index-1],links[index]] = [links[index],links[index-1]]; edit({designLinks:links});
        }}>↑</button>
        <button type="button" disabled={busy} title="Detach association" aria-label={`Detach ${link.designExternalId}`} onClick={() => edit({designLinks:document.designLinks.filter((_,i) => i !== index)})}>Detach</button>
      </li>)}</ol>
      <select aria-label="Associate design" title="Choose an architecture or document association" value={association} onChange={event => setAssociation(event.target.value)} disabled={busy}>
        <option value="">Choose design…</option>{options.filter(o => !(o.targetType === "markdown" && o.designExternalId === externalId) && !document.designLinks.some(l => l.targetType === o.targetType && l.designExternalId === o.designExternalId)).map(o => <option key={`${o.targetType}:${o.designExternalId}`} value={JSON.stringify(o)}>{o.targetType}: {o.title}</option>)}
      </select>
      <button type="button" title="Add selected association" disabled={busy || !association} onClick={() => {const {targetType,designExternalId} = JSON.parse(association); edit({designLinks:[...document.designLinks,{targetType,designExternalId}]}); setAssociation("");}}>Associate</button>
    </section>
    {conflict && <div role="alert">The saved document changed. Your draft is retained. Reload to discard it, or review the current document before reconciling.
      <details><summary>Current saved document</summary><pre>{JSON.stringify(snapshot?.document,null,2)}</pre></details>
      {snapshot?.document && <button type="button" disabled={busy} title="Keep this draft after reviewing the current saved document" onClick={() => {
        const reconciled = {...draft!,base:snapshot,error:undefined}; writeDraft(projectId,reconciled); setDraft(reconciled); setStatus("Reconciled draft; save to apply");
      }}>Use draft after review</button>}
    </div>}
    {draft?.error && <details open><summary>Save error</summary><pre role="alert">{draft.error}</pre></details>}
    <div className="document-actions">
      <button type="button" title="Save document (Ctrl+S)" disabled={busy || !draft || !document.title.trim()} onClick={() => void save()}>Save document</button>
      <button type="button" title="Discard draft and use the current saved document" disabled={busy || !draft} onClick={discard}>{conflict ? "Reload document" : "Discard changes"}</button>
      {snapshot?.document && <button type="button" title="Delete document using its read token" disabled={busy} onClick={() => setConfirmDelete(true)}>Delete document</button>}
    </div>
    {confirmDelete && <p>Delete this document and discard its draft? <button type="button" disabled={busy} onClick={() => void remove()}>Confirm deletion</button> <button type="button" onClick={() => setConfirmDelete(false)}>Cancel deletion</button></p>}
    <p role="status" aria-live="polite">{status || (draft ? "Unsaved changes" : "Saved")}</p>
  </section>;
}
