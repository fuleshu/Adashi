import React from "react";
import { invoke } from "@tauri-apps/api/core";
import { MarkdownEditor } from "./MarkdownEditor";
import { discardDraft, readDraft, writeDraft, type DocumentDraft } from "./drafts";
import type { DesignAssociation, DesignOption, DocumentSnapshot, DocumentSaved, DocumentView, MarkdownDocument } from "./types";

/**
 * One open document: its complete read, its session draft and the guarded mutations. The main
 * column renders the content and the save actions, the inspector column renders the associations,
 * and both share this single session instead of holding half of the document each.
 */
export function useDocumentSession({ projectId, externalId, changeCursor, onDeleted, onDraftChange }: {
  projectId: string; externalId: string | null; changeCursor: string;
  onDeleted: (message?: string) => void; onDraftChange: () => void;
}) {
  const [view, setView] = React.useState<DocumentView | null>(null);
  const [readError, setReadError] = React.useState<string | null>(null);
  const [busy, setBusy] = React.useState(false);
  const [status, setStatus] = React.useState("");
  const [confirmDelete, setConfirmDelete] = React.useState(false);
  const [association, setAssociation] = React.useState("");
  const [handledId, setHandledId] = React.useState(externalId);
  const [, refreshDraft] = React.useReducer((n) => n + 1, 0);
  const generation = React.useRef(0);

  // Pending prompts and status belong to one document; the draft itself is session state in drafts.ts.
  if (handledId !== externalId) {
    setHandledId(externalId);
    setBusy(false);
    setStatus("");
    setConfirmDelete(false);
    setAssociation("");
    setReadError(null);
  }

  React.useEffect(() => {
    let cancelled = false;
    const current = ++generation.current;
    if (!externalId || readDraft(projectId, externalId)?.base === null) { setView(null); return; }
    setView((existing) => existing?.document.documentId === `markdown:${externalId}` ? existing : null);
    invoke<DocumentView>("get_markdown_document", { projectId, externalId })
      .then((value) => { if (!cancelled && current === generation.current) { setView(value); setReadError(null); } })
      .catch((reason) => { if (!cancelled && current === generation.current) setReadError(String(reason)); });
    return () => { cancelled = true; };
  }, [projectId, externalId, changeCursor]);

  const draft = externalId ? readDraft(projectId, externalId) : undefined;
  const currentView = view?.document.documentId === `markdown:${externalId}` ? view : null;
  const snapshot = currentView?.document ?? null;
  const document = draft?.document ?? snapshot?.document;
  const conflict = Boolean(draft?.base && snapshot && draft.base.readToken !== snapshot.readToken);
  const isNew = Boolean(externalId && draft?.base === null);
  const exists = Boolean(document);

  function edit(change: Partial<MarkdownDocument>) {
    if (!document) return;
    writeDraft(projectId, { ...draft, base: draft ? draft.base : snapshot, document: { ...document, ...change }, error: undefined });
    refreshDraft();
    // The tree and breadcrumb follow the document's associations, so a link change must reach them;
    // typing a body character must not, because that would re-read the whole document per keystroke.
    if (change.designLinks !== undefined) onDraftChange();
  }

  function discard() {
    if (!externalId) return;
    discardDraft(projectId, externalId);
    refreshDraft();
    setStatus("Reloaded");
    onDraftChange();
    if (!snapshot?.document) onDeleted();
  }

  /** Keep the reviewed draft but adopt the current saved snapshot as its guard. */
  function reconcile() {
    if (!draft || !snapshot || !externalId) return;
    writeDraft(projectId, { ...draft, base: snapshot, error: undefined });
    refreshDraft();
    setStatus("Reconciled draft; save to apply");
  }

  async function save() {
    if (!draft || busy || !externalId) return;
    if (draft.base?.document && JSON.stringify(draft.document) === JSON.stringify(draft.base.document)) {
      discard();
      return;
    }
    setBusy(true);
    setStatus("Saving…");
    try {
      const result = await invoke<DocumentSaved>("save_markdown_document", { input: {
        projectId, operationId: crypto.randomUUID(), readToken: draft.base?.readToken ?? null, document: draft.document, importSource: draft.importSource ?? null,
      } });
      // A committed save stays successful even when its derived files could not be refreshed.
      if (result.document) {
        discardDraft(projectId, externalId);
        refreshDraft();
        setView({ document: result.document, backlinks: currentView?.backlinks ?? [] });
        onDraftChange();
      }
      setStatus(result.refreshError ? `Saved; reload failed: ${result.refreshError}` : result.projectionError ? `Saved; generated files need retry: ${result.projectionError}` : draft.importSource ? `Imported from ${draft.importSource.sourcePath}. Original file retained.` : "Saved");
    } catch (reason) {
      writeDraft(projectId, { ...draft, error: String(reason) });
      refreshDraft();
      setStatus("Save failed; draft retained");
    } finally { setBusy(false); }
  }

  async function remove() {
    if (!snapshot || busy || !externalId) return;
    setBusy(true);
    try {
      const result = await invoke<DocumentSaved>("delete_markdown_document", { input: {
        projectId, operationId: crypto.randomUUID(), externalId, readToken: draft?.base?.readToken ?? snapshot.readToken,
      } });
      discardDraft(projectId, externalId);
      refreshDraft();
      onDraftChange();
      onDeleted(result.projectionError ? `Deleted; generated files need retry: ${result.projectionError}` : undefined);
    } catch (reason) { setStatus(`Delete failed: ${String(reason)}`); setConfirmDelete(false); }
    finally { setBusy(false); }
  }

  return {
    projectId, externalId, document, draft, snapshot, currentView, conflict, isNew, exists, busy, status,
    confirmDelete, setConfirmDelete, association, setAssociation, readError, edit, discard, reconcile, save, remove,
  };
}

export type DocumentSession = ReturnType<typeof useDocumentSession>;

/** The document body and its guarded save actions; the associations live in the inspector. */
export function DocumentEditor({ session }: { session: DocumentSession }) {
  const { document, draft, conflict, status, busy, confirmDelete, setConfirmDelete, snapshot, save, discard, reconcile, remove, edit } = session;

  if (!document) return <p role="status">This document no longer exists.</p>;

  return <section className="document-editor" aria-label="Edit design document" onKeyDown={(event) => {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") { event.preventDefault(); void save(); }
  }}>
    <label>Title<input aria-label="Document title" title="Design document title" value={document.title} disabled={busy} onChange={(event) => edit({ title: event.target.value })} /></label>
    {draft?.importSource && <p>Import source: {draft.importSource.sourcePath}. Review the content and associations, then save. The original file stays unchanged.</p>}
    <fieldset disabled={busy} className="document-body-field">
      <legend>Content</legend>
      <MarkdownEditor value={document.body} disabled={busy} onChange={(body) => edit({ body })} onBlur={() => {}} placeholder="Write the design in Markdown…" startInPreview minHeight="280px" height="clamp(320px, 55vh, 720px)" />
    </fieldset>
    {conflict && <div role="alert">The saved document changed. Your draft is retained. Reload to discard it, or review the current document before reconciling.
      <details><summary>Current saved document</summary><pre>{JSON.stringify(snapshot?.document, null, 2)}</pre></details>
      {snapshot?.document && <button type="button" disabled={busy} title="Keep this draft after reviewing the current saved document" onClick={reconcile}>Use draft after review</button>}
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

/** The ordered associations of the open document, edited in the inspector column. */
export function DocumentAssociations({ session, options, onNavigate }: {
  session: DocumentSession; options: DesignOption[]; onNavigate: (link: DesignAssociation) => void;
}) {
  const { document, busy, association, setAssociation, edit } = session;

  if (!document) return null;

  return <section className="inspector-section document-associations">
    <h4>Associations</h4>
    {document.designLinks.length === 0 ? (
      <p className="inspector-note">Not connected to any design entity yet. Choose one below to connect this document.</p>
    ) : (
      <ol className="document-association-list">
        {document.designLinks.map((link, index) => (
          <li key={`${link.targetType}:${link.designExternalId}`}>
            <button
              className="document-association-target"
              onClick={() => onNavigate(link)}
              title="Open the related design entity"
              type="button"
            >
              {index + 1}. {options.find((option) => option.targetType === link.targetType && option.designExternalId === link.designExternalId)?.title ?? link.designExternalId}
            </button>
            <button type="button" disabled={busy || index === 0} title="Move association earlier" aria-label={`Move ${link.designExternalId} earlier`} onClick={() => {
              const links = [...document.designLinks];
              [links[index - 1], links[index]] = [links[index], links[index - 1]];
              edit({ designLinks: links });
            }}>↑</button>
            <button type="button" disabled={busy} title="Detach association" aria-label={`Detach ${link.designExternalId}`} onClick={() => edit({ designLinks: document.designLinks.filter((_, i) => i !== index) })}>Detach</button>
          </li>
        ))}
      </ol>
    )}
    <div className="document-association-add">
      <select aria-label="Associate design" title="Choose an architecture or document association" value={association} onChange={(event) => setAssociation(event.target.value)} disabled={busy}>
        <option value="">Choose design…</option>
        {options.filter((option) => !(option.targetType === "markdown" && option.designExternalId === document.externalId) && !document.designLinks.some((link) => link.targetType === option.targetType && link.designExternalId === option.designExternalId)).map((option) => (
          <option key={`${option.targetType}:${option.designExternalId}`} value={JSON.stringify(option)}>{option.targetType}: {option.title}</option>
        ))}
      </select>
      <button type="button" title="Add the selected association" disabled={busy || !association} onClick={() => {
        const { targetType, designExternalId } = JSON.parse(association);
        edit({ designLinks: [...document.designLinks, { targetType, designExternalId }] });
        setAssociation("");
      }}>Associate</button>
    </div>
  </section>;
}
