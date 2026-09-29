/** Project documents have their own selection; architecture navigation never invents a parent.
 *
 * The list is the "Documents" index mode. The open document renders inside the design viewer panel,
 * so the tree breadcrumb above it and the inspector beside it stay the ones every view uses, and the
 * document's links and backlinks appear in the inspector column instead of under the editor.
 */
import React from "react";
import { invoke } from "@tauri-apps/api/core";
import { Link2, ScrollText } from "lucide-react";
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

/** One complete read of the selected document; a draft-only document has no saved snapshot yet. */
function useDocumentView(projectId: string, selectedId: string | null, changeCursor: string, refreshToken: number) {
  const [view, setView] = React.useState<DocumentView | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const generation = React.useRef(0);

  React.useEffect(() => {
    let cancelled = false;
    const current = ++generation.current;
    if (!selectedId || readDraft(projectId, selectedId)?.base === null) { setView(null); return; }
    setView((existing) => existing?.document.documentId === `markdown:${selectedId}` ? existing : null);
    invoke<DocumentView>("get_markdown_document", { projectId, externalId: selectedId })
      .then((value) => { if (!cancelled && current === generation.current) { setView(value); setError(null); } })
      .catch((reason) => { if (!cancelled && current === generation.current) setError(String(reason)); });
    return () => { cancelled = true; };
  }, [projectId, selectedId, changeCursor, refreshToken]);

  return { view, error, setView, setError, generation };
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
        aria-label={document.title}
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

/** The open document, rendered in the design viewer panel while a document is selected. */
export function DocumentWorkspace({ projectId, changeCursor, options, selectedId, importing, refreshToken, onImportClose, onSelect, onDraftChange }: {
  projectId: string; changeCursor: string; options: DesignOption[];
  selectedId: string | null; importing: boolean; refreshToken: number;
  onImportClose: () => void; onSelect: (id: string | null) => void; onDraftChange: () => void;
}) {
  const { view, error, setView, setError, generation } = useDocumentView(projectId, selectedId, changeCursor, refreshToken);
  const selectedRef = React.useRef(selectedId);
  selectedRef.current = selectedId;

  if (importing) {
    return <article className="document-workspace" aria-label="Import Markdown design">
      <div className="panel-heading">
        <div>
          <p className="eyebrow">Markdown Design Document</p>
          <h3>Import document</h3>
        </div>
      </div>
      <div className="document-workspace-body">
        <ImportPanel projectId={projectId} onSelect={onSelect} onClose={onImportClose} />
      </div>
    </article>;
  }

  const currentView = view?.document.documentId === `markdown:${selectedId}` ? view : null;
  const document = currentView?.document.document;
  const draft = selectedId ? readDraft(projectId, selectedId) : undefined;
  const isNew = Boolean(selectedId && draft?.base === null);

  return <article className="document-workspace" aria-label="Design document">
    <div className="panel-heading">
      <div>
        <p className="eyebrow">Markdown Design Document</p>
        <h3>{document?.title ?? draft?.document.title ?? (selectedId ? selectedId : "No document selected")}</h3>
      </div>
      <div className="status-strip compact">
        <span>{selectedId ? `markdown:${selectedId}` : "markdown"}</span>
      </div>
    </div>
    <div className="document-workspace-body">
      {error && <p role="alert">{error}</p>}
      {!selectedId ? <p>Select a document in the Documents list.</p> : !currentView && !isNew ? <p>Loading document…</p> : (
        <DocumentEditor
          key={`${projectId}:${selectedId}`}
          projectId={projectId}
          externalId={selectedId}
          snapshot={isNew ? null : currentView?.document ?? null}
          options={options}
          onDraftChange={onDraftChange}
          onDeleted={(message) => { if (message) setError(message); if (selectedRef.current === selectedId) onSelect(null); }}
          onSaved={(snapshot) => {
            if (selectedRef.current === snapshot.document?.externalId) {
              generation.current++;
              setView((current) => ({ document: snapshot, backlinks: current?.backlinks ?? [] }));
            }
          }}
        />
      )}
    </div>
  </article>;
}

/** The document's own inspector: where it points, and what points back at it. */
export function DocumentInspector({ changeCursor, options, projectId, refreshToken, selectedId, onBacklink, onNavigate }: {
  changeCursor: string; options: DesignOption[]; projectId: string; refreshToken: number;
  selectedId: string | null; onBacklink: (link: Backlink) => void; onNavigate: (link: DesignAssociation) => void;
}) {
  const { view, error } = useDocumentView(projectId, selectedId, changeCursor, refreshToken);
  const draft = selectedId ? readDraft(projectId, selectedId) : undefined;
  const currentView = view?.document.documentId === `markdown:${selectedId}` ? view : null;
  const document = currentView?.document.document ?? null;
  const title = document?.title ?? draft?.document.title ?? (selectedId ? selectedId : "No document selected");
  const links = document?.designLinks ?? draft?.document.designLinks ?? [];
  const backlinks = currentView?.backlinks ?? [];

  return <aside className="design-inspector-panel" aria-label="Document inspector">
    <div className="inspector-heading">
      <div>
        <p className="eyebrow">Design Document</p>
        <h3>{title}</h3>
      </div>
      <span>Markdown</span>
    </div>

    <p className="inspector-note">
      {selectedId ? `markdown:${selectedId}` : "Select a document in the Documents list."}
    </p>

    <section className="inspector-section">
      <h4>Related designs</h4>
      {links.length === 0 ? (
        <p className="inspector-note">Not connected to any design entity yet. Connect one in the document's Associations.</p>
      ) : (
        <div className="inspector-document-list">
          {links.map((link, index) => (
            <button
              className="inspector-document-link"
              key={`${link.targetType}:${link.designExternalId}`}
              onClick={() => onNavigate(link)}
              title="Open the related design entity"
              type="button"
            >
              <Link2 size={14} />
              <span>{index + 1}. {options.find((option) => option.targetType === link.targetType && option.designExternalId === link.designExternalId)?.title ?? link.designExternalId}</span>
            </button>
          ))}
        </div>
      )}
    </section>

    <section className="inspector-section">
      <h4>Referenced by</h4>
      {backlinks.length === 0 ? (
        <p className="inspector-note">Nothing references this document yet.</p>
      ) : (
        <div className="inspector-document-list">
          {backlinks.map((link) => (
            <button
              className="inspector-document-link"
              disabled={link.sourceKind === "binding" && !link.sourceId.startsWith("file:")}
              key={`${link.sourceKind}:${link.sourceId}`}
              onClick={() => onBacklink(link)}
              title={`Open ${link.sourceKind}`}
              type="button"
            >
              <ScrollText size={14} />
              <span>{link.sourceKind}: {link.title}</span>
            </button>
          ))}
        </div>
      )}
    </section>

    {error ? <p role="alert">{error}</p> : null}
  </aside>;
}
