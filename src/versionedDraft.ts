/** The guard belongs to the content the user edited, not a later refresh. */
export type VersionedValue = { value: string; version: number };
export type VersionedDraft = VersionedValue & { baseValue: string };

export function newDraft(current: VersionedValue): VersionedDraft {
  return { ...current, baseValue: current.value };
}
export function refreshDraft(draft: VersionedDraft, current: VersionedValue): VersionedDraft {
  return draft.value === draft.baseValue ? newDraft(current) : draft;
}
export function savedDraft(draft: VersionedDraft, submitted: string, saved: VersionedValue): VersionedDraft {
  return { value: draft.value === submitted ? saved.value : draft.value,
    baseValue: saved.value, version: saved.version };
}
