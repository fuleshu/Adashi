/** Project documents have their own selection; architecture navigation never invents a parent.
 *
 * The list is the "Documents" index mode. The open document renders inside the design viewer panel
 * with the tree breadcrumb on top, and everything about that document's links lives in the inspector
 * column beside it, so the content area stays content and the side panel stays links and backlinks.
 */
import React from "react";
import { ScrollText } from "lucide-react";
import type { Backlink, DesignAssociation, DesignOption, MarkdownSummary } from "./types";
import { DocumentAssociations, DocumentEditor, type DocumentSession } from "./DocumentEditor";
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

/** The open document, in the design viewer panel below the tree breadcrumb. */
export function DocumentWorkspace({ session, importing, notice, onImportClose, onSelect }: {
  session: DocumentSession; importing: boolean; notice: string | null;
  onImportClose: () => void; onSelect: (id: string | null) => void;
}) {
  if (importing) {
    return <article className="document-workspace" aria-label="Import Markdown design">
      <div className="panel-heading">
        <div>
          <p className="eyebrow">Markdown Design Document</p>
          <h3>Import document</h3>
        </div>
      </div>
      <div className="document-workspace-body">
        <ImportPanel projectId={session.projectId} onSelect={onSelect} onClose={onImportClose} />
      </div>
    </article>;
  }

  return <article className="document-workspace" aria-label="Design document">
    <div className="panel-heading">
      <div>
        <p className="eyebrow">Markdown Design Document</p>
        <h3>{session.document?.title ?? session.externalId ?? "No document selected"}</h3>
      </div>
      <div className="status-strip compact">
        <span>{session.externalId ? `markdown:${session.externalId}` : "markdown"}</span>
      </div>
    </div>
    <div className="document-workspace-body">
      {session.readError && <p role="alert">{session.readError}</p>}
      {!session.externalId ? <p>{notice ?? "Select a document in the Documents list."}</p>
        : !session.document && !session.isNew ? <p>Loading document…</p>
        : <DocumentEditor key={`${session.projectId}:${session.externalId}`} session={session} />}
    </div>
  </article>;
}

/** Everything about the open document that is not its prose: its associations and its backlinks. */
export function DocumentInspector({ session, options, onBacklink, onNavigate }: {
  session: DocumentSession; options: DesignOption[];
  onBacklink: (link: Backlink) => void; onNavigate: (link: DesignAssociation) => void;
}) {
  const backlinks = session.currentView?.backlinks ?? [];

  return <aside className="design-inspector-panel" aria-label="Document inspector">
    <div className="inspector-heading">
      <div>
        <p className="eyebrow">Design Document</p>
        <h3>{session.document?.title ?? session.externalId ?? "No document selected"}</h3>
      </div>
      <span>Markdown</span>
    </div>

    <p className="inspector-note">
      {session.externalId ? `markdown:${session.externalId}` : "Select a document in the Documents list."}
    </p>

    {session.document ? (
      <>
        <DocumentAssociations onNavigate={onNavigate} options={options} session={session} />
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
      </>
    ) : null}

    {session.readError ? <p role="alert">{session.readError}</p> : null}
  </aside>;
}
