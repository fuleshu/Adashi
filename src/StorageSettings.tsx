import React from "react";
import { invoke } from "@tauri-apps/api/core";
import "./StorageSettings.css";

type Backend = "sqlite" | "text";
type Preview = { source: Backend; target: Backend; planToken: string; counts: Record<string, number>; destinationPopulated: boolean; warnings: string[] };
type Report = { backend: Backend; backupFolder: string };
const labels: Record<Backend, string> = { sqlite: "SQLite database", text: "Git text files" };
const collections: Record<string, string> = { agent_tasks: "Tasks", c4_elements: "Design elements", c4_relationships: "Relationships", design_bindings: "Bindings", diagrams: "Diagrams", ui_mockups: "Mockups", rules: "Rules", qa_jobs: "QA jobs", qa_runs: "QA runs", project_memory_notes: "Memory notes", mutation_operations: "Operation receipts" };

export function StorageSettings({ projectId, changeCursor, onChanged }: { projectId: string; changeCursor: string; onChanged: () => Promise<void> }) {
  const [current, setCurrent] = React.useState<Backend>();
  const [target, setTarget] = React.useState<Backend>("text");
  const [preview, setPreview] = React.useState<Preview>();
  const [archive, setArchive] = React.useState(false);
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState("");
  const [report, setReport] = React.useState<Report>();
  const mounted = React.useRef(true);
  const backend = React.useRef<Backend>();
  React.useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);
  React.useEffect(() => {
    let cancelled = false;
    invoke<{ backend: Backend }>("get_project_storage", { projectId }).then(s => {
      if (!cancelled) {
        setCurrent(s.backend);
        if (backend.current !== s.backend) { setTarget(s.backend === "sqlite" ? "text" : "sqlite"); setPreview(undefined); }
        backend.current = s.backend;
      }
    }).catch(e => { if (!cancelled) setError(String(e)); });
    return () => { cancelled = true; };
  }, [projectId, changeCursor]);
  async function review() {
    setBusy(true); setError(""); setReport(undefined); setPreview(undefined); setArchive(false);
    try {
      const plan = await invoke<Preview>("preview_storage_migration", { projectId, target });
      if (mounted.current) setPreview(plan);
    } catch (e) { if (mounted.current) setError(String(e)); }
    finally { if (mounted.current) setBusy(false); }
  }
  async function convert() {
    if (!preview) return;
    setBusy(true); setError("");
    try {
      const result = await invoke<Report>("migrate_project_storage", { projectId, target: preview.target, planToken: preview.planToken, archiveDestination: archive });
      if (mounted.current) {
        setCurrent(result.backend); setTarget(result.backend === "sqlite" ? "text" : "sqlite"); setReport(result); setPreview(undefined);
        await onChanged();
      }
    } catch (e) { if (mounted.current) { setError(String(e)); setPreview(undefined); } }
    finally { if (mounted.current) setBusy(false); }
  }
  return <section className="data-panel settings-panel storage-settings" aria-label="Project storage">
    <div className="rules-panel-heading"><h3>Project storage</h3></div>
    <p>Current storage: <strong>{current ? labels[current] : "Loading…"}</strong></p>
    <p>Choose how this project saves its data. Desktop and MCP use the same selection.</p>
    <label className="storage-choice"><span>Convert to</span>
      <select aria-label="Project storage backend" title="Select storage for this project. Conversion requires a preview before switching." value={target} disabled={busy || !current} onChange={e => { setTarget(e.target.value as Backend); setPreview(undefined); setReport(undefined); }}>
        <option value="sqlite">SQLite database</option><option value="text">Git text files</option>
      </select>
    </label>
    <p>{target === "text" ? "Individual text records can be reviewed and merged in Git. Commit the storage selection and text files together." : "A single local database stores the project. Share changes by exporting or converting the project."}</p>
    <button type="button" disabled={busy || !current || current === target} onClick={() => void review()}>{busy ? "Working…" : "Review conversion"}</button>
    {preview && <div className="storage-preview">
      <h4>{labels[preview.source]} → {labels[preview.target]}</h4>
      <p>All project data, drafts, history and references will be copied and verified. A source backup will be retained before switching.</p>
      <dl>{Object.entries(collections).filter(([key]) => preview.counts[key] > 0).map(([key, label]) => <div key={key}><dt>{label}</dt><dd>{preview.counts[key]}</dd></div>)}</dl>
      <p>The project’s Git ignore rules will include shared text files and keep machine settings and conversion backups private.</p>
      <p>Reload open editors before saving after conversion. Unsaved drafts keep their original versions and may require conflict review.</p>
      {preview.warnings.map(w => <p key={w}>{w}</p>)}
      {preview.destinationPopulated && <label className="storage-replace"><input type="checkbox" checked={archive} disabled={busy} title="Retain the existing destination in the conversion backup before replacing it with the verified copy." onChange={e => setArchive(e.target.checked)} />Preserve the existing destination in a backup and replace it</label>}
      <button type="button" disabled={busy || (preview.destinationPopulated && !archive)} onClick={() => void convert()}>Convert and switch</button>
    </div>}
    {error && <p className="storage-error" role="alert">{error}</p>}
    {report && <div role="status"><p>Converted and switched to {labels[report.backend]}.</p><p>Backup: <code className="storage-backup">{report.backupFolder}</code></p></div>}
  </section>;
}
