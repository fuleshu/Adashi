/** Canonical design identity and transport contracts; output paths are never identity. */
export type DesignTargetKind = "element" | "relationship" | "uml" | "mockup" | "markdown";
export type DesignAssociation = { targetType: DesignTargetKind; designExternalId: string };
export type MarkdownDocument = { externalId: string; title: string; body: string; designLinks: DesignAssociation[] };
export type MarkdownSummary = Omit<MarkdownDocument, "body"> & { readToken: string };
export type DocumentSnapshot = { documentId: string; document: MarkdownDocument | null; readToken: string };
export type Backlink = { sourceKind: string; sourceId: string; title: string };
export type DocumentView = { document: DocumentSnapshot; backlinks: Backlink[] };
export type DocumentSaved = { saved: {stored: boolean; ok: boolean; changedCount: number}; document: DocumentSnapshot | null; refreshError: string | null; projectionError: string | null };
export type DesignOption = DesignAssociation & { title: string };
export type ImportSource = {sourcePath:string;fingerprint:string};
export type ImportPreview = {source:ImportSource;document:MarkdownDocument;duplicates:string[]};
