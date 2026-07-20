export type MockupEditOperation = {
  sequence: number;
  kind: string;
  targetElementId?: string | null;
  payloadJson: string;
};

export type MockupAnnotation = {
  externalId: string;
  svgPath: string;
  optionalText: string;
  sortOrder: number;
};

export type MockupEditorSnapshot = {
  svg: string;
  annotations: MockupAnnotation[];
  operations: MockupEditOperation[];
};

const SVG_NS = "http://www.w3.org/2000/svg";
const LOGICAL_GROUP_ROLES = new Set([
  "button",
  "command",
  "field",
  "filter-group",
  "form",
  "group",
  "header",
  "image-placeholder",
  "list-item",
  "navigation",
  "panel",
  "section",
  "selected-list-item",
]);
const PROTECTED_BACK_ROLES = new Set(["background", "panel-surface"]);

function parseSvg(source: string): XMLDocument {
  const document = new DOMParser().parseFromString(source, "image/svg+xml");
  if (document.querySelector("parsererror") || document.documentElement.localName !== "svg") {
    throw new Error("The working mockup is not valid SVG.");
  }
  return document;
}

function serialize(document: XMLDocument): string {
  return new XMLSerializer().serializeToString(document.documentElement);
}

function identified(document: XMLDocument, id: string): Element {
  const element = Array.from(document.querySelectorAll("[data-adashi-id]")).find(
    (candidate) => candidate.getAttribute("data-adashi-id") === id,
  );
  if (!element) throw new Error(`Unknown mockup element '${id}'.`);
  return element;
}

function uniqueId(document: XMLDocument, prefix: string): string {
  const safePrefix = prefix.replace(/[^A-Za-z0-9_-]+/g, "-").replace(/^-+|-+$/g, "") || "element";
  const used = new Set(Array.from(document.querySelectorAll("[data-adashi-id]"), (element) => element.getAttribute("data-adashi-id") ?? ""));
  let suffix = Date.now().toString(36);
  let candidate = `${safePrefix}-${suffix}`;
  let index = 2;
  while (used.has(candidate)) candidate = `${safePrefix}-${suffix}-${index++}`;
  return candidate;
}

function operation(operations: MockupEditOperation[], kind: string, targetElementId: string | null, payload: object): MockupEditOperation[] {
  return [...operations, { sequence: operations.length, kind, targetElementId, payloadJson: JSON.stringify(payload) }];
}

export function mutateElements(
  source: string,
  ids: string[],
  mutate: (element: Element, document: XMLDocument) => void,
): string {
  const document = parseSvg(source);
  ids.forEach((id) => mutate(identified(document, id), document));
  return serialize(document);
}

export function translateElements(source: string, ids: string[], dx: number, dy: number): string {
  return mutateElements(source, ids, (element) => {
    const current = element.getAttribute("transform")?.trim();
    element.setAttribute("transform", `translate(${round(dx)} ${round(dy)})${current ? ` ${current}` : ""}`);
  });
}

export function resizeElement(source: string, id: string, originX: number, originY: number, scaleX: number, scaleY: number): string {
  return mutateElements(source, [id], (element) => {
    const current = element.getAttribute("transform")?.trim();
    element.setAttribute(
      "transform",
      `${current ? `${current} ` : ""}translate(${round(originX)} ${round(originY)}) scale(${round(scaleX)} ${round(scaleY)}) translate(${round(-originX)} ${round(-originY)})`,
    );
  });
}

export function editElement(
  snapshot: MockupEditorSnapshot,
  ids: string[],
  kind: string,
  payload: Record<string, unknown>,
  mutate: (element: Element) => void,
): MockupEditorSnapshot {
  return {
    ...snapshot,
    svg: mutateElements(snapshot.svg, ids, mutate),
    operations: operation(snapshot.operations, kind, ids.length === 1 ? ids[0] : null, { targetIds: ids, ...payload }),
  };
}

export type MockupPrimitive = "panel" | "text" | "button" | "field" | "image" | "divider";

export function addPrimitive(snapshot: MockupEditorSnapshot, primitive: MockupPrimitive, x = 80, y = 80): { snapshot: MockupEditorSnapshot; id: string } {
  const document = parseSvg(snapshot.svg);
  const id = uniqueId(document, primitive);
  const group = document.createElementNS(SVG_NS, "g");
  group.setAttribute("data-adashi-id", id);
  group.setAttribute("data-role", primitive === "image" ? "image-placeholder" : primitive);
  group.setAttribute("data-label", defaultLabel(primitive));
  group.setAttribute("data-adashi-editor-object", "true");

  const append = (name: string, attributes: Record<string, string>, text?: string) => {
    const element = document.createElementNS(SVG_NS, name);
    Object.entries(attributes).forEach(([key, value]) => element.setAttribute(key, value));
    element.setAttribute("data-adashi-id", uniqueId(document, `${id}-${name}`));
    element.setAttribute("data-role", name);
    element.setAttribute("data-label", text ?? `${defaultLabel(primitive)} ${name}`);
    if (text !== undefined) element.textContent = text;
    group.appendChild(element);
  };
  if (primitive === "text") {
    append("text", { x: String(x), y: String(y + 22), fill: "#17202a", "font-size": "18" }, "Text");
  } else if (primitive === "divider") {
    append("line", { x1: String(x), y1: String(y), x2: String(x + 180), y2: String(y), stroke: "#a8b0b8", "stroke-width": "2" });
  } else {
    const width = primitive === "panel" ? 260 : primitive === "field" ? 220 : 150;
    const height = primitive === "panel" ? 160 : primitive === "image" ? 110 : 44;
    append("rect", { x: String(x), y: String(y), width: String(width), height: String(height), rx: primitive === "panel" ? "10" : "7", fill: primitive === "button" ? "#315f5b" : primitive === "image" ? "#edf0ef" : "#ffffff", stroke: "#8e9997", "stroke-width": "1.5" });
    if (primitive === "image") {
      append("path", { d: `M ${x + 18} ${y + height - 18} L ${x + width * .43} ${y + height * .48} L ${x + width * .61} ${y + height * .67} L ${x + width - 18} ${y + 24}`, fill: "none", stroke: "#87918f", "stroke-width": "2" });
    } else if (primitive !== "panel") {
      append("text", { x: String(x + 14), y: String(y + 28), fill: primitive === "button" ? "#ffffff" : "#35413f", "font-size": "15" }, defaultLabel(primitive));
    }
  }
  document.documentElement.appendChild(group);
  return {
    id,
    snapshot: {
      ...snapshot,
      svg: serialize(document),
      operations: operation(snapshot.operations, "add", id, { primitive, x, y }),
    },
  };
}

export function deleteElements(snapshot: MockupEditorSnapshot, ids: string[]): MockupEditorSnapshot {
  const document = parseSvg(snapshot.svg);
  ids.forEach((id) => identified(document, id).remove());
  return { ...snapshot, svg: serialize(document), operations: operation(snapshot.operations, "delete", null, { targetIds: ids }) };
}

export function duplicateElements(snapshot: MockupEditorSnapshot, ids: string[]): { snapshot: MockupEditorSnapshot; ids: string[] } {
  const document = parseSvg(snapshot.svg);
  const duplicatedIds: string[] = [];
  ids.forEach((id) => {
    const element = identified(document, id);
    const clone = element.cloneNode(true) as Element;
    const duplicateId = uniqueId(document, `${id}-copy`);
    clone.setAttribute("data-adashi-id", duplicateId);
    clone.setAttribute("data-label", `${element.getAttribute("data-label") ?? id} copy`);
    Array.from(clone.querySelectorAll("[data-adashi-id]")).forEach((child) => {
      const sourceId = child.getAttribute("data-adashi-id") ?? child.localName;
      child.setAttribute("data-adashi-id", uniqueId(document, `${sourceId}-copy`));
      child.setAttribute("data-label", `${child.getAttribute("data-label") ?? sourceId} copy`);
    });
    clone.setAttribute("transform", `translate(16 16)${clone.getAttribute("transform") ? ` ${clone.getAttribute("transform")}` : ""}`);
    element.parentNode?.insertBefore(clone, element.nextSibling);
    duplicatedIds.push(duplicateId);
  });
  return {
    ids: duplicatedIds,
    snapshot: { ...snapshot, svg: serialize(document), operations: operation(snapshot.operations, "duplicate", null, { sourceIds: ids, duplicateIds: duplicatedIds }) },
  };
}

export function groupElements(snapshot: MockupEditorSnapshot, ids: string[]): { snapshot: MockupEditorSnapshot; id: string } {
  if (ids.length < 2) throw new Error("Select at least two elements to group.");
  const document = parseSvg(snapshot.svg);
  const elements = ids.map((id) => identified(document, id));
  const parent = elements[0].parentNode;
  if (!parent || elements.some((element) => element.parentNode !== parent)) throw new Error("Grouped elements must share the same layer.");
  const id = uniqueId(document, "group");
  const group = document.createElementNS(SVG_NS, "g");
  group.setAttribute("data-adashi-id", id);
  group.setAttribute("data-role", "group");
  group.setAttribute("data-label", "Group");
  group.setAttribute("data-adashi-editor-object", "true");
  parent.insertBefore(group, elements[0]);
  elements.forEach((element) => group.appendChild(element));
  return { id, snapshot: { ...snapshot, svg: serialize(document), operations: operation(snapshot.operations, "group", id, { childIds: ids }) } };
}

export function ungroupElements(snapshot: MockupEditorSnapshot, ids: string[]): { snapshot: MockupEditorSnapshot; ids: string[] } {
  const document = parseSvg(snapshot.svg);
  const released: string[] = [];
  ids.forEach((id) => {
    const group = identified(document, id);
    if (group.localName !== "g") throw new Error("Only SVG groups can be ungrouped.");
    const parent = group.parentNode;
    if (!parent) return;
    Array.from(group.children).forEach((child) => {
      if (!child.getAttribute("data-adashi-id")) child.setAttribute("data-adashi-id", uniqueId(document, child.localName));
      released.push(child.getAttribute("data-adashi-id")!);
      parent.insertBefore(child, group);
    });
    group.remove();
  });
  return { ids: released, snapshot: { ...snapshot, svg: serialize(document), operations: operation(snapshot.operations, "ungroup", null, { groupIds: ids, releasedIds: released }) } };
}

export type OrderDirection = "forward" | "backward" | "front" | "back";

export function orderElements(snapshot: MockupEditorSnapshot, ids: string[], direction: OrderDirection): MockupEditorSnapshot {
  const document = parseSvg(snapshot.svg);
  ids.forEach((id) => {
    const element = identified(document, id);
    const parent = element.parentNode;
    if (!parent) return;
    if (direction === "front") parent.appendChild(element);
    else if (direction === "back") {
      const siblings = Array.from(parent.children).filter((candidate) => candidate !== element);
      const firstMovable = siblings.find((candidate) => !isProtectedBackLayer(candidate)) ?? null;
      parent.insertBefore(element, firstMovable);
    }
    else if (direction === "forward" && element.nextSibling) parent.insertBefore(element.nextSibling, element);
    else if (direction === "backward" && element.previousElementSibling && !isProtectedBackLayer(element.previousElementSibling)) parent.insertBefore(element, element.previousElementSibling);
  });
  return { ...snapshot, svg: serialize(document), operations: operation(snapshot.operations, "ordering", null, { targetIds: ids, direction }) };
}

export function logicalSelectionElement(target: Element, preferLeaf = false): Element | null {
  const identifiedTarget = target.closest("[data-adashi-id]");
  if (!identifiedTarget) return null;
  if (preferLeaf) return identifiedTarget;
  const targetRole = identifiedTarget.getAttribute("data-role") ?? "";
  if (identifiedTarget.localName === "g" && targetRole === "group") return identifiedTarget;

  let ancestor = identifiedTarget.parentElement?.closest("g[data-adashi-id]") ?? null;
  let logicalAncestor: Element | null = identifiedTarget.localName === "g" &&
    (identifiedTarget.getAttribute("data-adashi-editor-object") === "true" || LOGICAL_GROUP_ROLES.has(targetRole))
    ? identifiedTarget
    : null;
  while (ancestor) {
    const role = ancestor.getAttribute("data-role") ?? "";
    if (role === "group") return ancestor;
    if (!logicalAncestor && (ancestor.getAttribute("data-adashi-editor-object") === "true" || LOGICAL_GROUP_ROLES.has(role))) logicalAncestor = ancestor;
    ancestor = ancestor.parentElement?.closest("g[data-adashi-id]") ?? null;
  }
  return logicalAncestor ?? identifiedTarget;
}

export function addAnnotation(snapshot: MockupEditorSnapshot, svgPath: string, optionalText = ""): MockupEditorSnapshot {
  const externalId = `annotation-${Date.now().toString(36)}`;
  const annotations = [...snapshot.annotations, { externalId, svgPath, optionalText: optionalText.slice(0, 120), sortOrder: snapshot.annotations.length }];
  return { ...snapshot, annotations, operations: operation(snapshot.operations, optionalText ? "annotationCallout" : "annotationDraw", externalId, { svgPath, optionalText }) };
}

export function eraseAnnotation(snapshot: MockupEditorSnapshot, externalId: string): MockupEditorSnapshot {
  const annotations = snapshot.annotations.filter((annotation) => annotation.externalId !== externalId).map((annotation, sortOrder) => ({ ...annotation, sortOrder }));
  return { ...snapshot, annotations, operations: operation(snapshot.operations, "annotationErase", externalId, {}) };
}

export function describeElement(source: string, id: string): Record<string, string> {
  const element = identified(parseSvg(source), id);
  const text = element.localName === "text" ? element.textContent ?? "" : element.querySelector("text")?.textContent ?? "";
  const geometry = element.matches("rect,image,text") ? element : element.querySelector("rect,image,text");
  return {
    label: element.getAttribute("data-label") ?? id,
    role: element.getAttribute("data-role") ?? element.localName,
    text,
    fill: element.getAttribute("fill") ?? element.querySelector("[fill]")?.getAttribute("fill") ?? "",
    stroke: element.getAttribute("stroke") ?? element.querySelector("[stroke]")?.getAttribute("stroke") ?? "",
    opacity: element.getAttribute("opacity") ?? "1",
    x: geometry?.getAttribute("x") ?? "",
    y: geometry?.getAttribute("y") ?? "",
    width: geometry?.getAttribute("width") ?? "",
    height: geometry?.getAttribute("height") ?? "",
  };
}

function defaultLabel(primitive: MockupPrimitive): string {
  return primitive === "image" ? "Image" : primitive.charAt(0).toUpperCase() + primitive.slice(1);
}

function isProtectedBackLayer(element: Element): boolean {
  const role = element.getAttribute("data-role") ?? "";
  return PROTECTED_BACK_ROLES.has(role) || role.endsWith("-surface") && element.parentElement?.localName === "svg";
}

function round(value: number): number {
  return Math.round(value * 1000) / 1000;
}
