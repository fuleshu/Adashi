import React from "react";
import { newDraft, refreshDraft, savedDraft, type VersionedValue } from "./versionedDraft";

export type SaveDraft = (value: string, version: number) => Promise<VersionedValue | null>;

export async function savedField<T extends { version: number }>(
  result: Promise<T | null>, value: (record: T) => string,
): Promise<VersionedValue | null> {
  const record = await result;
  return record ? { value: value(record), version: record.version } : null;
}

export function useVersionedDraft(value: string, version: number, onSave: SaveDraft) {
  const [draft, setDraft] = React.useState(() => newDraft({ value, version }));
  const draftRef = React.useRef(draft);
  const saving = React.useRef(false);
  const publish = (next: typeof draft) => { draftRef.current = next; setDraft(next); };
  React.useEffect(() => { publish(refreshDraft(draftRef.current, { value, version })); }, [value, version]);
  const save = async (force = false) => {
    const submitted = draftRef.current;
    if (saving.current || (!force && submitted.value === submitted.baseValue)) return;
    saving.current = true;
    try {
      const result = await onSave(submitted.value, submitted.version);
      if (result) publish(savedDraft(draftRef.current, submitted.value, result));
    } finally { saving.current = false; }
  };
  return {
    value: draft.value,
    edit: (next: string) => publish({ ...draftRef.current, value: next }),
    save,
    conflict: draft.value !== draft.baseValue && draft.version !== version,
    reload: () => publish(newDraft({ value, version })),
  };
}

export function DraftConflict({ conflict, reload }: { conflict: boolean; reload: () => void }) {
  return conflict ? <span className="draft-conflict" role="status">
    Changed elsewhere. Your draft is kept. <button type="button" onClick={reload}>Use current value</button>
  </span> : null;
}

export function VersionedField({ value, version, onSave, multiline = false, ...attributes }: {
  value: string; version: number; onSave: SaveDraft; multiline?: boolean;
  type?: string; min?: number; max?: number; placeholder?: string;
}) {
  const draft = useVersionedDraft(value, version, onSave);
  return <>
    {multiline
      ? <textarea value={draft.value} onChange={event => draft.edit(event.target.value)} onBlur={() => void draft.save()} placeholder={attributes.placeholder} />
      : <input {...attributes} value={draft.value} onChange={event => draft.edit(event.target.value)} onBlur={() => void draft.save()} />}
    <DraftConflict conflict={draft.conflict} reload={draft.reload} />
  </>;
}
