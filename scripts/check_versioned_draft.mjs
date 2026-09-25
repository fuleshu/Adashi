import assert from "node:assert/strict";
import { newDraft, refreshDraft, savedDraft } from "../src/versionedDraft.ts";

const base = newDraft({ value: "original", version: 3 });
const dirty = { ...base, value: "my unsaved edit" };
const remote = { value: "peer edit", version: 4 };
assert.deepEqual(refreshDraft(dirty, remote), dirty, "refresh must preserve both draft AND original guard");
assert.deepEqual(refreshDraft(base, remote), newDraft(remote), "clean editors must show new content");
assert.deepEqual(refreshDraft(dirty, { value: "original", version: 3 }), dirty, "disjoint refresh preserves draft");
const saved = savedDraft(dirty, dirty.value, { value: "my unsaved edit", version: 4 });
assert.equal(saved.baseValue, saved.value);
assert.equal(saved.version, 4);
assert.deepEqual(refreshDraft(saved, { value: "later peer edit", version: 5 }), newDraft({ value: "later peer edit", version: 5 }));
const typingDuringSave = { ...dirty, value: "continued typing" };
assert.deepEqual(savedDraft(typingDuringSave, dirty.value, { value: dirty.value, version: 4 }),
  { value: "continued typing", baseValue: dirty.value, version: 4 });
const normalized = savedDraft({ ...base, value: " trimmed " }, " trimmed ", { value: "trimmed", version: 4 });
assert.deepEqual(savedDraft(typingDuringSave, dirty.value, { value: "peer won after commit", version: 5 }),
  typingDuringSave, "a late readback must not give continued typing a peer's guard");
assert.equal(normalized.value, "trimmed");
assert.equal(normalized.baseValue, "trimmed");
console.log("Versioned draft regression: stale guard, dirty/clean refresh, save acknowledgment and typing during save passed.");
