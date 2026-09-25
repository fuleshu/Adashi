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
  // A dashboard readback can already contain a peer's write. Never rebase
  // continued typing onto content the user did not edit or explicitly accept.
  if (draft.value !== submitted && saved.value !== submitted) return draft;
  return { value: draft.value === submitted ? saved.value : draft.value,
    baseValue: saved.value, version: saved.version };
}
