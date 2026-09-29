/** Project documents have their own selection; architecture navigation never invents a parent.
 *
 * The list lives in the design index panel ("Documents") and the open document is rendered inside
 * the design panel, so a document never replaces the architecture workspace.
 */
import React from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Backlink, DesignAssociation, DesignOption, DocumentView, MarkdownSummary } from "./types";
import { DocumentEditor } from "./DocumentEditor";
import { ImportPanel } from "./ImportPanel";
import { projectDrafts, readDraft, writeDraft } from "./drafts";
import "./documents.css";

/** Saved documents plus session drafts that are not saved yet, in one list. */
function listedDocuments(projectId: string, documents: MarkdownSummary[]) {
  const localDrafts = projectDrafts(projectId);
  return [
    ...documents,
    ...localDrafts
      .filter((draft) => !documents.some((summary) => summary.externalId === draft.document.externalId))
      .map((draft) => ({ ...draft.document, readToken: "" })),
  ];
}

/** The "Documents" mode of the design index: create, import and pick one design document. The
 *  shared index search filters this list, so there is one filter box for every index mode. */
export function DocumentsIndex({ projectId, documents, query, selectedId, onSelect, onImport }: {
  projectId: string; documents: MarkdownSummary[]; query: string; selectedId: string | null;
  onSelect: (id: string | null) => void; onImport: () => void;
}) {
  const [, refreshDrafts] = React.useReducer((n) => n + 1, 0);
  const listed = listedDocuments(projectId, documents);
  const normalizedQuery = query.trim().toLowerCase();
  const visible = listed.filter((document) =>
    `${document.title} ${document.externalId}`.toLowerCase().includes(normalizedQuery),
  );

  return <nav className="design-list documents-index" aria-label="Design documents">
    <div className="documents-index-actions">
      <button type="button" title="Create an official Markdown design document" onClick={() => {
        const externalId = `doc-${crypto.randomUUID()}`;
        writeDraft(projectId, { base: null, document: { externalId, title: "Untitled document", body: "", designLinks: [] } });
        refreshDrafts();
        onSelect(externalId);
      }}>New document</button>
      <button type="button" title="Preview one existing Markdown file for explicit import" onClick={onImport}>Import document</button>
    </div>
    <div className="design-tree-heading">
      <span>Documents</span>
      <strong>{visible.length === documents.length ? documents.length : `${visible.length} of ${documents.length}`}</strong>
    </div>
    {visible.map((document) => (
      <button
        className={document.externalId === selectedId ? "design-list-item active" : "design-list-item"}
        key={document.externalId}
        onClick={() => onSelect(document.externalId)}
        title={`Open ${document.title}${readDraft(projectId, document.externalId) ? " (unsaved draft)" : ""}`}
        type="button"
      >
        <strong>{document.title}</strong>
        <span>
          {document.externalId}
          {document.designLinks.length === 0
            ? " · unconnected"
            : ` · ${document.designLinks.length} link${document.designLinks.length === 1 ? "" : "s"}`}
        </span>
      </button>
    ))}
    {visible.length === 0 ? <div className="empty-state compact">{listed.length === 0 ? "No Markdown designs yet." : "No document matches the filter."}</div> : null}
  </nav>;
}

/** The open document, rendered inside the design panel next to the index and inspector. */
export function DocumentWorkspace({ projectId, changeCursor, options, selectedId, importing, onImportClose, onSelect, onNavigate, onBacklink }: {
  projectId: string; changeCursor: string; options: DesignOption[];
  selectedId: string | null; importing: boolean; onImportClose: () => void;
  onSelect: (id: string | null) => void; onNavigate: (link: DesignAssociation) => void; onBacklink: (link: Backlink) => void;
}) {
  const [view, setView] = React.useState<DocumentView | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const [, refreshDrafts] = React.useReducer((n) => n + 1, 0);
  const selectedRef = React.useRef(selectedId);
  const readGeneration = React.useRef(0);
  selectedRef.current = selectedId;

  React.useEffect(() => {
    let cancelled = false;
    const generation = ++readGeneration.current;
    if (!selectedId) { setView(null); return; }
    if (readDraft(projectId, selectedId)?.base === null) { setView(null); return; }
    setView((current) => current?.document.documentId === `markdown:${selectedId}` ? current : null);
    invoke<DocumentView>("get_markdown_document", { projectId, externalId: selectedId })
      .then((value) => { if (!cancelled && generation === readGeneration.current) { setView(value); setError(null); } })
      .catch((reason) => { if (!cancelled && generation === readGeneration.current) setError(String(reason)); });
    return () => { cancelled = true; };
  }, [projectId, selectedId, changeCursor]);

  if (importing) {
    return <article className="document-workspace" aria-label="Import Markdown design">
      <ImportPanel projectId={projectId} onSelect={onSelect} onClose={onImportClose} />
    </article>;
  }

  const currentView = view?.document.documentId === `markdown:${selectedId}` ? view : null;
  const document = currentView?.document.document;
  const draft = selectedId ? readDraft(projectId, selectedId) : undefined;
  const isNew = Boolean(selectedId && draft?.base === null);

  return <article className="document-workspace" aria-label="Design document">
    <nav className="document-breadcrumbs" aria-label="Document breadcrumbs">
      <button type="button" onClick={() => onSelect(null)}>Documents</button>
      {selectedId && <span> / {document?.title ?? draft?.document.title ?? selectedId}</span>}
    </nav>
    {error && <p role="alert">{error}</p>}
    {!selectedId ? <p>Select a document.</p> : !currentView && !isNew ? <p>Loading document…</p> : <>
      <h2>{document?.title ?? draft?.document.title ?? selectedId}</h2>
      <DocumentEditor
        key={`${projectId}:${selectedId}`}
        projectId={projectId}
        externalId={selectedId}
        snapshot={isNew ? null : currentView?.document ?? null}
        options={options}
        onDraftChange={refreshDrafts}
        onDeleted={(message) => { if (message) setError(message); if (selectedRef.current === selectedId) onSelect(null); }}
        onSaved={(snapshot) => {
          if (selectedRef.current === snapshot.document?.externalId) {
            readGeneration.current++;
            setView((current) => ({ document: snapshot, backlinks: current?.backlinks ?? [] }));
          }
        }}
      />
      <section aria-label="Design associations">
        <h4>Related designs</h4>
        {document?.designLinks.map((link, index) => (
          <button type="button" key={`${link.targetType}:${link.designExternalId}`} title="Open related design" onClick={() => onNavigate(link)}>
            {index + 1}. {options.find((option) => option.targetType === link.targetType && option.designExternalId === link.designExternalId)?.title ?? link.designExternalId}
          </button>
        ))}
        {!document?.designLinks.length ? <span className="documents-index-note">Not connected to any design entity yet.</span> : null}
      </section>
      <section aria-label="Document backlinks">
        <h4>Referenced by</h4>
        {view?.backlinks.map((link) => (
          <button type="button" key={`${link.sourceKind}:${link.sourceId}`} disabled={link.sourceKind === "binding" && !link.sourceId.startsWith("file:")} title={`Open ${link.sourceKind}`} onClick={() => onBacklink(link)}>
            {link.sourceKind}: {link.title}
          </button>
        ))}
        {!view?.backlinks.length ? <span className="documents-index-note">Nothing references this document yet.</span> : null}
      </section>
    </>}
  </article>;
}
