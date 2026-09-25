import type { DocumentSnapshot, MarkdownDocument, ImportSource } from "./types";

/** Session drafts outlive panel/project unmounts. Only explicit save or discard clears them. */
export type DocumentDraft = {
  base: DocumentSnapshot | null;
  document: MarkdownDocument;
  error?: string;
  status?: string;
  importSource?: ImportSource;
};
const drafts = new Map<string, DocumentDraft>();
const key = (project: string, id: string) => JSON.stringify([project, id]);
export const readDraft = (project: string, id: string) => drafts.get(key(project, id));
export const writeDraft = (project: string, draft: DocumentDraft) => drafts.set(key(project, draft.document.externalId), draft);
export const discardDraft = (project: string, id: string) => drafts.delete(key(project, id));
export function projectDrafts(project: string) {
  return [...drafts].filter(([id]) => JSON.parse(id)[0] === project).map(([, draft]) => draft);
}
