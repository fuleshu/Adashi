import React from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  addAnnotation,
  addPrimitive,
  deleteElements,
  describeElement,
  duplicateElements,
  editElement,
  eraseAnnotation,
  groupElements,
  logicalSelectionElement,
  MockupAnnotation,
  MockupEditOperation,
  MockupEditorSnapshot,
  MockupPrimitive,
  orderElements,
  resizeElement,
  translateElements,
  ungroupElements,
} from "./mockupEditorModel";

type EditorTool = "select" | "pan" | "pencil" | "eraser" | "callout";

type EditorMockup = {
  externalId: string;
  acceptedRevision: number;
  acceptedSvg: string;
  workingSvg?: string | null;
  editOperations: MockupEditOperation[];
  annotations: MockupAnnotation[];
  manifest: { viewportWidth: number; viewportHeight: number };
};

type DragState = {
  kind: "move" | "resize" | "pencil" | "marquee" | "pan";
  startX: number;
  startY: number;
  startSnapshot: MockupEditorSnapshot;
  bbox?: { x: number; y: number; width: number; height: number };
  points?: Array<[number, number]>;
  currentX?: number;
  currentY?: number;
  startClientX?: number;
  startClientY?: number;
  startPan?: { x: number; y: number };
  baseSelection?: string[];
};

export function MockupEditor({
  projectId,
  projectRevision,
  mockup,
  onDashboardChange,
  onError,
  onRequestRevision,
}: {
  projectId: string;
  projectRevision: number;
  mockup: EditorMockup;
  onDashboardChange: (payload: unknown) => void;
  onError: (message: string) => void;
  onRequestRevision: (revision: number) => void;
}) {
  const initial = React.useMemo<MockupEditorSnapshot>(() => ({
    svg: mockup.workingSvg ?? mockup.acceptedSvg,
    operations: mockup.editOperations,
    annotations: mockup.annotations,
  }), [mockup.externalId]);
  const [present, setPresent] = React.useState(initial);
  const [past, setPast] = React.useState<MockupEditorSnapshot[]>([]);
  const [future, setFuture] = React.useState<MockupEditorSnapshot[]>([]);
  const [selection, setSelection] = React.useState<string[]>([]);
  const [tool, setTool] = React.useState<EditorTool>("select");
  const [dirty, setDirty] = React.useState(false);
  const [saving, setSaving] = React.useState(false);
  const [lastSaved, setLastSaved] = React.useState<string | null>(mockup.workingSvg ? "Recovered draft" : null);
  const [drag, setDrag] = React.useState<DragState | null>(null);
  const [resizeHandle, setResizeHandle] = React.useState<React.CSSProperties | null>(null);
  const [zoom, setZoom] = React.useState(100);
  const [fitScale, setFitScale] = React.useState(1);
  const [pan, setPan] = React.useState({ x: 0, y: 0 });
  const [spacePan, setSpacePan] = React.useState(false);
  const canvasRef = React.useRef<HTMLDivElement>(null);
  const latestRevision = React.useRef(projectRevision);
  const queuedSave = React.useRef(false);
  const editVersion = React.useRef(0);
  const spacePressed = React.useRef(false);

  React.useEffect(() => { latestRevision.current = projectRevision; }, [projectRevision]);
  React.useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    canvas.querySelectorAll(".adashi-editor-selected").forEach((element) => element.classList.remove("adashi-editor-selected"));
    selection.forEach((id) => {
      const element = Array.from(canvas.querySelectorAll<SVGGraphicsElement>("[data-adashi-id]")).find((candidate) => candidate.dataset.adashiId === id);
      element?.classList.add("adashi-editor-selected");
    });
    const selected = selection.length === 1 ? Array.from(canvas.querySelectorAll<SVGGraphicsElement>("[data-adashi-id]")).find((candidate) => candidate.dataset.adashiId === selection[0]) : null;
    if (selected && canvas.getBoundingClientRect().width > 0) {
      const box = selected.getBoundingClientRect();
      const root = canvas.getBoundingClientRect();
      setResizeHandle({ left: box.right - root.left - 7, top: box.bottom - root.top - 7 });
    } else setResizeHandle(null);
  }, [fitScale, pan.x, pan.y, present.svg, selection, zoom]);

  React.useLayoutEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const update = () => {
      const width = Math.max(canvas.clientWidth - 48, 1);
      const height = Math.max(canvas.clientHeight - 48, 1);
      setFitScale(Math.min(width / mockup.manifest.viewportWidth, height / mockup.manifest.viewportHeight));
    };
    update();
    const observer = new ResizeObserver(update);
    observer.observe(canvas);
    return () => observer.disconnect();
  }, [mockup.manifest.viewportHeight, mockup.manifest.viewportWidth]);

  React.useEffect(() => {
    if (!dirty || saving) return;
    const timer = window.setTimeout(() => { void saveDraft(); }, 700);
    return () => window.clearTimeout(timer);
  }, [dirty, present, saving]);

  function commit(next: MockupEditorSnapshot, nextSelection = selection) {
    editVersion.current += 1;
    setPast((items) => [...items.slice(-99), present]);
    setPresent(next);
    setFuture([]);
    setSelection(nextSelection);
    setDirty(true);
  }

  function commitFrom(start: MockupEditorSnapshot, next: MockupEditorSnapshot, nextSelection = selection) {
    editVersion.current += 1;
    setPast((items) => [...items.slice(-99), start]);
    setPresent(next);
    setFuture([]);
    setSelection(nextSelection);
    setDirty(true);
  }

  function undo() {
    const previous = past[past.length - 1];
    if (!previous) return;
    editVersion.current += 1;
    setPast((items) => items.slice(0, -1));
    setFuture((items) => [present, ...items].slice(0, 100));
    setPresent(previous);
    setSelection([]);
    setDirty(true);
  }

  function redo() {
    const next = future[0];
    if (!next) return;
    editVersion.current += 1;
    setFuture((items) => items.slice(1));
    setPast((items) => [...items, present].slice(-100));
    setPresent(next);
    setSelection([]);
    setDirty(true);
  }

  async function saveDraft(): Promise<number | null> {
    if (saving) {
      queuedSave.current = true;
      return null;
    }
    setSaving(true);
    queuedSave.current = false;
    const snapshot = present;
    const savedVersion = editVersion.current;
    try {
      const payload = await invoke<{ revision: number }>("save_mockup_draft", {
        projectId,
        input: {
          externalId: mockup.externalId,
          workingSvg: snapshot.svg,
          baseRevision: mockup.acceptedRevision,
          expectedRevision: latestRevision.current,
          editOperations: snapshot.operations,
          annotations: snapshot.annotations,
        },
      });
      latestRevision.current = payload.revision;
      onDashboardChange(payload);
      setDirty((current) => current && editVersion.current !== savedVersion);
      setLastSaved(new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" }));
      return payload.revision;
    } catch (error) {
      onError(String(error));
      return null;
    } finally {
      setSaving(false);
      if (queuedSave.current) window.setTimeout(() => void saveDraft(), 0);
    }
  }

  async function requestRevision() {
    const revision = dirty ? await saveDraft() : latestRevision.current;
    if (revision !== null) onRequestRevision(revision);
  }

  function coordinates(event: React.PointerEvent): [number, number] {
    const svg = canvasRef.current?.querySelector("svg");
    if (!svg) return [0, 0];
    const rect = svg.getBoundingClientRect();
    const viewBox = svg.viewBox.baseVal;
    const width = viewBox.width || mockup.manifest.viewportWidth;
    const height = viewBox.height || mockup.manifest.viewportHeight;
    return [viewBox.x + (event.clientX - rect.left) * width / rect.width, viewBox.y + (event.clientY - rect.top) * height / rect.height];
  }

  function setZoomAt(nextZoom: number, clientX?: number, clientY?: number) {
    const bounded = Math.min(220, Math.max(25, Math.round(nextZoom)));
    if (bounded === zoom) return;
    const canvas = canvasRef.current;
    if (canvas && clientX !== undefined && clientY !== undefined) {
      const rect = canvas.getBoundingClientRect();
      const anchorX = clientX - (rect.left + rect.width / 2);
      const anchorY = clientY - (rect.top + rect.height / 2);
      const ratio = bounded / zoom;
      setPan((current) => ({
        x: anchorX - (anchorX - current.x) * ratio,
        y: anchorY - (anchorY - current.y) * ratio,
      }));
    }
    setZoom(bounded);
  }

  function fitCanvas() {
    setZoom(100);
    setPan({ x: 0, y: 0 });
  }

  function marqueeSelection(left: number, top: number, right: number, bottom: number): string[] {
    const root = canvasRef.current;
    if (!root) return [];
    const result: string[] = [];
    const seen = new Set<string>();
    root.querySelectorAll<SVGGraphicsElement>(".mockup-edit-source [data-adashi-id]").forEach((candidate) => {
      const logical = logicalSelectionElement(candidate, false) as SVGGraphicsElement | null;
      const id = logical?.dataset.adashiId;
      if (!logical || !id || seen.has(id) || logical.dataset.role === "background") return;
      seen.add(id);
      const rect = logical.getBoundingClientRect();
      if (rect.width > 0 && rect.height > 0 && rect.left >= left && rect.right <= right && rect.top >= top && rect.bottom <= bottom) result.push(id);
    });
    return result;
  }

  function onPointerDown(event: React.PointerEvent<HTMLDivElement>) {
    event.currentTarget.closest<HTMLElement>(".mockup-editor")?.focus({ preventScroll: true });
    if (event.button === 1 || tool === "pan" || spacePressed.current) {
      event.preventDefault();
      event.currentTarget.setPointerCapture(event.pointerId);
      setDrag({
        kind: "pan",
        startX: 0,
        startY: 0,
        startClientX: event.clientX,
        startClientY: event.clientY,
        startPan: pan,
        startSnapshot: present,
      });
      return;
    }
    if (event.button !== 0) return;
    const [x, y] = coordinates(event);
    if (tool === "pencil") {
      event.currentTarget.setPointerCapture(event.pointerId);
      setDrag({ kind: "pencil", startX: x, startY: y, startSnapshot: present, points: [[x, y]] });
      return;
    }
    const annotation = (event.target as Element).closest<SVGElement>("[data-annotation-id]");
    if (tool === "eraser" && annotation?.dataset.annotationId) {
      commit(eraseAnnotation(present, annotation.dataset.annotationId), []);
      return;
    }
    if (tool === "callout") {
      const label = window.prompt("Short callout text (optional)", "Change this");
      if (label !== null) commit(addAnnotation(present, `M ${x} ${y} l 28 -28`, label), []);
      return;
    }
    const target = logicalSelectionElement(event.target as Element, event.altKey) as SVGGraphicsElement | null;
    if (!target?.dataset.adashiId || target.dataset.role === "background") {
      event.currentTarget.setPointerCapture(event.pointerId);
      setDrag({ kind: "marquee", startX: x, startY: y, currentX: x, currentY: y, startClientX: event.clientX, startClientY: event.clientY, startSnapshot: present, baseSelection: event.shiftKey ? selection : [] });
      return;
    }
    const id = target.dataset.adashiId;
    const nextSelection = event.shiftKey ? (selection.includes(id) ? selection.filter((item) => item !== id) : [...selection, id]) : selection.includes(id) ? selection : [id];
    setSelection(nextSelection);
    if (event.shiftKey && selection.includes(id)) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    setDrag({ kind: "move", startX: x, startY: y, startSnapshot: present });
  }

  function beginResize(event: React.PointerEvent<HTMLButtonElement>) {
    if (selection.length !== 1) return;
    event.stopPropagation();
    const element = Array.from(canvasRef.current?.querySelectorAll<SVGGraphicsElement>("[data-adashi-id]") ?? []).find((candidate) => candidate.dataset.adashiId === selection[0]);
    if (!element) return;
    const box = element.getBBox();
    const [x, y] = coordinates(event as unknown as React.PointerEvent);
    canvasRef.current?.setPointerCapture(event.pointerId);
    setDrag({ kind: "resize", startX: x, startY: y, startSnapshot: present, bbox: { x: box.x, y: box.y, width: Math.max(box.width, 1), height: Math.max(box.height, 1) } });
  }

  function onPointerMove(event: React.PointerEvent<HTMLDivElement>) {
    if (!drag) return;
    if (drag.kind === "pan" && drag.startPan && drag.startClientX !== undefined && drag.startClientY !== undefined) {
      setPan({ x: drag.startPan.x + event.clientX - drag.startClientX, y: drag.startPan.y + event.clientY - drag.startClientY });
      return;
    }
    const [x, y] = coordinates(event);
    if (drag.kind === "move") setPresent({ ...drag.startSnapshot, svg: translateElements(drag.startSnapshot.svg, selection, x - drag.startX, y - drag.startY) });
    else if (drag.kind === "resize" && drag.bbox && selection[0]) {
      const scaleX = Math.max(.08, (drag.bbox.width + x - drag.startX) / drag.bbox.width);
      const scaleY = Math.max(.08, (drag.bbox.height + y - drag.startY) / drag.bbox.height);
      setPresent({ ...drag.startSnapshot, svg: resizeElement(drag.startSnapshot.svg, selection[0], drag.bbox.x, drag.bbox.y, scaleX, scaleY) });
    } else if (drag.kind === "pencil") setDrag({ ...drag, points: [...(drag.points ?? []), [x, y]] });
    else if (drag.kind === "marquee") setDrag({ ...drag, currentX: x, currentY: y });
  }

  function onPointerUp(event: React.PointerEvent<HTMLDivElement>) {
    if (!drag) return;
    if (drag.kind === "pan") {
      setDrag(null);
      return;
    }
    const [x, y] = coordinates(event);
    if (drag.kind === "move" && (Math.abs(x - drag.startX) > .5 || Math.abs(y - drag.startY) > .5)) {
      commitFrom(drag.startSnapshot, { ...present, operations: [...drag.startSnapshot.operations, { sequence: drag.startSnapshot.operations.length, kind: "move", targetElementId: selection.length === 1 ? selection[0] : null, payloadJson: JSON.stringify({ targetIds: selection, dx: x - drag.startX, dy: y - drag.startY }) }] });
    } else if (drag.kind === "resize" && present.svg !== drag.startSnapshot.svg) {
      commitFrom(drag.startSnapshot, { ...present, operations: [...drag.startSnapshot.operations, { sequence: drag.startSnapshot.operations.length, kind: "resize", targetElementId: selection[0], payloadJson: JSON.stringify({ handle: "south-east", x, y }) }] });
    } else if (drag.kind === "pencil" && (drag.points?.length ?? 0) > 1) {
      const path = drag.points!.map(([px, py], index) => `${index ? "L" : "M"} ${Math.round(px * 10) / 10} ${Math.round(py * 10) / 10}`).join(" ");
      commitFrom(drag.startSnapshot, addAnnotation(drag.startSnapshot, path), []);
    } else if (drag.kind === "marquee") {
      const left = Math.min(event.clientX, drag.startClientX ?? event.clientX);
      const right = Math.max(event.clientX, drag.startClientX ?? event.clientX);
      const top = Math.min(event.clientY, drag.startClientY ?? event.clientY);
      const bottom = Math.max(event.clientY, drag.startClientY ?? event.clientY);
      const selected = marqueeSelection(left, top, right, bottom);
      setSelection([...new Set([...(drag.baseSelection ?? []), ...selected])]);
    } else setPresent(drag.startSnapshot);
    setDrag(null);
  }

  function add(kind: MockupPrimitive) {
    const result = addPrimitive(present, kind, 70 + (present.operations.length % 7) * 12, 70 + (present.operations.length % 5) * 12);
    commit(result.snapshot, [result.id]);
  }

  function applyProperty(name: "text" | "fill" | "stroke" | "opacity" | "x" | "y" | "width" | "height", value: string) {
    if (selection.length !== 1) return;
    if (details?.[name] === value) return;
    commit(editElement(present, selection, name === "text" ? "rename" : ["x", "y", "width", "height"].includes(name) ? "geometryChange" : "appearanceChange", { [name]: value }, (element) => {
      if (name === "text") {
        const text = element.localName === "text" ? element : element.querySelector("text");
        if (text) text.textContent = value;
        element.setAttribute("data-label", value || element.getAttribute("data-label") || selection[0]);
      } else {
        const target = element.localName === "g" ? element.querySelector(`[${name}]`) ?? element.querySelector("rect,image,text") ?? element.firstElementChild : element;
        target?.setAttribute(name, value);
      }
    }));
  }

  function keyboard(event: React.KeyboardEvent) {
    if (isTextEntry(event.target)) return;
    const command = event.ctrlKey || event.metaKey;
    const key = event.key.toLowerCase();
    if (event.key === " ") {
      event.preventDefault();
      spacePressed.current = true;
      setSpacePan(true);
      return;
    }
    if (command && key === "z") { event.preventDefault(); event.shiftKey ? redo() : undo(); }
    else if (command && key === "y") { event.preventDefault(); redo(); }
    else if (command && key === "d" && selection.length) { event.preventDefault(); const result = duplicateElements(present, selection); commit(result.snapshot, result.ids); }
    else if (command && event.shiftKey && key === "g" && selection.length) { event.preventDefault(); try { const result = ungroupElements(present, selection); commit(result.snapshot, result.ids); } catch (error) { onError(String(error)); } }
    else if (command && key === "g" && selection.length > 1) { event.preventDefault(); try { const result = groupElements(present, selection); commit(result.snapshot, [result.id]); } catch (error) { onError(String(error)); } }
    else if (event.altKey && ["PageUp", "PageDown", "Home", "End"].includes(event.key) && selection.length) { event.preventDefault(); const direction = event.key === "PageUp" ? "forward" : event.key === "PageDown" ? "backward" : event.key === "Home" ? "front" : "back"; commit(orderElements(present, selection, direction)); }
    else if ((event.key === "Delete" || event.key === "Backspace") && selection.length && (event.target as HTMLElement).tagName !== "INPUT") { event.preventDefault(); commit(deleteElements(present, selection), []); }
    else if (event.key.startsWith("Arrow") && selection.length) { event.preventDefault(); const step = event.shiftKey ? 10 : 1; const dx = event.key === "ArrowLeft" ? -step : event.key === "ArrowRight" ? step : 0; const dy = event.key === "ArrowUp" ? -step : event.key === "ArrowDown" ? step : 0; commit({ ...present, svg: translateElements(present.svg, selection, dx, dy), operations: [...present.operations, { sequence: present.operations.length, kind: "move", targetElementId: selection.length === 1 ? selection[0] : null, payloadJson: JSON.stringify({ targetIds: selection, dx, dy, keyboard: true }) }] }); }
    else if (!command && ["v", "h", "p", "e", "c"].includes(key)) setTool(key === "h" ? "pan" : key === "p" ? "pencil" : key === "e" ? "eraser" : key === "c" ? "callout" : "select");
  }

  function keyboardUp(event: React.KeyboardEvent) {
    if (event.key !== " ") return;
    spacePressed.current = false;
    setSpacePan(false);
  }

  const details = selection.length === 1 ? describeElement(present.svg, selection[0]) : null;
  const pencilPreview = drag?.kind === "pencil" && drag.points ? drag.points.map(([x, y], index) => `${index ? "L" : "M"} ${x} ${y}`).join(" ") : null;
  const marquee = drag?.kind === "marquee" ? {
    x: Math.min(drag.startX, drag.currentX ?? drag.startX),
    y: Math.min(drag.startY, drag.currentY ?? drag.startY),
    width: Math.abs((drag.currentX ?? drag.startX) - drag.startX),
    height: Math.abs((drag.currentY ?? drag.startY) - drag.startY),
  } : null;
  const stageScale = fitScale * zoom / 100;

  return (
    <div className={`mockup-editor${spacePan ? " space-pan" : ""}`} onBlur={() => { spacePressed.current = false; setSpacePan(false); }} onKeyDown={keyboard} onKeyUp={keyboardUp} tabIndex={0}>
      <div className="mockup-editor-tools" role="toolbar" aria-label="Mockup editing tools">
        {(["select", "pan", "pencil", "eraser", "callout"] as EditorTool[]).map((item) => <button aria-pressed={tool === item} className={tool === item ? "active" : ""} key={item} onClick={() => setTool(item)} title={item === "callout" ? "Place a red leader line with a short revision note. It stays separate from the accepted UI." : item === "pan" ? "Drag the canvas without changing elements. Space-drag and middle-drag work from any tool." : undefined} type="button">{item === "select" ? "Select (V)" : item === "pan" ? "Pan (H)" : item === "pencil" ? "Red pencil (P)" : item === "eraser" ? "Eraser (E)" : "Text callout (C)"}</button>)}
        <button onClick={fitCanvas} type="button">Fit</button>
        <label className="mockup-zoom-control">Zoom
          <input aria-label="Canvas zoom" max="220" min="25" onChange={(event) => setZoomAt(Number(event.currentTarget.value))} type="range" value={zoom} />
          <output>{zoom}%</output>
        </label>
        <span className="tool-separator" />
        <button disabled={!past.length} onClick={undo} type="button">Undo</button>
        <button disabled={!future.length} onClick={redo} type="button">Redo</button>
        <button disabled={!dirty || saving} onClick={() => void saveDraft()} type="button">{saving ? "Saving…" : "Save draft"}</button>
        <button disabled={saving} onClick={() => void requestRevision()} type="button">Request AI revision</button>
        <span className={dirty ? "mockup-save-state dirty" : "mockup-save-state"}>{dirty ? "Unsaved changes" : lastSaved ? `Draft saved ${lastSaved}` : "Accepted source"}</span>
      </div>
      <div className="mockup-editor-body">
        <aside className="mockup-palette" aria-label="Add mockup element">
          <strong>Add</strong>
          {(["panel", "text", "button", "field", "image", "divider"] as MockupPrimitive[]).map((item) => <button key={item} onClick={() => add(item)} type="button">{item === "image" ? "Image placeholder" : item[0].toUpperCase() + item.slice(1)}</button>)}
        </aside>
        <div
          aria-label="Layered SVG mockup canvas"
          className={`mockup-edit-canvas tool-${tool}`}
          onPointerDown={onPointerDown}
          onDoubleClick={(event) => {
            const target = logicalSelectionElement(event.target as Element, event.altKey) as SVGGraphicsElement | null;
            if (!target?.dataset.adashiId) return;
            setSelection([target.dataset.adashiId]);
            const current = describeElement(present.svg, target.dataset.adashiId);
            const value = window.prompt("Edit label", current.text || current.label);
            if (value !== null) {
              commit(editElement(present, [target.dataset.adashiId], "rename", { text: value }, (element) => {
                const text = element.localName === "text" ? element : element.querySelector("text");
                if (text) text.textContent = value;
                element.setAttribute("data-label", value || current.label);
              }), [target.dataset.adashiId]);
            }
          }}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
          onWheel={(event) => {
            event.preventDefault();
            if (event.deltaY) setZoomAt(zoom - Math.sign(event.deltaY) * 10, event.clientX, event.clientY);
          }}
          ref={canvasRef}
        >
          <div
            className="mockup-canvas-stage"
            style={{
              height: mockup.manifest.viewportHeight,
              transform: `translate(${pan.x}px, ${pan.y}px) scale(${stageScale})`,
              width: mockup.manifest.viewportWidth,
            }}
          >
            <div className="mockup-edit-source" dangerouslySetInnerHTML={{ __html: present.svg }} />
            <svg className="mockup-annotation-layer" preserveAspectRatio="none" viewBox={`0 0 ${mockup.manifest.viewportWidth} ${mockup.manifest.viewportHeight}`}>
              {present.annotations.map((annotation) => <g data-annotation-id={annotation.externalId} key={annotation.externalId}><path d={annotation.svgPath} fill="none" pointerEvents="stroke" stroke="#e03131" strokeLinecap="round" strokeLinejoin="round" strokeWidth="4" vectorEffect="non-scaling-stroke" />{annotation.optionalText ? <text fill="#e03131" fontSize="16" fontWeight="700" x={annotationPoint(annotation.svgPath)[0] + 32} y={annotationPoint(annotation.svgPath)[1] - 30}>{annotation.optionalText}</text> : null}</g>)}
              {pencilPreview ? <path d={pencilPreview} fill="none" stroke="#e03131" strokeLinecap="round" strokeLinejoin="round" strokeWidth="4" vectorEffect="non-scaling-stroke" /> : null}
              {marquee ? <rect className="mockup-marquee" fill="rgba(38, 132, 255, 0.13)" height={marquee.height} pointerEvents="none" stroke="#2684ff" strokeDasharray="5 4" strokeWidth="1.5" vectorEffect="non-scaling-stroke" width={marquee.width} x={marquee.x} y={marquee.y} /> : null}
            </svg>
          </div>
          {resizeHandle ? <button aria-label="Resize selected element" className="mockup-resize-handle" onPointerDown={beginResize} style={resizeHandle} type="button" /> : null}
        </div>
        <aside className="mockup-properties">
          <strong>{selection.length ? `${selection.length} selected` : "Properties"}</strong>
          {details ? <>
            <label>Label<input defaultValue={details.text || details.label} key={`${selection[0]}-text-${details.text}`} onBlur={(event) => applyProperty("text", event.currentTarget.value)} /></label>
            <ColorProperty label="Fill" onChange={(value) => applyProperty("fill", value)} value={details.fill} />
            <ColorProperty label="Stroke" onChange={(value) => applyProperty("stroke", value)} value={details.stroke} />
            <label>Opacity<input defaultValue={details.opacity} key={`${selection[0]}-opacity-${details.opacity}`} max="1" min="0" onBlur={(event) => applyProperty("opacity", event.currentTarget.value)} step="0.1" type="number" /></label>
            <div className="mockup-geometry-fields">
              {(["x", "y", "width", "height"] as const).map((name) => <label key={`${selection[0]}-${name}-${details[name]}`}>{name.toUpperCase()}<input defaultValue={details[name]} min={name === "width" || name === "height" ? "1" : undefined} onBlur={(event) => applyProperty(name, event.currentTarget.value)} type="number" /></label>)}
            </div>
          </> : <p>Click selects a whole logical group. Drag on empty canvas for rectangular multi-selection; Shift-drag adds. Alt-click selects an individual child shape.</p>}
          {tool === "pan" ? <p className="mockup-tool-explanation"><strong>Pan:</strong> drag to move the canvas. You can also Space-drag or middle-drag from any tool, and use the mouse wheel to zoom around the pointer.</p> : null}
          {tool === "callout" ? <p className="mockup-tool-explanation"><strong>Text callout:</strong> click the canvas to place a red leader line and short revision note. It remains annotation evidence and never becomes accepted UI.</p> : null}
          <div className="mockup-structural-actions">
            <button disabled={!selection.length} onClick={() => { const result = duplicateElements(present, selection); commit(result.snapshot, result.ids); }} type="button">Duplicate</button>
            <button disabled={!selection.length} onClick={() => commit(deleteElements(present, selection), [])} type="button">Delete</button>
            <button disabled={selection.length < 2} onClick={() => { try { const result = groupElements(present, selection); commit(result.snapshot, [result.id]); } catch (error) { onError(String(error)); } }} type="button">Group</button>
            <button disabled={!selection.length} onClick={() => { try { const result = ungroupElements(present, selection); commit(result.snapshot, result.ids); } catch (error) { onError(String(error)); } }} type="button">Ungroup</button>
            <button disabled={!selection.length} onClick={() => commit(orderElements(present, selection, "forward"))} type="button">Forward</button>
            <button disabled={!selection.length} onClick={() => commit(orderElements(present, selection, "backward"))} type="button">Backward</button>
            <button disabled={!selection.length} onClick={() => commit(orderElements(present, selection, "front"))} type="button">To front</button>
            <button disabled={!selection.length} onClick={() => commit(orderElements(present, selection, "back"))} type="button">To back</button>
          </div>
        </aside>
      </div>
    </div>
  );
}

function annotationPoint(path: string): [number, number] {
  const numbers = path.match(/-?\d+(?:\.\d+)?/g)?.map(Number) ?? [0, 0];
  return [numbers[0] ?? 0, numbers[1] ?? 0];
}

function isTextEntry(target: EventTarget | null): boolean {
  const element = target as HTMLElement | null;
  return Boolean(element?.closest("input, textarea, select, [contenteditable='true']"));
}

function ColorProperty({ label, onChange, value }: { label: string; onChange: (value: string) => void; value: string }) {
  const color = /^#[0-9a-f]{6}$/i.test(value) ? value : "#000000";
  const [open, setOpen] = React.useState(false);
  const [draft, setDraft] = React.useState(color);
  const rootRef = React.useRef<HTMLSpanElement>(null);
  const hsl = hexToHsl(draft);
  React.useEffect(() => { setDraft(color); }, [color]);
  React.useEffect(() => {
    if (!open) return;
    const closeOutside = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    document.addEventListener("pointerdown", closeOutside, true);
    document.addEventListener("keydown", closeOnEscape, true);
    return () => {
      document.removeEventListener("pointerdown", closeOutside, true);
      document.removeEventListener("keydown", closeOnEscape, true);
    };
  }, [open]);

  function preview(next: string) {
    setDraft(next);
    onChange(next);
  }

  function previewHsl(hue: number, saturation: number, lightness: number) {
    preview(hslToHex(hue, saturation, lightness));
  }

  return <span className="mockup-property-label">
    <span>{label}</span>
    <span className="mockup-color-field" ref={rootRef}>
      <button
        aria-expanded={open}
        aria-label={`${label} color picker`}
        className="mockup-color-trigger"
        onClick={() => setOpen((current) => !current)}
        style={{ backgroundColor: draft }}
        type="button"
      />
      <input aria-label={`${label} color value`} defaultValue={value} key={`${label}-value-${value}`} onBlur={(event) => onChange(event.currentTarget.value)} placeholder="#ffffff or none" />
      {open ? <span aria-label={`${label} color controls`} className="mockup-color-popover" role="dialog">
        <span className="mockup-color-preview" style={{ backgroundColor: draft }} />
        <label>Hue
          <input aria-label={`${label} hue`} className="mockup-hue-range" max="360" min="0" onChange={(event) => previewHsl(Number(event.currentTarget.value), hsl.s, hsl.l)} type="range" value={Math.round(hsl.h)} />
        </label>
        <label>Saturation
          <input aria-label={`${label} saturation`} max="100" min="0" onChange={(event) => previewHsl(hsl.h, Number(event.currentTarget.value), hsl.l)} style={{ background: `linear-gradient(90deg, hsl(${hsl.h} 0% ${hsl.l}%), hsl(${hsl.h} 100% ${hsl.l}%))` }} type="range" value={Math.round(hsl.s)} />
        </label>
        <label>Lightness
          <input aria-label={`${label} lightness`} max="100" min="0" onChange={(event) => previewHsl(hsl.h, hsl.s, Number(event.currentTarget.value))} style={{ background: `linear-gradient(90deg, #000, hsl(${hsl.h} ${hsl.s}% 50%), #fff)` }} type="range" value={Math.round(hsl.l)} />
        </label>
        <span className="mockup-color-presets" aria-label="Color presets">
          {["#ffffff", "#202c2b", "#315f5b", "#2684ff", "#e4b363", "#e03131"].map((preset) => <button aria-label={`Use ${preset}`} key={preset} onClick={() => preview(preset)} style={{ backgroundColor: preset }} type="button" />)}
        </span>
        <span className="mockup-color-popover-footer">
          <input
            aria-label={`${label} live color value`}
            onChange={(event) => {
              const next = event.currentTarget.value;
              setDraft(next);
              if (/^#[0-9a-f]{6}$/i.test(next)) onChange(next);
            }}
            value={draft}
          />
          <button onClick={() => setOpen(false)} type="button">OK</button>
        </span>
      </span> : null}
    </span>
  </span>;
}

function hexToHsl(hex: string): { h: number; s: number; l: number } {
  const valid = /^#[0-9a-f]{6}$/i.test(hex) ? hex : "#000000";
  const red = parseInt(valid.slice(1, 3), 16) / 255;
  const green = parseInt(valid.slice(3, 5), 16) / 255;
  const blue = parseInt(valid.slice(5, 7), 16) / 255;
  const max = Math.max(red, green, blue);
  const min = Math.min(red, green, blue);
  const delta = max - min;
  const lightness = (max + min) / 2;
  if (!delta) return { h: 0, s: 0, l: lightness * 100 };
  const saturation = delta / (1 - Math.abs(2 * lightness - 1));
  const hue = max === red ? 60 * (((green - blue) / delta) % 6) : max === green ? 60 * ((blue - red) / delta + 2) : 60 * ((red - green) / delta + 4);
  return { h: hue < 0 ? hue + 360 : hue, s: saturation * 100, l: lightness * 100 };
}

function hslToHex(hue: number, saturation: number, lightness: number): string {
  const s = saturation / 100;
  const l = lightness / 100;
  const chroma = (1 - Math.abs(2 * l - 1)) * s;
  const section = ((hue % 360) + 360) % 360 / 60;
  const x = chroma * (1 - Math.abs(section % 2 - 1));
  const [red, green, blue] = section < 1 ? [chroma, x, 0] : section < 2 ? [x, chroma, 0] : section < 3 ? [0, chroma, x] : section < 4 ? [0, x, chroma] : section < 5 ? [x, 0, chroma] : [chroma, 0, x];
  const match = l - chroma / 2;
  return `#${[red, green, blue].map((channel) => Math.round((channel + match) * 255).toString(16).padStart(2, "0")).join("")}`;
}
