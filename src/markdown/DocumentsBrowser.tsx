/** Project documents have their own selection; architecture navigation never invents a parent. */
import React from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Backlink, DesignAssociation, DesignOption, DocumentView, MarkdownSummary } from "./types";
import { DocumentEditor } from "./DocumentEditor";
import { ImportPanel } from "./ImportPanel";
import { projectDrafts, readDraft, writeDraft } from "./drafts";
import "./documents.css";

export function DocumentsBrowser({ projectId, changeCursor, documents, options, selectedId, onSelect, onNavigate, onBacklink, onClose }: {
  projectId: string; changeCursor: string; documents: MarkdownSummary[]; selectedId: string | null;
  options: DesignOption[];
  onClose: () => void;
  onSelect: (id: string | null) => void; onNavigate: (link: DesignAssociation) => void; onBacklink: (link: Backlink) => void;
}) {
  const [query, setQuery] = React.useState("");
  const [view, setView] = React.useState<DocumentView | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const [importing,setImporting] = React.useState(false);
  const [, refreshDrafts] = React.useReducer(n => n + 1, 0);
  const selectedRef = React.useRef(selectedId);
  const readGeneration = React.useRef(0);
  selectedRef.current = selectedId;
  React.useEffect(() => {
    let cancelled = false;
    const generation = ++readGeneration.current;
    if (!selectedId) { setView(null); return; }
    if (readDraft(projectId,selectedId)?.base === null) { setView(null); return; }
    setView(current => current?.document.documentId === `markdown:${selectedId}` ? current : null);
    invoke<DocumentView>("get_markdown_document", { projectId, externalId: selectedId })
      .then(value => { if (!cancelled && generation === readGeneration.current) { setView(value); setError(null); } })
      .catch(reason => { if (!cancelled && generation === readGeneration.current) setError(String(reason)); });
    return () => { cancelled = true; };
  }, [projectId, selectedId, changeCursor]);
  const currentView = view?.document.documentId === `markdown:${selectedId}` ? view : null;
  const document = currentView?.document.document;
  const localDrafts = projectDrafts(projectId);
  const listed = [...documents, ...localDrafts.filter(d => !documents.some(s => s.externalId === d.document.externalId)).map(d => ({...d.document,readToken:""}))];
  const isNew = selectedId && readDraft(projectId, selectedId)?.base === null;
  return <section className="documents-workspace" aria-label="Markdown designs">
    <nav className="documents-list" aria-label="Design documents">
      <button type="button" title="Return to architecture diagrams" onClick={onClose}>Architecture</button>
      <h3><button type="button" title="Browse project documents" onClick={() => onSelect(null)}>Documents ({documents.length})</button></h3>
      <button type="button" title="Create an official Markdown design document" onClick={() => {
        const externalId = `doc-${crypto.randomUUID()}`;
        writeDraft(projectId,{base:null,document:{externalId,title:"Untitled document",body:"",designLinks:[]}});
        refreshDrafts(); onSelect(externalId);
      }}>New document</button>
      <button type="button" title="Preview one existing Markdown file for explicit import" onClick={()=>setImporting(true)}>Import document</button>
      <input aria-label="Find design documents" title="Filter design document titles and identities" placeholder="Find document" value={query} onChange={event => setQuery(event.target.value)} />
      {listed.filter(d => `${d.title} ${d.externalId}`.toLowerCase().includes(query.toLowerCase())).map(d =>
        <button key={d.externalId} type="button" aria-current={d.externalId === selectedId ? "page" : undefined} title={`Open ${d.title}${readDraft(projectId,d.externalId) ? " (unsaved draft)" : ""}`} onClick={() => onSelect(d.externalId)}>{d.title}</button>)}
      {!documents.length && <p>No Markdown designs yet.</p>}
    </nav>
    <article className="document-panel" aria-label="Design document">
      {importing && <ImportPanel projectId={projectId} onSelect={onSelect} onClose={()=>setImporting(false)}/>}
      <div hidden={importing}>
      <nav aria-label="Document breadcrumbs"><button type="button" onClick={() => onSelect(null)}>Documents</button>{selectedId && <span> / {document?.title ?? readDraft(projectId,selectedId)?.document.title ?? selectedId}</span>}</nav>
      {error && <p role="alert">{error}</p>}
      {!selectedId ? <p>Select a document.</p> : !currentView && !isNew ? <p>Loading document…</p> : <>
        <h2>{document?.title ?? readDraft(projectId,selectedId)?.document.title ?? selectedId}</h2>
        <DocumentEditor key={`${projectId}:${selectedId}`} projectId={projectId} externalId={selectedId} snapshot={isNew ? null : currentView?.document ?? null} options={options} onDraftChange={refreshDrafts} onDeleted={message => {if (message) setError(message); if (selectedRef.current === selectedId) onSelect(null);}} onSaved={snapshot => {if (selectedRef.current === snapshot.document?.externalId) {readGeneration.current++;setView(current => ({document:snapshot,backlinks:current?.backlinks ?? []}));}}} />
        <section aria-label="Design associations"><h4>Related designs</h4>{document?.designLinks.map((link,index) => <button type="button" key={`${link.targetType}:${link.designExternalId}`} title="Open related design" onClick={() => onNavigate(link)}>{index+1}. {options.find(d => d.targetType === link.targetType && d.designExternalId === link.designExternalId)?.title ?? link.designExternalId}</button>)}</section>
        <section aria-label="Document backlinks"><h4>Referenced by</h4>{view?.backlinks.map(link => <button type="button" key={`${link.sourceKind}:${link.sourceId}`} disabled={link.sourceKind === "binding" && !link.sourceId.startsWith("file:")} title={`Open ${link.sourceKind}`} onClick={() => onBacklink(link)}>{link.sourceKind}: {link.title}</button>)}</section>
      </>}
      </div>
    </article>
  </section>;
}
