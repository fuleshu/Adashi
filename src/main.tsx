import React from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import EasyMDE from "easymde";
import {
  Activity,
  ArrowDown,
  ArrowUp,
  Bot,
  Braces,
  Check,
  CheckCircle2,
  ChevronRight,
  Database,
  ExternalLink,
  Folder,
  GitBranch,
  Link2,
  ListChecks,
  Network,
  PlayCircle,
  Plus,
  Search,
  ScrollText,
  Settings,
  ServerCog,
  Trash2,
  Wand2,
  X,
  ZoomIn,
  ZoomOut,
} from "lucide-react";
import mermaid from "mermaid";
import "easymde/dist/easymde.min.css";
import "./styles.css";

type DiagramKind = "mermaid" | "structurizr";
type DesignLevel = "context" | "components" | "features";
type DesignEntityType = "element" | "relationship";
type DesignProjectionMode = "branch" | "dependencies";

type DesignDiagram = {
  id: number;
  kind: DiagramKind;
  key: string;
  title: string;
  source: string;
  diagramType: string;
  artifactRole: string;
  artifactLabel: string;
  artifactRank: number;
  attachedToExternalId?: string | null;
  attachedToTargetType?: DesignEntityType | null;
  sortOrder: number;
};

type UmlArtifactType = {
  diagramType: string;
  artifactRole: string;
  artifactLabel: string;
  artifactRank: number;
  mermaidHeader: string;
  description: string;
};

type DesignElement = {
  id: number;
  externalId: string;
  parentExternalId?: string | null;
  elementType: string;
  name: string;
  description: string;
  technology: string;
  tags: string;
};

type DesignRelationship = {
  id: number;
  externalId: string;
  sourceExternalId: string;
  destinationExternalId: string;
  description: string;
  technology: string;
  tags: string;
};

type StructurizrRelationshipJson = {
  id: string;
  sourceId: string;
  destinationId: string;
  description: string;
  technology: string;
  tags: string;
};

type StructurizrElementJson = {
  id: string;
  canonicalName: string;
  description: string;
  location: "Internal" | "External";
  name: string;
  parentId: string | null;
  relationships: StructurizrRelationshipJson[];
  tags: string;
  technology: string;
  type: string;
  containers?: StructurizrElementJson[];
  components?: StructurizrElementJson[];
};

type StructurizrViewElementJson = {
  id: string;
  relationships: string[];
  x: number;
  y: number;
  width: number;
  height: number;
};

type StructurizrWorkspaceJson = {
  id: number;
  name: string;
  description: string;
  model: {
    people: StructurizrElementJson[];
    softwareSystems: StructurizrElementJson[];
  };
  views: {
    systemContextViews?: StructurizrViewJson[];
    containerViews?: StructurizrViewJson[];
    componentViews?: StructurizrViewJson[];
    configuration: {
      defaultView: string;
      styles: {
        elements: Array<Record<string, string | number>>;
        relationships: Array<Record<string, string | number>>;
      };
    };
  };
};

type StructurizrViewJson = {
  key: string;
  description: string;
  softwareSystemId?: string;
  containerId?: string;
  elements: StructurizrViewElementJson[];
  relationships: Array<{ id: string }>;
  automaticLayout?: {
    implementation: "Dagre";
    rankDirection: "LeftRight";
    rankSeparation: number;
    nodeSeparation: number;
    edgeSeparation: number;
    vertices: boolean;
  };
};

type StructurizrProjection = {
  workspace: string;
  viewKey: string;
  source: string;
  visibleElementIds: Set<string>;
  visibleRelationshipIds: Set<string>;
  hiddenRelationshipCount: number;
};

type Task = {
  id: number;
  number: number;
  title: string;
  description: string;
  state: TaskState;
  designSpecificationLinks: TaskDesignSpecificationLink[];
  createdAt: string;
  updatedAt: string;
  completedAt?: string | null;
  confirmedAt?: string | null;
  completionMemo: string;
  createdFiles: string[];
  changedFiles: string[];
  confirmationCommitId?: string | null;
};

type TaskState = "open" | "finished" | "confirmed";

type TaskDesignSpecificationLink = {
  id: number;
  taskId: number;
  sortOrder: number;
  targetType: "element" | "relationship" | "uml";
  designExternalId: string;
  title: string;
};

type Guideline = {
  id: number;
  title: string;
  body: string;
};

type Command = {
  id: number;
  label: string;
  command: string;
  trigger: string;
};

type QaCheck = {
  id: number;
  label: string;
  command: string;
  required: boolean;
};

type QaDerivedState = "green" | "red" | "needs-rerun" | "running";

type QaJobDesignLink = {
  id: number;
  qaJobId: number;
  sortOrder: number;
  targetType: "element" | "relationship" | "uml";
  designExternalId: string;
  title: string;
};

type QaJobTaskLink = {
  id: number;
  qaJobId: number;
  taskId: number;
  sortOrder: number;
  title: string;
  number: number;
  state: TaskState;
};

type QaJobRun = {
  id: number;
  qaRunId: number;
  qaJobId: number;
  commandSnapshot: string;
  status: "running" | "passed" | "failed" | "timed_out";
  exitCode?: number | null;
  startedAt: string;
  finishedAt?: string | null;
  durationMs?: number | null;
  output: string;
};

type QaRun = {
  id: number;
  triggerSource: string;
  querySnapshot: string;
  status: "running" | "passed" | "failed";
  startedAt: string;
  finishedAt?: string | null;
  summary: string;
  jobRuns: QaJobRun[];
};

type QaJob = {
  id: number;
  number: number;
  name: string;
  description: string;
  command: string;
  workingDirectory: string;
  shell: string;
  timeoutSeconds: number;
  enabled: boolean;
  createdBy: string;
  createdAt: string;
  updatedAt: string;
  derivedState: QaDerivedState;
  designSpecificationLinks: QaJobDesignLink[];
  taskLinks: QaJobTaskLink[];
  tags: string[];
  latestRun?: QaJobRun | null;
  runHistory: QaJobRun[];
};

type QaJobQuery = {
  jobIds?: number[];
  states?: QaDerivedState[];
  tags?: string[];
  taskIds?: number[];
  designExternalIds?: string[];
  enabled?: boolean;
};

type RuleIntend = "general" | "design" | "implementation";
type RuleHook = "run.start" | "task.start" | "task.end" | "run.end";

type Rule = {
  id: number;
  name: string;
  enabled: boolean;
  intend: RuleIntend;
  hook: RuleHook;
  prompt: string;
};

type RuleTemplate = {
  id: string;
  name: string;
  enabled: boolean;
  intend: RuleIntend;
  hook: RuleHook;
  prompt: string;
  createdAt: string;
  updatedAt: string;
};

type FixedHookPrompt = {
  key: string;
  title: string;
  intend: RuleIntend;
  hook: RuleHook;
  prompt: string;
  updatedAt: string;
};

type ProjectMemory = {
  rule: string;
  memory: string;
  updatedAt: string;
};

type ProjectSettings = {
  id: string;
  name: string;
  folder: string;
};

type AppSettings = {
  window: {
    width: number;
    height: number;
    x?: number | null;
    y?: number | null;
  };
  projects: ProjectSettings[];
  lastActiveProjectId?: string | null;
  ruleTemplates: RuleTemplate[];
};

type DashboardPayload = {
  projectId: string;
  projectName: string;
  projectFolder: string;
  revision: number;
  workspaceName: string;
  workspaceDescription: string;
  structurizrDsl: string;
  structurizrWorkspace: string;
  structurizrViewKey: string;
  designElements: DesignElement[];
  designRelationships: DesignRelationship[];
  umlArtifactTypes: UmlArtifactType[];
  diagrams: DesignDiagram[];
  tasks: Task[];
  guidelines: Guideline[];
  postTaskCommands: Command[];
  qaChecks: QaCheck[];
  qaJobs: QaJob[];
  qaRuns: QaRun[];
  rules: Rule[];
  ruleTemplates: RuleTemplate[];
  fixedHookPrompts: FixedHookPrompt[];
  memory: ProjectMemory;
};

type ProjectRevision = {
  projectId: string;
  revision: number;
  updatedAt: string;
};

mermaid.initialize({
  startOnLoad: false,
  securityLevel: "strict",
  theme: "base",
  themeVariables: {
    primaryColor: "#f7f7f2",
    primaryTextColor: "#1f2933",
    primaryBorderColor: "#2f6f6d",
    lineColor: "#47615f",
    secondaryColor: "#dce7e3",
    tertiaryColor: "#fffaf0",
    fontFamily: "Inter, Segoe UI, sans-serif",
  },
});

function App() {
  const [settings, setSettings] = React.useState<AppSettings | null>(null);
  const [payload, setPayload] = React.useState<DashboardPayload | null>(null);
  const [activeDesignLevel, setActiveDesignLevel] = React.useState<DesignLevel>("components");
  const [selectedDesignEntity, setSelectedDesignEntity] = React.useState<{
    type: DesignEntityType;
    externalId: string;
  } | null>(null);
  const [activeView, setActiveView] = React.useState<"design" | "tasks" | "rules" | "memory" | "qa" | "settings">("design");
  const [error, setError] = React.useState<string | null>(null);
  const payloadRef = React.useRef<DashboardPayload | null>(null);
  const refreshInFlightRef = React.useRef(false);

  React.useEffect(() => {
    payloadRef.current = payload;
  }, [payload]);

  const loadDashboard = React.useCallback((projectId?: string | null, mode: "replace" | "merge" = "replace") => {
    if (mode === "replace") {
      setPayload(null);
    }

    return invoke<DashboardPayload>("get_dashboard", { projectId })
      .then((loadedPayload) => {
        setPayload((currentPayload) =>
          mode === "merge" && currentPayload?.projectId === loadedPayload.projectId
            ? mergeDashboardPayload(currentPayload, loadedPayload)
            : loadedPayload,
        );
      })
      .catch((reason) => setError(String(reason)));
  }, []);

  React.useEffect(() => {
    invoke<AppSettings>("get_app_settings")
      .then((loadedSettings) => {
        setSettings(loadedSettings);
        loadDashboard(loadedSettings.lastActiveProjectId);
      })
      .catch((reason) => setError(String(reason)));
  }, [loadDashboard]);

  React.useEffect(() => {
    const interval = window.setInterval(() => {
      const currentPayload = payloadRef.current;

      if (!currentPayload || refreshInFlightRef.current) {
        return;
      }

      invoke<ProjectRevision>("get_project_revision", { projectId: currentPayload.projectId })
        .then((revision) => {
          const latestPayload = payloadRef.current;

          if (
            !latestPayload ||
            latestPayload.projectId !== revision.projectId ||
            revision.revision <= latestPayload.revision ||
            refreshInFlightRef.current
          ) {
            return;
          }

          refreshInFlightRef.current = true;
          loadDashboard(latestPayload.projectId, "merge").finally(() => {
            refreshInFlightRef.current = false;
          });
        })
        .catch((reason) => setError(String(reason)));
    }, 1500);

    return () => window.clearInterval(interval);
  }, [loadDashboard]);

  const switchProject = React.useCallback(
    (projectId: string) => {
      invoke<AppSettings>("set_active_project", { projectId })
        .then((updatedSettings) => {
          setSettings(updatedSettings);
          loadDashboard(projectId);
          setActiveView("design");
        })
        .catch((reason) => setError(String(reason)));
    },
    [loadDashboard],
  );

  if (error) {
    return (
      <main className="shell error-shell">
        <h1>Adashi</h1>
        <p>{error}</p>
      </main>
    );
  }

  if (!settings || !payload) {
    return (
      <main className="shell loading-shell">
        <Activity className="spin" />
        <span>Loading Adashi workspace</span>
      </main>
    );
  }

  return (
    <main className="shell">
      <aside className="sidebar">
        <div className="brand-block">
          <div className="brand-mark">
            <Bot size={24} />
          </div>
          <div>
            <h1>Adashi</h1>
            <p>Senior agent dashboard</p>
          </div>
        </div>

        <label className="project-switcher">
          <span>Project</span>
          <select value={payload.projectId} onChange={(event) => switchProject(event.target.value)}>
            {settings.projects.map((project) => (
              <option key={project.id} value={project.id}>
                {project.name}
              </option>
            ))}
          </select>
        </label>

        <nav className="nav-list" aria-label="Workspace sections">
          <button className={activeView === "design" ? "active" : ""} onClick={() => setActiveView("design")} type="button">
            <Network size={18} />
            Design
          </button>
          <button className={activeView === "tasks" ? "active" : ""} onClick={() => setActiveView("tasks")} type="button">
            <ListChecks size={18} />
            Tasks
          </button>
          <button className={activeView === "rules" ? "active" : ""} onClick={() => setActiveView("rules")} type="button">
            <ScrollText size={18} />
            Rules
          </button>
          <button className={activeView === "memory" ? "active" : ""} onClick={() => setActiveView("memory")} type="button">
            <Database size={18} />
            Memory
          </button>
          <button className={activeView === "settings" ? "active" : ""} onClick={() => setActiveView("settings")} type="button">
            <Settings size={18} />
            Settings
          </button>
          <button className={activeView === "qa" ? "active" : ""} onClick={() => setActiveView("qa")} type="button">
            <CheckCircle2 size={18} />
            QA
          </button>
        </nav>

        <section className="side-panel">
          <h2>MCP Surface</h2>
          <div className="metric">
            <ServerCog size={18} />
            <span>MCP calls carry project id/name context</span>
          </div>
          <div className="metric">
            <Database size={18} />
            <span>SQL data lives in project .adashi folders</span>
          </div>
        </section>
      </aside>

      <section className="content">
        <header className="topbar">
          <div>
            <p className="eyebrow">{payload.projectName}</p>
            <h2>{payload.workspaceName}</h2>
            <p>{payload.workspaceDescription}</p>
            <div className="project-path-line">
              <span>Project Path</span>
              <code>{payload.projectFolder}</code>
            </div>
          </div>
          <div className="status-strip">
            <span>Architecture as Code</span>
            <span>C4</span>
            <span>UML</span>
          </div>
        </header>

        {activeView === "settings" ? (
          <SettingsView
            activeProjectId={payload.projectId}
            fixedHookPrompts={payload.fixedHookPrompts}
            settings={settings}
            onAdd={(updatedSettings, projectId) => {
              setSettings(updatedSettings);
              loadDashboard(projectId);
              setActiveView("design");
            }}
            onDelete={(updatedSettings) => {
              setSettings(updatedSettings);
              loadDashboard(updatedSettings.lastActiveProjectId);
            }}
            onDashboardChange={(updatedPayload) => {
              setPayload((currentPayload) =>
                currentPayload ? mergeDashboardPayload(currentPayload, updatedPayload) : updatedPayload,
              );
            }}
            onError={setError}
          />
        ) : activeView === "rules" ? (
          <RulesView
            projectId={payload.projectId}
            rules={payload.rules}
            ruleTemplates={settings.ruleTemplates}
            onChange={(updatedPayload) => {
              setPayload((currentPayload) =>
                currentPayload ? mergeDashboardPayload(currentPayload, updatedPayload) : updatedPayload,
              );
            }}
            onError={setError}
            onSettingsChange={setSettings}
          />
        ) : activeView === "memory" ? (
          <MemoryView
            projectId={payload.projectId}
            memory={payload.memory}
            onChange={(updatedPayload) => {
              setPayload((currentPayload) =>
                currentPayload ? mergeDashboardPayload(currentPayload, updatedPayload) : updatedPayload,
              );
            }}
            onError={setError}
          />
        ) : activeView === "tasks" ? (
          <TasksView
            projectId={payload.projectId}
            tasks={payload.tasks}
            designElements={payload.designElements}
            designRelationships={payload.designRelationships}
            diagrams={payload.diagrams}
            postTaskCommands={payload.postTaskCommands}
            onChange={(updatedPayload) => {
              setPayload((currentPayload) =>
                currentPayload ? mergeDashboardPayload(currentPayload, updatedPayload) : updatedPayload,
              );
            }}
            onError={setError}
            onOpenDesignLink={(link) => {
              const target =
                link.targetType === "uml"
                  ? payload.diagrams.find((diagram) => diagram.key === link.designExternalId)
                  : null;
              const externalId =
                link.targetType === "uml"
                  ? target?.attachedToExternalId ?? link.designExternalId
                  : link.designExternalId;
              const entityType =
                link.targetType === "relationship" || target?.attachedToTargetType === "relationship"
                  ? "relationship"
                  : "element";

              setSelectedDesignEntity({ type: entityType, externalId });
              setActiveDesignLevel(link.targetType === "uml" || entityType === "relationship" ? "features" : "components");
              setActiveView("design");
            }}
          />
        ) : activeView === "qa" ? (
          <QaView
            projectId={payload.projectId}
            qaChecks={payload.qaChecks}
            qaJobs={payload.qaJobs}
            tasks={payload.tasks}
            designElements={payload.designElements}
            designRelationships={payload.designRelationships}
            diagrams={payload.diagrams}
            onChange={(updatedPayload) => {
              setPayload((currentPayload) =>
                currentPayload ? mergeDashboardPayload(currentPayload, updatedPayload) : updatedPayload,
              );
            }}
            onError={setError}
            onOpenDesignLink={(link) => {
              const target =
                link.targetType === "uml"
                  ? payload.diagrams.find((diagram) => diagram.key === link.designExternalId)
                  : null;
              const externalId =
                link.targetType === "uml"
                  ? target?.attachedToExternalId ?? link.designExternalId
                  : link.designExternalId;
              const entityType =
                link.targetType === "relationship" || target?.attachedToTargetType === "relationship"
                  ? "relationship"
                  : "element";

              setSelectedDesignEntity({ type: entityType, externalId });
              setActiveDesignLevel(link.targetType === "uml" || entityType === "relationship" ? "features" : "components");
              setActiveView("design");
            }}
          />
        ) : (
          <DesignBrowser
            activeLevel={activeDesignLevel}
            payload={payload}
            selectedEntity={selectedDesignEntity}
            onChange={(updatedPayload) => {
              setPayload((currentPayload) =>
                currentPayload ? mergeDashboardPayload(currentPayload, updatedPayload) : updatedPayload,
              );
            }}
            onError={setError}
            onLevelChange={setActiveDesignLevel}
            onSelect={setSelectedDesignEntity}
          />
        )}
      </section>
    </main>
  );
}

function mergeDashboardPayload(current: DashboardPayload, next: DashboardPayload): DashboardPayload {
  return {
    ...current,
    ...next,
    designElements: reconcileById(current.designElements, next.designElements),
    designRelationships: reconcileById(current.designRelationships, next.designRelationships),
    diagrams: reconcileById(current.diagrams, next.diagrams),
    tasks: reconcileById(current.tasks, next.tasks),
    guidelines: reconcileById(current.guidelines, next.guidelines),
    postTaskCommands: reconcileById(current.postTaskCommands, next.postTaskCommands),
    qaChecks: reconcileById(current.qaChecks, next.qaChecks),
    qaJobs: reconcileById(current.qaJobs, next.qaJobs),
    qaRuns: reconcileById(current.qaRuns, next.qaRuns),
    rules: reconcileById(current.rules, next.rules),
    ruleTemplates: reconcileByStringId(current.ruleTemplates, next.ruleTemplates),
    fixedHookPrompts: reconcileByKey(current.fixedHookPrompts, next.fixedHookPrompts),
  };
}

function reconcileById<T extends { id: number }>(current: T[], next: T[]): T[] {
  const currentById = new Map(current.map((item) => [item.id, item]));
  let changed = current.length !== next.length;

  const merged = next.map((nextItem, index) => {
    const currentItem = currentById.get(nextItem.id);

    if (current[index]?.id !== nextItem.id) {
      changed = true;
    }

    if (currentItem && shallowEqualRecord(currentItem, nextItem)) {
      return currentItem;
    }

    changed = true;
    return nextItem;
  });

  return changed ? merged : current;
}

function reconcileByStringId<T extends { id: string }>(current: T[], next: T[]): T[] {
  const currentById = new Map(current.map((item) => [item.id, item]));
  let changed = current.length !== next.length;

  const merged = next.map((nextItem, index) => {
    const currentItem = currentById.get(nextItem.id);

    if (current[index]?.id !== nextItem.id) {
      changed = true;
    }

    if (currentItem && shallowEqualRecord(currentItem, nextItem)) {
      return currentItem;
    }

    changed = true;
    return nextItem;
  });

  return changed ? merged : current;
}

function reconcileByKey<T extends { key: string }>(current: T[], next: T[]): T[] {
  const currentByKey = new Map(current.map((item) => [item.key, item]));
  let changed = current.length !== next.length;

  const merged = next.map((nextItem, index) => {
    const currentItem = currentByKey.get(nextItem.key);

    if (current[index]?.key !== nextItem.key) {
      changed = true;
    }

    if (currentItem && shallowEqualRecord(currentItem, nextItem)) {
      return currentItem;
    }

    changed = true;
    return nextItem;
  });

  return changed ? merged : current;
}

function shallowEqualRecord(left: object, right: object): boolean {
  const leftRecord = left as Record<string, unknown>;
  const rightRecord = right as Record<string, unknown>;
  const leftKeys = Object.keys(left);
  const rightKeys = Object.keys(right);

  if (leftKeys.length !== rightKeys.length) {
    return false;
  }

  return leftKeys.every((key) => leftRecord[key] === rightRecord[key]);
}

function DesignBrowser({
  activeLevel,
  payload,
  selectedEntity,
  onChange,
  onError,
  onLevelChange,
  onSelect,
}: {
  activeLevel: DesignLevel;
  payload: DashboardPayload;
  selectedEntity: { type: DesignEntityType; externalId: string } | null;
  onChange: (payload: DashboardPayload) => void;
  onError: (message: string) => void;
  onLevelChange: (level: DesignLevel) => void;
  onSelect: (entity: { type: DesignEntityType; externalId: string } | null) => void;
}) {
  const [query, setQuery] = React.useState("");
  const [isResizingSource, setIsResizingSource] = React.useState(false);
  const [sourcePanelHeight, setSourcePanelHeight] = React.useState(200);
  const [activeArtifactKey, setActiveArtifactKey] = React.useState<string | null>(null);
  const [projectionMode, setProjectionMode] = React.useState<DesignProjectionMode>("branch");
  const [structurizrZoom, setStructurizrZoom] = React.useState(0);
  const designMainRef = React.useRef<HTMLElement | null>(null);
  const designTree = React.useMemo(() => buildDesignTree(payload.designElements), [payload.designElements]);
  const rootElement =
    payload.designElements.find(
      (element) => !element.parentExternalId && element.elementType.toLowerCase() === "software system",
    ) ??
    payload.designElements.find((element) => !element.parentExternalId) ??
    null;
  const selectedElement =
    selectedEntity?.type === "element"
      ? payload.designElements.find((element) => element.externalId === selectedEntity.externalId) ?? null
      : null;
  const selectedRelationship =
    selectedEntity?.type === "relationship"
      ? payload.designRelationships.find((relationship) => relationship.externalId === selectedEntity.externalId) ?? null
      : null;
  const activeBranchElement = activeLevel === "context" ? rootElement : selectedElement ?? rootElement;
  const branchChildren = activeBranchElement
    ? payload.designElements.filter((element) => element.parentExternalId === activeBranchElement.externalId)
    : [];
  const branchRelationships = activeBranchElement
    ? payload.designRelationships.filter(
        (relationship) =>
          relationship.sourceExternalId === activeBranchElement.externalId ||
          relationship.destinationExternalId === activeBranchElement.externalId ||
          branchChildren.some(
            (child) =>
              child.externalId === relationship.sourceExternalId || child.externalId === relationship.destinationExternalId,
          ),
      )
    : [];
  const artifactTarget = selectedRelationship
    ? { type: "relationship" as const, externalId: selectedRelationship.externalId, name: selectedRelationship.description }
    : activeBranchElement
      ? { type: "element" as const, externalId: activeBranchElement.externalId, name: activeBranchElement.name }
      : null;
  const umlArtifacts = React.useMemo(
    () => selectUmlArtifacts(payload.diagrams, artifactTarget),
    [artifactTarget?.externalId, artifactTarget?.type, payload.diagrams],
  );
  const activeUmlArtifact = umlArtifacts.find((diagram) => diagram.key === activeArtifactKey) ?? umlArtifacts[0];
  const branchStructurizr = React.useMemo(
    () =>
      buildBranchStructurizrWorkspace(
        payload,
        activeLevel,
        activeBranchElement,
        rootElement,
        payload.structurizrViewKey,
        projectionMode,
      ),
    [activeBranchElement, activeLevel, payload, projectionMode, rootElement],
  );
  const sourceDiagram =
    activeLevel === "features"
      ? activeUmlArtifact
      : payload.diagrams.find((diagram) => diagram.kind === "structurizr");
  const visibleBranchRelationships =
    activeLevel === "features" || projectionMode === "branch"
      ? branchRelationships
      : payload.designRelationships.filter((relationship) => branchStructurizr.visibleRelationshipIds.has(relationship.externalId));

  React.useEffect(() => {
    if (activeArtifactKey && umlArtifacts.some((diagram) => diagram.key === activeArtifactKey)) {
      return;
    }

    setActiveArtifactKey(umlArtifacts[0]?.key ?? null);
  }, [activeArtifactKey, umlArtifacts]);

  function selectLevel(level: DesignLevel) {
    onLevelChange(level);

    if (level === "context" && rootElement) {
      onSelect({ type: "element", externalId: rootElement.externalId });
    }

    if (level === "components" && rootElement && (!selectedElement || !hasChildElements(selectedElement, payload.designElements))) {
      onSelect({ type: "element", externalId: rootElement.externalId });
    }

    if (level === "features" && !selectedEntity && rootElement) {
      onSelect({ type: "element", externalId: rootElement.externalId });
    }
  }

  function selectTreeElement(element: DesignElement) {
    onSelect({ type: "element", externalId: element.externalId });

    if (hasChildElements(element, payload.designElements)) {
      onLevelChange("components");
      return;
    }

    onLevelChange(element.parentExternalId ? "features" : "context");
  }

  function resizeSourcePanel(clientY: number) {
    const bounds = designMainRef.current?.getBoundingClientRect();

    if (!bounds) {
      return;
    }

    const maximumHeight = Math.max(140, bounds.height - 220);
    const nextHeight = Math.min(Math.max(bounds.bottom - clientY, 120), maximumHeight);
    setSourcePanelHeight(nextHeight);
  }

  const breadcrumbElements = buildBreadcrumb(activeBranchElement, payload.designElements);
  const viewerTitle =
    activeLevel === "context"
      ? "Level 1: System Context"
      : activeLevel === "components"
        ? `Level 2: ${activeBranchElement?.name ?? "Components"}`
        : `Level 3: ${activeBranchElement?.name ?? "UML Artifacts"}`;

  return (
    <section id="design" className="design-browser">
      <aside className="design-index-panel">
        <div className="design-level-tabs" role="tablist" aria-label="C4 design level">
          {DESIGN_LEVELS.map((level) => (
            <button
              className={activeLevel === level.id ? "active" : ""}
              key={level.id}
              onClick={() => selectLevel(level.id)}
              type="button"
            >
              <level.icon size={17} />
              <span>{level.label}</span>
            </button>
          ))}
        </div>

        <div className="design-search">
          <Search size={16} />
          <input
            aria-label="Filter design entities"
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Filter design"
            value={query}
          />
        </div>

        <DesignTree
          activeLevel={activeLevel}
          branchElement={activeBranchElement}
          elements={payload.designElements}
          query={query}
          relationships={payload.designRelationships}
          roots={designTree}
          selectedEntity={selectedEntity}
          onSelectElement={selectTreeElement}
          onSelectRelationship={(relationship) => onSelect({ type: "relationship", externalId: relationship.externalId })}
        />
      </aside>

      <section
        className={isResizingSource ? "design-main resizing-source" : "design-main"}
        ref={designMainRef}
        style={{ "--design-source-height": `${sourcePanelHeight}px` } as React.CSSProperties}
      >
        <div className="design-breadcrumbs" aria-label="Design breadcrumbs">
          <button onClick={() => selectLevel("context")} type="button">System Context</button>
          {breadcrumbElements.map((element) => (
            <React.Fragment key={element.externalId}>
              <ChevronRight size={15} />
              <button onClick={() => selectTreeElement(element)} type="button">{element.name}</button>
            </React.Fragment>
          ))}
          {activeLevel === "features" ? (
            <>
              <ChevronRight size={15} />
              <span>UML Artifacts</span>
            </>
          ) : null}
        </div>

        <div className="viewer-panel design-viewer-panel">
          <div className="panel-heading">
            <div>
              <p className="eyebrow">
                {activeLevel === "features"
                  ? "Mermaid UML Artifacts"
                  : projectionMode === "dependencies"
                    ? "Structurizr Dependency View"
                    : "Structurizr C4"}
              </p>
              <h3>{viewerTitle}</h3>
            </div>
            {activeLevel === "features" ? (
              <div className="status-strip compact">
                <span>{artifactTarget?.type ?? "artifact"}</span>
                <span>{activeUmlArtifact?.key ?? "No attached UML"}</span>
              </div>
            ) : (
              <div className="viewer-controls">
                <div className="segmented compact" role="tablist" aria-label="C4 view mode">
                  <button
                    aria-selected={projectionMode === "branch"}
                    className={projectionMode === "branch" ? "active" : ""}
                    onClick={() => setProjectionMode("branch")}
                    role="tab"
                    title="Branch C4 view"
                    type="button"
                  >
                    <Network size={15} />
                    <span>Branch</span>
                  </button>
                  <button
                    aria-selected={projectionMode === "dependencies"}
                    className={projectionMode === "dependencies" ? "active" : ""}
                    onClick={() => setProjectionMode("dependencies")}
                    role="tab"
                    title="Dependency View"
                    type="button"
                  >
                    <GitBranch size={15} />
                    <span>Dependency</span>
                  </button>
                </div>
                <div className="status-strip compact">
                  <span>{projectionMode === "dependencies" ? "dependency view" : "branch view"}</span>
                  <span>{branchStructurizr.viewKey}</span>
                  {branchStructurizr.hiddenRelationshipCount > 0 ? (
                    <span>{branchStructurizr.hiddenRelationshipCount} hidden</span>
                  ) : null}
                </div>
                <div className="zoom-control">
                  <ZoomOut size={15} />
                  <input
                    aria-label="Structurizr zoom"
                    max={220}
                    min={25}
                    onChange={(event) => setStructurizrZoom(Number(event.target.value))}
                    step={5}
                    type="range"
                    value={structurizrZoom === 0 ? 100 : structurizrZoom}
                  />
                  <ZoomIn size={15} />
                  <button onClick={() => setStructurizrZoom(0)} title="Fit diagram" type="button">
                    Fit
                  </button>
                  <span>{structurizrZoom === 0 ? "Fit" : `${structurizrZoom}%`}</span>
                </div>
              </div>
            )}
          </div>

          {activeLevel === "features" ? (
            <div className="uml-artifact-viewer">
              <UmlArtifactTabs
                activeKey={activeUmlArtifact?.key ?? null}
                artifacts={umlArtifacts}
                onSelect={setActiveArtifactKey}
              />
              <MermaidPanel diagram={activeUmlArtifact?.source ?? ""} />
            </div>
          ) : (
            <StructurizrFrame
              workspace={branchStructurizr.workspace}
              viewKey={branchStructurizr.viewKey}
              zoomPercent={structurizrZoom}
              onZoomChange={setStructurizrZoom}
              onOpen={(entity) => {
                onSelect(entity);

                if (entity.type === "element") {
                  const openedElement = payload.designElements.find((element) => element.externalId === entity.externalId);
                  onLevelChange(openedElement && hasChildElements(openedElement, payload.designElements) ? "components" : "features");
                }
              }}
              onSelect={(externalId) => {
                onSelect({ type: "element", externalId });
              }}
            />
          )}
        </div>

        <button
          aria-label="Resize source view"
          className="panel-resizer"
          onLostPointerCapture={() => setIsResizingSource(false)}
          onPointerDown={(event) => {
            event.currentTarget.setPointerCapture(event.pointerId);
            setIsResizingSource(true);
            resizeSourcePanel(event.clientY);
          }}
          onPointerMove={(event) => {
            if (isResizingSource) {
              resizeSourcePanel(event.clientY);
            }
          }}
          onPointerUp={(event) => {
            resizeSourcePanel(event.clientY);
            event.currentTarget.releasePointerCapture(event.pointerId);
            setIsResizingSource(false);
          }}
          type="button"
        />

        <SourcePanel
          diagram={sourceDiagram}
          source={
            activeLevel === "features"
              ? sourceDiagram?.source ?? ""
              : projectionMode === "dependencies"
                ? branchStructurizr.source
                : payload.structurizrDsl
          }
          sourceKind={
            activeLevel === "features"
              ? activeUmlArtifact?.artifactLabel ?? "Mermaid UML"
              : projectionMode === "dependencies"
                ? "Dependency Projection"
                : "Structurizr DSL"
          }
        />
      </section>

      <DesignInspector
        elements={payload.designElements}
        projectId={payload.projectId}
        relationships={payload.designRelationships}
        selectedElement={selectedElement}
        selectedRelationship={selectedRelationship}
        branchChildren={branchChildren}
        branchRelationships={visibleBranchRelationships}
        onChange={onChange}
        onError={onError}
        onJumpToFeatures={() => selectLevel("features")}
      />
    </section>
  );
}

type DesignTreeNode = {
  element: DesignElement;
  children: DesignTreeNode[];
};

const DESIGN_LEVELS: { id: DesignLevel; label: string; icon: React.ElementType }[] = [
  { id: "context", label: "System Context", icon: Network },
  { id: "components", label: "Components", icon: Braces },
  { id: "features", label: "UML Artifacts", icon: GitBranch },
];

const DESIGN_LEVEL_LABELS: Record<DesignLevel, string> = {
  context: "Level 1: System Context",
  components: "Level 2: Components",
  features: "Level 3: UML Artifacts",
};

function DesignTree({
  activeLevel,
  branchElement,
  elements,
  query,
  relationships,
  roots,
  selectedEntity,
  onSelectElement,
  onSelectRelationship,
}: {
  activeLevel: DesignLevel;
  branchElement: DesignElement | null;
  elements: DesignElement[];
  query: string;
  relationships: DesignRelationship[];
  roots: DesignTreeNode[];
  selectedEntity: { type: DesignEntityType; externalId: string } | null;
  onSelectElement: (element: DesignElement) => void;
  onSelectRelationship: (relationship: DesignRelationship) => void;
}) {
  const normalizedQuery = query.trim().toLowerCase();
  const [expandedIds, setExpandedIds] = React.useState<Set<string>>(() => new Set());
  const branchRelationships = branchElement
    ? relationships.filter(
        (relationship) =>
          relationship.sourceExternalId === branchElement.externalId ||
          relationship.destinationExternalId === branchElement.externalId,
      )
    : [];

  function toggleBranch(externalId: string) {
    setExpandedIds((current) => {
      const next = new Set(current);

      if (next.has(externalId)) {
        next.delete(externalId);
      } else {
        next.add(externalId);
      }

      return next;
    });
  }

  return (
    <div className="design-list">
      <div className="design-tree-heading">
        <span>{DESIGN_LEVEL_LABELS[activeLevel]}</span>
        {branchElement ? <strong>{branchElement.name}</strong> : null}
      </div>

      {roots.map((node) => (
        <DesignTreeItem
          key={node.element.externalId}
          node={node}
          normalizedQuery={normalizedQuery}
          selectedEntity={selectedEntity}
          activeBranchExternalId={branchElement?.externalId ?? null}
          expandedIds={expandedIds}
          onToggleBranch={toggleBranch}
          onSelectElement={onSelectElement}
        />
      ))}

      {activeLevel === "features" ? (
        <article className="design-list-note">
          <GitBranch size={18} />
          <div>
            <strong>{branchElement ? `${branchElement.name} UML artifacts` : "UML artifact source"}</strong>
            <span>Agents edit typed Mermaid artifacts attached to the selected element or relationship.</span>
          </div>
        </article>
      ) : null}

      {branchRelationships.length > 0 ? <h4>Branch Relationships</h4> : null}
      {branchRelationships.map((relationship) => {
        const source = elements.find((element) => element.externalId === relationship.sourceExternalId);
        const destination = elements.find((element) => element.externalId === relationship.destinationExternalId);
        const haystack = `${relationship.description} ${relationship.technology} ${source?.name ?? ""} ${destination?.name ?? ""}`;

        if (normalizedQuery && !haystack.toLowerCase().includes(normalizedQuery)) {
          return null;
        }

        return (
          <button
            className={
              selectedEntity?.type === "relationship" && selectedEntity.externalId === relationship.externalId
                ? "design-list-item relationship active"
                : "design-list-item relationship"
            }
            key={relationship.externalId}
            onClick={() => onSelectRelationship(relationship)}
            type="button"
          >
            <strong>
              {source?.name ?? relationship.sourceExternalId} {"->"} {destination?.name ?? relationship.destinationExternalId}
            </strong>
            <span>{relationship.description}</span>
          </button>
        );
      })}
    </div>
  );
}

function DesignTreeItem({
  node,
  normalizedQuery,
  selectedEntity,
  activeBranchExternalId,
  expandedIds,
  onToggleBranch,
  onSelectElement,
}: {
  node: DesignTreeNode;
  normalizedQuery: string;
  selectedEntity: { type: DesignEntityType; externalId: string } | null;
  activeBranchExternalId: string | null;
  expandedIds: Set<string>;
  onToggleBranch: (externalId: string) => void;
  onSelectElement: (element: DesignElement) => void;
}) {
  const matches =
    !normalizedQuery ||
    `${node.element.name} ${node.element.description} ${node.element.elementType} ${node.element.tags}`
      .toLowerCase()
      .includes(normalizedQuery);
  const visibleChildren = node.children.filter((child) => treeNodeMatches(child, normalizedQuery));
  const hasChildren = visibleChildren.length > 0;
  const isSearching = normalizedQuery.length > 0;
  const isExpanded = isSearching || expandedIds.has(node.element.externalId);

  if (!matches && visibleChildren.length === 0) {
    return null;
  }

  return (
    <div className="design-tree-node">
      <div className="design-tree-row">
        {hasChildren ? (
          <button
            aria-expanded={isExpanded}
            aria-label={`${isExpanded ? "Collapse" : "Expand"} ${node.element.name}`}
            className="design-tree-toggle"
            onClick={() => onToggleBranch(node.element.externalId)}
            type="button"
          >
            <ChevronRight className={isExpanded ? "expanded" : ""} size={16} />
          </button>
        ) : (
          <span className="design-tree-toggle-placeholder" />
        )}
        <button
          className={[
            "design-list-item",
            selectedEntity?.type === "element" && selectedEntity.externalId === node.element.externalId ? "active" : "",
            activeBranchExternalId === node.element.externalId ? "branch-active" : "",
          ]
            .filter(Boolean)
            .join(" ")}
          onClick={() => onSelectElement(node.element)}
          type="button"
        >
          <strong>{node.element.name}</strong>
          <span>{node.element.elementType}</span>
        </button>
      </div>
      {hasChildren && isExpanded ? (
        <div className="design-tree-children">
          {visibleChildren.map((child) => (
            <DesignTreeItem
              activeBranchExternalId={activeBranchExternalId}
              expandedIds={expandedIds}
              key={child.element.externalId}
              node={child}
              normalizedQuery={normalizedQuery}
              selectedEntity={selectedEntity}
              onToggleBranch={onToggleBranch}
              onSelectElement={onSelectElement}
            />
          ))}
        </div>
      ) : null}
    </div>
  );
}

function buildDesignTree(elements: DesignElement[]): DesignTreeNode[] {
  const nodes = new Map<string, DesignTreeNode>();
  const roots: DesignTreeNode[] = [];

  elements.forEach((element) => {
    nodes.set(element.externalId, { element, children: [] });
  });

  elements.forEach((element) => {
    const node = nodes.get(element.externalId);

    if (!node) {
      return;
    }

    const parentNode = element.parentExternalId ? nodes.get(element.parentExternalId) : null;

    if (parentNode) {
      parentNode.children.push(node);
    } else {
      roots.push(node);
    }
  });

  return roots;
}

function treeNodeMatches(node: DesignTreeNode, normalizedQuery: string): boolean {
  if (!normalizedQuery) {
    return true;
  }

  const matches = `${node.element.name} ${node.element.description} ${node.element.elementType} ${node.element.tags}`
    .toLowerCase()
    .includes(normalizedQuery);

  return matches || node.children.some((child) => treeNodeMatches(child, normalizedQuery));
}

function hasChildElements(element: DesignElement, elements: DesignElement[]): boolean {
  return elements.some((candidate) => candidate.parentExternalId === element.externalId);
}

function buildBreadcrumb(element: DesignElement | null, elements: DesignElement[]): DesignElement[] {
  if (!element) {
    return [];
  }

  const byId = new Map(elements.map((candidate) => [candidate.externalId, candidate]));
  const path: DesignElement[] = [];
  let current: DesignElement | undefined = element;

  while (current) {
    path.unshift(current);
    current = current.parentExternalId ? byId.get(current.parentExternalId) : undefined;
  }

  return path;
}

function selectUmlArtifacts(
  diagrams: DesignDiagram[],
  target: { type: DesignEntityType; externalId: string } | null,
): DesignDiagram[] {
  if (!target) {
    return [];
  }

  return diagrams
    .filter(
      (diagram) =>
        diagram.kind === "mermaid" &&
        diagram.attachedToExternalId === target.externalId &&
        (!diagram.attachedToTargetType || diagram.attachedToTargetType === target.type),
    )
    .sort(compareUmlArtifacts);
}

function compareUmlArtifacts(left: DesignDiagram, right: DesignDiagram): number {
  return (
    left.artifactRank - right.artifactRank ||
    left.sortOrder - right.sortOrder ||
    left.title.localeCompare(right.title) ||
    left.key.localeCompare(right.key)
  );
}

function buildBranchStructurizrWorkspace(
  payload: DashboardPayload,
  activeLevel: DesignLevel,
  activeBranchElement: DesignElement | null,
  rootElement: DesignElement | null,
  fallbackViewKey: string,
  projectionMode: DesignProjectionMode,
): StructurizrProjection {
  const viewKey = `${sanitizeViewKey(payload.projectId)}-${projectionMode}-${activeLevel}-${sanitizeViewKey(activeBranchElement?.externalId ?? "root")}`;
  const branchIds = selectStructurizrViewElementIds(payload.designElements, activeLevel, activeBranchElement, rootElement);
  const dependencyProjection =
    projectionMode === "dependencies" && activeLevel !== "context"
      ? buildDependencyProjection(payload.designElements, payload.designRelationships, branchIds, activeBranchElement)
      : null;
  const visibleIds = dependencyProjection?.visibleIds ?? branchIds;
  const visibleElements = payload.designElements.filter((element) => visibleIds.has(element.externalId));
  const visibleRelationships =
    dependencyProjection?.relationships ??
    payload.designRelationships.filter(
      (relationship) => visibleIds.has(relationship.sourceExternalId) && visibleIds.has(relationship.destinationExternalId),
    );
  const visibleRelationshipIds = new Set(visibleRelationships.map((relationship) => relationship.externalId));
  const viewElements = buildViewLayout(
    visibleElements,
    activeLevel,
    activeBranchElement,
    dependencyProjection?.roles ?? new Map(),
    projectionMode,
  ).map((layout) => ({
    id: layout.element.externalId,
    relationships: visibleRelationships
      .filter((relationship) => relationship.sourceExternalId === layout.element.externalId)
      .map((relationship) => relationship.externalId),
    x: layout.x,
    y: layout.y,
    width: layout.width,
    height: layout.height,
  }));
  const view = buildStructurizrView(
    viewKey || fallbackViewKey,
    activeLevel,
    activeBranchElement,
    rootElement,
    viewElements,
    visibleRelationships,
    projectionMode,
  );
  const workspace: StructurizrWorkspaceJson = {
    id: 1,
    name: payload.workspaceName,
    description: payload.workspaceDescription,
    model: buildStructurizrDisplayModel(
      payload.designElements
        .filter((element) => element.elementType.toLowerCase() === "person" && !element.parentExternalId)
        .concat(
          payload.designElements.filter(
            (element) => element.elementType.toLowerCase() === "software system" && !element.parentExternalId,
          ),
        ),
      payload.designElements,
      payload.designRelationships,
    ),
    views: {
      configuration: {
        defaultView: view.key,
        styles: {
          elements: [
            { tag: "Element", fontSize: 14 },
            { tag: "Person", shape: "Person", background: "#2f6f6d", color: "#ffffff", fontSize: 14 },
            { tag: "Software System", background: "#335c67", color: "#ffffff", fontSize: 14 },
            { tag: "Container", background: "#fffaf0", color: "#1f2933", stroke: "#2f6f6d", fontSize: 14 },
            { tag: "Component", background: "#f8faf7", color: "#1f2933", stroke: "#7fb6ad", fontSize: 14 },
            { tag: "Database", shape: "Cylinder", background: "#e4b363", color: "#1f2933", fontSize: 14 },
            { tag: "Placeholder", background: "#f7efe3", color: "#1f2933", stroke: "#c89f5d", fontSize: 14 },
          ],
          relationships: [{ tag: "Relationship", color: "#47615f", fontSize: 11, thickness: 2 }],
        },
      },
    },
  };

  if (activeLevel === "context") {
    workspace.views.systemContextViews = [view];
  } else if (activeBranchElement?.elementType.toLowerCase() === "container") {
    workspace.views.componentViews = [view];
  } else {
    workspace.views.containerViews = [view];
  }

  return {
    workspace: JSON.stringify(workspace),
    viewKey: view.key,
    source:
      projectionMode === "dependencies" && dependencyProjection
        ? buildDependencyProjectionSource(
            activeBranchElement,
            visibleElements,
            visibleRelationships,
            dependencyProjection.hiddenRelationshipCount,
            payload.designElements,
          )
        : payload.structurizrDsl,
    visibleElementIds: visibleIds,
    visibleRelationshipIds,
    hiddenRelationshipCount: dependencyProjection?.hiddenRelationshipCount ?? 0,
  };
}

type DependencyNodeRole = "incoming" | "core" | "outgoing";

type DependencyProjection = {
  visibleIds: Set<string>;
  relationships: DesignRelationship[];
  roles: Map<string, DependencyNodeRole>;
  hiddenRelationshipCount: number;
};

function buildDependencyProjection(
  elements: DesignElement[],
  relationships: DesignRelationship[],
  branchIds: Set<string>,
  activeBranchElement: DesignElement | null,
): DependencyProjection {
  const visibleIds = new Set(branchIds);
  const roles = new Map<string, DependencyNodeRole>();
  const branchRelationshipIds = new Set<string>();

  branchIds.forEach((externalId) => roles.set(externalId, "core"));

  relationships.forEach((relationship) => {
    const sourceInBranch = branchIds.has(relationship.sourceExternalId);
    const destinationInBranch = branchIds.has(relationship.destinationExternalId);

    if (!sourceInBranch && !destinationInBranch) {
      return;
    }

    branchRelationshipIds.add(relationship.externalId);

    if (sourceInBranch && !destinationInBranch) {
      visibleIds.add(relationship.destinationExternalId);
      roles.set(relationship.destinationExternalId, "outgoing");
    }

    if (!sourceInBranch && destinationInBranch) {
      visibleIds.add(relationship.sourceExternalId);
      roles.set(relationship.sourceExternalId, roles.get(relationship.sourceExternalId) === "outgoing" ? "outgoing" : "incoming");
    }
  });

  const candidates = relationships
    .filter((relationship) => branchRelationshipIds.has(relationship.externalId))
    .filter((relationship) => visibleIds.has(relationship.sourceExternalId) && visibleIds.has(relationship.destinationExternalId))
    .map((relationship) => ({
      relationship,
      score: scoreDependencyRelationship(relationship, branchIds, activeBranchElement, elements),
    }))
    .sort((left, right) => right.score - left.score || compareRelationshipStable(left.relationship, right.relationship, elements));

  const kept: DesignRelationship[] = [];
  const endpointCounts = new Map<string, number>();
  const maxTotalRelationships = 48;
  const maxRelationshipsPerEndpoint = 8;

  for (const candidate of candidates) {
    const sourceCount = endpointCounts.get(candidate.relationship.sourceExternalId) ?? 0;
    const destinationCount = endpointCounts.get(candidate.relationship.destinationExternalId) ?? 0;

    if (
      kept.length >= maxTotalRelationships ||
      sourceCount >= maxRelationshipsPerEndpoint ||
      destinationCount >= maxRelationshipsPerEndpoint
    ) {
      continue;
    }

    kept.push(candidate.relationship);
    endpointCounts.set(candidate.relationship.sourceExternalId, sourceCount + 1);
    endpointCounts.set(candidate.relationship.destinationExternalId, destinationCount + 1);
  }

  return {
    visibleIds,
    relationships: kept.sort((left, right) => compareRelationshipStable(left, right, elements)),
    roles,
    hiddenRelationshipCount: candidates.length - kept.length,
  };
}

function scoreDependencyRelationship(
  relationship: DesignRelationship,
  branchIds: Set<string>,
  activeBranchElement: DesignElement | null,
  elements: DesignElement[],
): number {
  let score = 0;
  const sourceInBranch = branchIds.has(relationship.sourceExternalId);
  const destinationInBranch = branchIds.has(relationship.destinationExternalId);

  if (sourceInBranch && destinationInBranch) {
    score += 40;
  } else {
    score += 30;
  }

  if (
    activeBranchElement &&
    (relationship.sourceExternalId === activeBranchElement.externalId ||
      relationship.destinationExternalId === activeBranchElement.externalId)
  ) {
    score += 12;
  }

  if (relationship.technology.trim()) {
    score += 6;
  }

  if (relationship.description.trim()) {
    score += 4;
  }

  const source = elements.find((element) => element.externalId === relationship.sourceExternalId);
  const destination = elements.find((element) => element.externalId === relationship.destinationExternalId);

  if (source?.parentExternalId === destination?.parentExternalId) {
    score += 3;
  }

  return score;
}

function buildStructurizrDisplayModel(
  topLevelElements: DesignElement[],
  allElements: DesignElement[],
  allRelationships: DesignRelationship[],
): StructurizrWorkspaceJson["model"] {
  const people = topLevelElements
    .filter((element) => element.elementType.toLowerCase() === "person")
    .map((element) => elementToStructurizrJson(element, allElements, allRelationships));
  const softwareSystems = topLevelElements
    .filter((element) => element.elementType.toLowerCase() === "software system" && !element.parentExternalId)
    .map((element) => elementToStructurizrJson(element, allElements, allRelationships));

  return { people, softwareSystems };
}

function selectStructurizrViewElementIds(
  elements: DesignElement[],
  activeLevel: DesignLevel,
  activeBranchElement: DesignElement | null,
  rootElement: DesignElement | null,
): Set<string> {
  const visibleIds = new Set<string>();

  if (activeLevel === "context") {
    elements
      .filter((element) => !element.parentExternalId && ["person", "software system"].includes(element.elementType.toLowerCase()))
      .forEach((element) => visibleIds.add(element.externalId));
    return visibleIds;
  }

  if (!activeBranchElement) {
    return visibleIds;
  }

  const children = elements.filter((element) => element.parentExternalId === activeBranchElement.externalId);

  if (activeBranchElement.externalId === rootElement?.externalId) {
    elements
      .filter((element) => element.elementType.toLowerCase() === "person" && !element.parentExternalId)
      .forEach((element) => visibleIds.add(element.externalId));
  }

  if (children.length === 0) {
    visibleIds.add(activeBranchElement.externalId);
    return visibleIds;
  }

  children.forEach((element) => visibleIds.add(element.externalId));
  return visibleIds;
}

function buildStructurizrView(
  viewKey: string,
  activeLevel: DesignLevel,
  activeBranchElement: DesignElement | null,
  rootElement: DesignElement | null,
  elements: StructurizrViewElementJson[],
  relationships: DesignRelationship[],
  projectionMode: DesignProjectionMode,
): StructurizrViewJson {
  const view: StructurizrViewJson = {
    key: viewKey,
    description: activeBranchElement
      ? `${activeBranchElement.name} ${activeLevel} ${projectionMode} view`
      : "Adashi branch view",
    softwareSystemId: rootElement?.externalId,
    elements,
    relationships: relationships.map((relationship) => ({ id: relationship.externalId })),
    automaticLayout: {
      implementation: "Dagre",
      rankDirection: "LeftRight",
      rankSeparation: projectionMode === "dependencies" ? 220 : 300,
      nodeSeparation: projectionMode === "dependencies" ? 180 : 240,
      edgeSeparation: 70,
      vertices: true,
    },
  };

  if (activeBranchElement?.elementType.toLowerCase() === "container") {
    view.containerId = activeBranchElement.externalId;
    delete view.softwareSystemId;
  }

  return view;
}

function buildViewLayout(
  elements: DesignElement[],
  activeLevel: DesignLevel,
  activeBranchElement: DesignElement | null,
  dependencyRoles: Map<string, DependencyNodeRole> = new Map(),
  projectionMode: DesignProjectionMode = "branch",
): Array<{
  element: DesignElement;
  x: number;
  y: number;
  width: number;
  height: number;
}> {
  if (projectionMode === "dependencies" && dependencyRoles.size > 0) {
    return buildDependencyViewLayout(elements, dependencyRoles, activeBranchElement);
  }

  const people = elements.filter((element) => element.elementType.toLowerCase() === "person");
  const nonPeople = elements.filter((element) => element.elementType.toLowerCase() !== "person");
  const orderedElements = activeLevel === "context" ? [...people, ...nonPeople] : elements;
  const elementWidth = activeLevel === "context" ? 340 : 420;
  const elementHeight = activeLevel === "context" ? 190 : 230;
  const gapX = 120;
  const gapY = 130;
  const margin = 80;
  const maxColumns = activeLevel === "context" ? Math.max(orderedElements.length, 1) : 3;

  return orderedElements.map((element, index) => {
    const column = activeLevel === "context" ? index : index % maxColumns;
    const row = activeLevel === "context" ? 0 : Math.floor(index / maxColumns);
    const branchInset = activeBranchElement?.elementType.toLowerCase() === "container" ? 20 : 0;

    return {
      element,
      x: margin + branchInset + column * (elementWidth + gapX),
      y: margin + row * (elementHeight + gapY),
      width: elementWidth,
      height: elementHeight,
    };
  });
}

function buildDependencyViewLayout(
  elements: DesignElement[],
  dependencyRoles: Map<string, DependencyNodeRole>,
  activeBranchElement: DesignElement | null,
): Array<{
  element: DesignElement;
  x: number;
  y: number;
  width: number;
  height: number;
}> {
  const elementWidth = 360;
  const elementHeight = 180;
  const gapX = 160;
  const gapY = 82;
  const margin = 80;
  const roleColumns: Record<DependencyNodeRole, number> = {
    incoming: 0,
    core: 1,
    outgoing: 2,
  };
  const roleRows = new Map<DependencyNodeRole, DesignElement[]>();

  elements
    .slice()
    .sort((left, right) => compareElementsForDependencyView(left, right, activeBranchElement))
    .forEach((element) => {
      const role = dependencyRoles.get(element.externalId) ?? "core";
      const current = roleRows.get(role) ?? [];
      roleRows.set(role, [...current, element]);
    });

  return Array.from(roleRows.entries()).flatMap(([role, roleElements]) =>
    roleElements.map((element, index) => ({
      element,
      x: margin + roleColumns[role] * (elementWidth + gapX),
      y: margin + index * (elementHeight + gapY),
      width: elementWidth,
      height: elementHeight,
    })),
  );
}

function compareElementsForDependencyView(
  left: DesignElement,
  right: DesignElement,
  activeBranchElement: DesignElement | null,
): number {
  const leftActive = activeBranchElement?.externalId === left.externalId ? 0 : 1;
  const rightActive = activeBranchElement?.externalId === right.externalId ? 0 : 1;

  return (
    leftActive - rightActive ||
    elementTypeRank(left) - elementTypeRank(right) ||
    (left.parentExternalId ?? "").localeCompare(right.parentExternalId ?? "") ||
    left.name.localeCompare(right.name) ||
    left.externalId.localeCompare(right.externalId)
  );
}

function elementTypeRank(element: DesignElement): number {
  switch (element.elementType.toLowerCase()) {
    case "person":
      return 0;
    case "software system":
      return 1;
    case "container":
      return 2;
    case "component":
      return 3;
    default:
      return 4;
  }
}

function compareRelationshipStable(
  left: DesignRelationship,
  right: DesignRelationship,
  elements: DesignElement[],
): number {
  const leftSource = elements.find((element) => element.externalId === left.sourceExternalId);
  const rightSource = elements.find((element) => element.externalId === right.sourceExternalId);
  const leftDestination = elements.find((element) => element.externalId === left.destinationExternalId);
  const rightDestination = elements.find((element) => element.externalId === right.destinationExternalId);

  return (
    (leftSource?.name ?? left.sourceExternalId).localeCompare(rightSource?.name ?? right.sourceExternalId) ||
    (leftDestination?.name ?? left.destinationExternalId).localeCompare(
      rightDestination?.name ?? right.destinationExternalId,
    ) ||
    left.description.localeCompare(right.description) ||
    left.externalId.localeCompare(right.externalId)
  );
}

function buildDependencyProjectionSource(
  activeBranchElement: DesignElement | null,
  visibleElements: DesignElement[],
  visibleRelationships: DesignRelationship[],
  hiddenRelationshipCount: number,
  allElements: DesignElement[],
): string {
  const elementById = new Map(allElements.map((element) => [element.externalId, element]));
  const lines = [
    `Dependency View: ${activeBranchElement?.name ?? "Adashi"}`,
    "",
    "Visible elements:",
    ...visibleElements
      .slice()
      .sort((left, right) => compareElementsForDependencyView(left, right, activeBranchElement))
      .map((element) => `- ${element.name} [${element.externalId}] (${element.elementType}) :: ${element.technology || "no technology"}`),
    "",
    "Visible relationships:",
  ];

  if (visibleRelationships.length === 0) {
    lines.push("- none");
  } else {
    visibleRelationships.forEach((relationship) => {
      const source = elementById.get(relationship.sourceExternalId);
      const destination = elementById.get(relationship.destinationExternalId);
      const technology = relationship.technology ? ` (${relationship.technology})` : "";
      lines.push(
        `- ${source?.name ?? relationship.sourceExternalId} -> ${destination?.name ?? relationship.destinationExternalId}: ${relationship.description}${technology}`,
      );
    });
  }

  if (hiddenRelationshipCount > 0) {
    lines.push("", `${hiddenRelationshipCount} lower-priority relationship(s) hidden in this projection.`);
  }

  return lines.join("\n");
}

function elementToStructurizrJson(
  element: DesignElement,
  elements: DesignElement[],
  relationships: DesignRelationship[],
): StructurizrElementJson {
  const children = elements.filter((candidate) => candidate.parentExternalId === element.externalId);
  const value: StructurizrElementJson = {
    id: element.externalId,
    canonicalName: buildCanonicalName(element, elements),
    description: summarizeDiagramDescription(element.description, 82),
    location: hasTag(element.tags, "External") ? "External" : "Internal",
    name: element.name,
    parentId: element.parentExternalId ?? null,
    relationships: relationships
      .filter((relationship) => relationship.sourceExternalId === element.externalId)
      .map((relationship) => ({
        id: relationship.externalId,
        sourceId: relationship.sourceExternalId,
        destinationId: relationship.destinationExternalId,
        description: summarizeDiagramDescription(relationship.description, 58),
        technology: relationship.technology,
        tags: mergeStructurizrTags(relationship.tags, ["Relationship"]),
      })),
    tags: buildStructurizrElementTags(element),
    technology: element.technology,
    type: element.elementType,
  };

  if (children.length > 0) {
    const childValues = children.map((child) => elementToStructurizrJson(child, elements, relationships));

    if (element.elementType.toLowerCase() === "software system") {
      value.containers = childValues;
    } else {
      value.components = childValues;
    }
  }

  return value;
}

function buildCanonicalName(element: DesignElement, elements: DesignElement[]): string {
  return `/${buildBreadcrumb(element, elements)
    .map((part) => part.name)
    .join("/")}`;
}

function buildStructurizrElementTags(element: DesignElement): string {
  return mergeStructurizrTags(element.tags, ["Element", element.elementType]);
}

function mergeStructurizrTags(tags: string, requiredTags: string[]): string {
  const values = new Map<string, string>();

  [...requiredTags, ...tags.split(",")]
    .map((tag) => tag.trim())
    .filter(Boolean)
    .forEach((tag) => values.set(tag.toLowerCase(), tag));

  return Array.from(values.values()).join(",");
}

function summarizeDiagramDescription(description: string, maxLength = 82): string {
  const normalized = description.replace(/\s+/g, " ").trim();

  if (normalized.length <= maxLength) {
    return normalized;
  }

  const sentenceEnd = normalized.search(/[.!?]\s/);

  if (sentenceEnd > 32 && sentenceEnd <= maxLength) {
    return normalized.slice(0, sentenceEnd + 1);
  }

  const clipped = normalized.slice(0, Math.max(0, maxLength - 2));
  const wordBoundary = clipped.lastIndexOf(" ");

  return `${clipped.slice(0, wordBoundary > maxLength * 0.55 ? wordBoundary : clipped.length).trim()}...`;
}

function hasTag(tags: string, tag: string): boolean {
  return tags
    .split(",")
    .map((part) => part.trim().toLowerCase())
    .includes(tag.toLowerCase());
}

function sanitizeViewKey(value: string): string {
  const sanitized = value.replace(/[^a-zA-Z0-9_-]/g, "_");
  return sanitized || "root";
}

function DesignInspector({
  elements,
  projectId,
  relationships,
  selectedElement,
  selectedRelationship,
  branchChildren,
  branchRelationships,
  onChange,
  onError,
  onJumpToFeatures,
}: {
  elements: DesignElement[];
  projectId: string;
  relationships: DesignRelationship[];
  selectedElement: DesignElement | null;
  selectedRelationship: DesignRelationship | null;
  branchChildren: DesignElement[];
  branchRelationships: DesignRelationship[];
  onChange: (payload: DashboardPayload) => void;
  onError: (message: string) => void;
  onJumpToFeatures: () => void;
}) {
  return (
    <aside className="design-inspector-panel">
      {selectedElement ? (
        <ElementInspector
          element={selectedElement}
          elements={elements}
          projectId={projectId}
          relationships={relationships}
          onChange={onChange}
          onError={onError}
          onJumpToFeatures={onJumpToFeatures}
        />
      ) : selectedRelationship ? (
        <RelationshipInspector
          elements={elements}
          projectId={projectId}
          relationship={selectedRelationship}
          onChange={onChange}
          onError={onError}
        />
      ) : (
        <BranchInspector branchChildren={branchChildren} branchRelationships={branchRelationships} elements={elements} />
      )}
    </aside>
  );
}

function BranchInspector({
  branchChildren,
  branchRelationships,
  elements,
}: {
  branchChildren: DesignElement[];
  branchRelationships: DesignRelationship[];
  elements: DesignElement[];
}) {
  return (
    <>
      <div className="inspector-heading">
        <div>
          <p className="eyebrow">Current Branch</p>
          <h3>Tree Overview</h3>
        </div>
        <span>C4</span>
      </div>

      <InspectorSection title="Child Nodes">
        {branchChildren.length === 0 ? (
          <div className="empty-state compact">No child nodes on this branch.</div>
        ) : (
          <div className="branch-fact-list">
            {branchChildren.map((child) => (
              <span key={child.externalId}>{child.name}</span>
            ))}
          </div>
        )}
      </InspectorSection>

      <InspectorSection title="Branch Relations">
        {branchRelationships.length === 0 ? (
          <div className="empty-state compact">No relationships on this branch.</div>
        ) : (
          <div className="branch-fact-list">
            {branchRelationships.map((relationship) => {
              const source = elements.find((element) => element.externalId === relationship.sourceExternalId);
              const destination = elements.find((element) => element.externalId === relationship.destinationExternalId);

              return (
                <span key={relationship.externalId}>
                  {source?.name ?? relationship.sourceExternalId} {"->"} {destination?.name ?? relationship.destinationExternalId}
                </span>
              );
            })}
          </div>
        )}
      </InspectorSection>
    </>
  );
}

function ElementInspector({
  element,
  elements,
  projectId,
  relationships,
  onChange,
  onError,
  onJumpToFeatures,
}: {
  element: DesignElement;
  elements: DesignElement[];
  projectId: string;
  relationships: DesignRelationship[];
  onChange: (payload: DashboardPayload) => void;
  onError: (message: string) => void;
  onJumpToFeatures: () => void;
}) {
  const outgoing = relationships.filter((relationship) => relationship.sourceExternalId === element.externalId);
  const incoming = relationships.filter((relationship) => relationship.destinationExternalId === element.externalId);

  function save(changes: Partial<DesignElement>) {
    const updatedElement = { ...element, ...changes };

    invoke<DashboardPayload>("update_design_element", {
      input: {
        projectId,
        externalId: updatedElement.externalId,
        name: updatedElement.name,
        description: updatedElement.description,
        technology: updatedElement.technology,
        tags: updatedElement.tags,
      },
    })
      .then(onChange)
      .catch((reason) => onError(String(reason)));
  }

  return (
    <>
      <div className="inspector-heading">
        <div>
          <p className="eyebrow">Design Entity</p>
          <h3>{element.name}</h3>
        </div>
        <span>{element.elementType}</span>
      </div>

      <div className="inspector-form">
        <label>
          <span>Name</span>
          <input
            defaultValue={element.name}
            key={`element-name-${element.externalId}-${element.name}`}
            onBlur={(event) => save({ name: event.target.value })}
          />
        </label>
        <label>
          <span>Description</span>
          <textarea
            defaultValue={element.description}
            key={`element-description-${element.externalId}-${element.description}`}
            onBlur={(event) => save({ description: event.target.value })}
          />
        </label>
        <label>
          <span>Technology</span>
          <input
            defaultValue={element.technology}
            key={`element-technology-${element.externalId}-${element.technology}`}
            onBlur={(event) => save({ technology: event.target.value })}
          />
        </label>
        <label>
          <span>Tags</span>
          <input
            defaultValue={element.tags}
            key={`element-tags-${element.externalId}-${element.tags}`}
            onBlur={(event) => save({ tags: event.target.value })}
          />
        </label>
      </div>

      <InspectorSection title="Connected Facts">
        <RelationshipSummary direction="Outgoing" elements={elements} relationships={outgoing} />
        <RelationshipSummary direction="Incoming" elements={elements} relationships={incoming} />
      </InspectorSection>

      <AddRelationshipForm elements={elements} projectId={projectId} sourceElement={element} onChange={onChange} onError={onError} />

      <InspectorSection title="AI Edit Prompt">
        <button className="wide-tool-button" onClick={onJumpToFeatures} type="button">
          <Wand2 size={17} />
          Ask AI to change this design
        </button>
      </InspectorSection>
    </>
  );
}

function RelationshipInspector({
  elements,
  projectId,
  relationship,
  onChange,
  onError,
}: {
  elements: DesignElement[];
  projectId: string;
  relationship: DesignRelationship;
  onChange: (payload: DashboardPayload) => void;
  onError: (message: string) => void;
}) {
  const source = elements.find((element) => element.externalId === relationship.sourceExternalId);
  const destination = elements.find((element) => element.externalId === relationship.destinationExternalId);

  function save(changes: Partial<DesignRelationship>) {
    const updatedRelationship = { ...relationship, ...changes };

    invoke<DashboardPayload>("update_design_relationship", {
      input: {
        projectId,
        externalId: updatedRelationship.externalId,
        description: updatedRelationship.description,
        technology: updatedRelationship.technology,
        tags: updatedRelationship.tags,
      },
    })
      .then(onChange)
      .catch((reason) => onError(String(reason)));
  }

  return (
    <>
      <div className="inspector-heading">
        <div>
          <p className="eyebrow">Relationship</p>
          <h3>
            {source?.name ?? relationship.sourceExternalId} {"->"} {destination?.name ?? relationship.destinationExternalId}
          </h3>
        </div>
        <span>{relationship.technology || "link"}</span>
      </div>

      <div className="inspector-form">
        <label>
          <span>Description</span>
          <textarea
            defaultValue={relationship.description}
            key={`relationship-description-${relationship.externalId}-${relationship.description}`}
            onBlur={(event) => save({ description: event.target.value })}
          />
        </label>
        <label>
          <span>Technology</span>
          <input
            defaultValue={relationship.technology}
            key={`relationship-technology-${relationship.externalId}-${relationship.technology}`}
            onBlur={(event) => save({ technology: event.target.value })}
          />
        </label>
        <label>
          <span>Tags</span>
          <input
            defaultValue={relationship.tags}
            key={`relationship-tags-${relationship.externalId}-${relationship.tags}`}
            onBlur={(event) => save({ tags: event.target.value })}
          />
        </label>
      </div>

      <InspectorSection title="Semantic Id">
        <code>{`design://relationship/${relationship.externalId}`}</code>
      </InspectorSection>
    </>
  );
}

function RelationshipSummary({
  direction,
  elements,
  relationships,
}: {
  direction: string;
  elements: DesignElement[];
  relationships: DesignRelationship[];
}) {
  return (
    <div className="relationship-summary">
      <strong>{direction}</strong>
      {relationships.length === 0 ? (
        <span>None</span>
      ) : (
        relationships.map((relationship) => {
          const source = elements.find((element) => element.externalId === relationship.sourceExternalId);
          const destination = elements.find((element) => element.externalId === relationship.destinationExternalId);

          return (
            <span key={relationship.externalId}>
              {source?.name ?? relationship.sourceExternalId} {"->"} {destination?.name ?? relationship.destinationExternalId}
            </span>
          );
        })
      )}
    </div>
  );
}

function AddRelationshipForm({
  elements,
  projectId,
  sourceElement,
  onChange,
  onError,
}: {
  elements: DesignElement[];
  projectId: string;
  sourceElement: DesignElement;
  onChange: (payload: DashboardPayload) => void;
  onError: (message: string) => void;
}) {
  const [destinationExternalId, setDestinationExternalId] = React.useState(
    elements.find((element) => element.externalId !== sourceElement.externalId)?.externalId ?? "",
  );
  const [description, setDescription] = React.useState("");
  const [technology, setTechnology] = React.useState("");

  React.useEffect(() => {
    if (!destinationExternalId || destinationExternalId === sourceElement.externalId) {
      setDestinationExternalId(elements.find((element) => element.externalId !== sourceElement.externalId)?.externalId ?? "");
    }
  }, [destinationExternalId, elements, sourceElement.externalId]);

  function submit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();

    invoke<DashboardPayload>("create_design_relationship", {
      input: {
        projectId,
        sourceExternalId: sourceElement.externalId,
        destinationExternalId,
        description,
        technology,
        tags: "Relationship",
      },
    })
      .then((updatedPayload) => {
        setDescription("");
        setTechnology("");
        onChange(updatedPayload);
      })
      .catch((reason) => onError(String(reason)));
  }

  return (
    <InspectorSection title="Add Relation">
      <form className="add-relationship-form" onSubmit={submit}>
        <label>
          <span>To</span>
          <select value={destinationExternalId} onChange={(event) => setDestinationExternalId(event.target.value)}>
            {elements
              .filter((element) => element.externalId !== sourceElement.externalId)
              .map((element) => (
                <option key={element.externalId} value={element.externalId}>
                  {element.name}
                </option>
              ))}
          </select>
        </label>
        <label>
          <span>Description</span>
          <input value={description} onChange={(event) => setDescription(event.target.value)} />
        </label>
        <label>
          <span>Technology</span>
          <input value={technology} onChange={(event) => setTechnology(event.target.value)} />
        </label>
        <button type="submit">
          <Link2 size={17} />
          Add
        </button>
      </form>
    </InspectorSection>
  );
}

function InspectorSection({ title, children }: React.PropsWithChildren<{ title: string }>) {
  return (
    <section className="inspector-section">
      <h4>{title}</h4>
      {children}
    </section>
  );
}

function UmlArtifactTabs({
  activeKey,
  artifacts,
  onSelect,
}: {
  activeKey: string | null;
  artifacts: DesignDiagram[];
  onSelect: (key: string) => void;
}) {
  if (artifacts.length <= 1) {
    return null;
  }

  return (
    <div className="uml-artifact-tabs" role="tablist" aria-label="UML artifacts">
      {artifacts.map((artifact) => {
        const Icon = iconForArtifactRole(artifact.artifactRole);

        return (
          <button
            aria-selected={artifact.key === activeKey}
            className={artifact.key === activeKey ? "active" : ""}
            key={artifact.key}
            onClick={() => onSelect(artifact.key)}
            role="tab"
            title={artifact.title}
            type="button"
          >
            <Icon size={15} />
            <span>{artifact.artifactLabel}</span>
          </button>
        );
      })}
    </div>
  );
}

function iconForArtifactRole(role: string): React.ElementType {
  switch (role) {
    case "primary-structure":
      return Braces;
    case "interaction":
      return PlayCircle;
    case "workflow":
      return GitBranch;
    case "lifecycle":
      return Activity;
    default:
      return ScrollText;
  }
}

function SourcePanel({ diagram, source, sourceKind }: { diagram?: DesignDiagram; source: string; sourceKind: string }) {
  return (
    <section className="source-panel">
      <div className="source-heading">
        <div>
          <p className="eyebrow">Design Source</p>
          <h3>{diagram?.title ?? "No diagram source"}</h3>
        </div>
        <span>{sourceKind}</span>
      </div>
      <pre>{source}</pre>
    </section>
  );
}

function SettingsView({
  activeProjectId,
  fixedHookPrompts,
  settings,
  onAdd,
  onDashboardChange,
  onDelete,
  onError,
}: {
  activeProjectId: string;
  fixedHookPrompts: FixedHookPrompt[];
  settings: AppSettings;
  onAdd: (settings: AppSettings, projectId: string) => void;
  onDashboardChange: (payload: DashboardPayload) => void;
  onDelete: (settings: AppSettings) => void;
  onError: (message: string) => void;
}) {
  const [name, setName] = React.useState("");
  const [folder, setFolder] = React.useState("");
  const [isPickingFolder, setIsPickingFolder] = React.useState(false);

  function submit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    invoke<AppSettings>("add_project", { name: name.trim(), folder: folder.trim() })
      .then((updatedSettings) => {
        const projectId = updatedSettings.lastActiveProjectId ?? updatedSettings.projects[0]?.id;
        setName("");
        setFolder("");
        onAdd(updatedSettings, projectId);
      })
      .catch((reason) => onError(String(reason)));
  }

  function browseFolder() {
    setIsPickingFolder(true);
    invoke<string | null>("pick_project_folder", { currentFolder: folder.trim() || null })
      .then((selectedFolder) => {
        if (selectedFolder) {
          setFolder(selectedFolder);
        }
      })
      .catch((reason) => onError(String(reason)))
      .finally(() => setIsPickingFolder(false));
  }

  function remove(projectId: string) {
    invoke<AppSettings>("delete_project", { projectId })
      .then(onDelete)
      .catch((reason) => onError(String(reason)));
  }

  function saveFixedHookPrompt(hookPrompt: FixedHookPrompt, prompt: string) {
    invoke<DashboardPayload>("update_fixed_hook_prompt", {
      input: {
        projectId: activeProjectId,
        key: hookPrompt.key,
        prompt,
      },
    })
      .then(onDashboardChange)
      .catch((reason) => onError(String(reason)));
  }

  return (
    <section className="settings-grid">
      <section className="data-panel settings-panel">
        <h3>Projects</h3>
        <div className="project-list">
          {settings.projects.map((project) => (
            <article className="project-row" key={project.id}>
              <Folder size={18} />
              <div>
                <strong>{project.name}</strong>
                <span>{project.id}</span>
                <code>{project.folder}</code>
              </div>
              <button
                aria-label={`Delete ${project.name}`}
                disabled={project.id === activeProjectId && settings.projects.length === 1}
                onClick={() => remove(project.id)}
                title="Remove from Adashi settings"
                type="button"
              >
                <Trash2 size={17} />
              </button>
            </article>
          ))}
        </div>
      </section>

      <section className="data-panel settings-panel">
        <h3>Add Project</h3>
        <form className="settings-form" onSubmit={submit}>
          <label>
            <span>Name</span>
            <input value={name} onChange={(event) => setName(event.target.value)} placeholder="Project name" />
          </label>
          <label>
            <span>Folder</span>
            <div className="folder-picker-row">
              <input
                value={folder}
                onChange={(event) => setFolder(event.target.value)}
                placeholder="C:\src\MyProject"
              />
              <button disabled={isPickingFolder} onClick={browseFolder} title="Browse for project folder" type="button">
                <Folder size={17} />
                Browse
              </button>
            </div>
          </label>
          <button type="submit">
            <Plus size={17} />
            Add
          </button>
        </form>
      </section>

      <section className="data-panel settings-panel settings-design-protocol-panel">
        <div className="rules-panel-heading">
          <h3>Fixed Design Hooks</h3>
        </div>
        <div className="fixed-hook-editor-list">
          {fixedHookPrompts.map((hookPrompt) => (
            <section className="fixed-hook-editor" key={hookPrompt.key}>
              <div className="rules-panel-heading">
                <div>
                  <p className="eyebrow">
                    {hookPrompt.intend} / {hookPrompt.hook}
                  </p>
                  <h4>{hookPrompt.title}</h4>
                </div>
              </div>
              <MarkdownEditor
                key={`${hookPrompt.key}-${hookPrompt.updatedAt}`}
                value={hookPrompt.prompt}
                onBlur={(prompt) => saveFixedHookPrompt(hookPrompt, prompt)}
                placeholder="Write the fixed run-start hook prompt..."
                minHeight="260px"
                maxHeight="420px"
                height="320px"
              />
            </section>
          ))}
        </div>
      </section>
    </section>
  );
}

function TasksView({
  projectId,
  tasks,
  designElements,
  designRelationships,
  diagrams,
  postTaskCommands,
  onChange,
  onError,
  onOpenDesignLink,
}: {
  projectId: string;
  tasks: Task[];
  designElements: DesignElement[];
  designRelationships: DesignRelationship[];
  diagrams: DesignDiagram[];
  postTaskCommands: Command[];
  onChange: (payload: DashboardPayload) => void;
  onError: (message: string) => void;
  onOpenDesignLink: (link: TaskDesignSpecificationLink) => void;
}) {
  const [selectedTaskId, setSelectedTaskId] = React.useState<number | null>(tasks[0]?.id ?? null);
  const [visibleStates, setVisibleStates] = React.useState<Record<TaskState, boolean>>({
    open: true,
    finished: true,
    confirmed: false,
  });
  const [linkQuery, setLinkQuery] = React.useState("");
  const [completionMemo, setCompletionMemo] = React.useState("");
  const [createdFiles, setCreatedFiles] = React.useState("");
  const [changedFiles, setChangedFiles] = React.useState("");

  const visibleTasks = tasks.filter((task) => visibleStates[task.state]);
  const selectedTask =
    tasks.find((task) => task.id === selectedTaskId && visibleStates[task.state]) ?? visibleTasks[0] ?? null;
  const designLinkOptions = React.useMemo(
    () => buildTaskDesignLinkOptions(designElements, designRelationships, diagrams, linkQuery),
    [designElements, designRelationships, diagrams, linkQuery],
  );

  React.useEffect(() => {
    if (!selectedTask && visibleTasks[0]) {
      setSelectedTaskId(visibleTasks[0].id);
    }
  }, [selectedTask, visibleTasks]);

  React.useEffect(() => {
    setCompletionMemo(selectedTask?.completionMemo ?? "");
    setCreatedFiles((selectedTask?.createdFiles ?? []).join("\n"));
    setChangedFiles((selectedTask?.changedFiles ?? []).join("\n"));
  }, [selectedTask?.id]);

  function addTask() {
    invoke<DashboardPayload>("create_task", {
      input: {
        projectId,
        title: "New Task",
        description: "",
        designSpecificationLinks: [],
      },
    })
      .then((updatedPayload) => {
        const newestTask = updatedPayload.tasks.reduce<Task | null>(
          (newest, task) => (!newest || task.id > newest.id ? task : newest),
          null,
        );
        setSelectedTaskId(newestTask?.id ?? null);
        onChange(updatedPayload);
      })
      .catch((reason) => onError(String(reason)));
  }

  function updateTask(task: Task, changes: Partial<Pick<Task, "title" | "description" | "state">> & {
    designSpecificationLinks?: Array<Pick<TaskDesignSpecificationLink, "targetType" | "designExternalId">>;
  }) {
    invoke<DashboardPayload>("update_task", {
      input: {
        projectId,
        taskId: task.id,
        ...changes,
      },
    })
      .then((updatedPayload) => {
        setSelectedTaskId(task.id);
        onChange(updatedPayload);
      })
      .catch((reason) => onError(String(reason)));
  }

  function deleteSelectedTask(task: Task) {
    if (!window.confirm(`Delete Task Id ${task.id}: ${task.title}?`)) {
      return;
    }

    invoke<DashboardPayload>("delete_task", { projectId, taskId: task.id })
      .then((updatedPayload) => {
        setSelectedTaskId(updatedPayload.tasks.find((candidate) => visibleStates[candidate.state])?.id ?? null);
        onChange(updatedPayload);
      })
      .catch((reason) => onError(String(reason)));
  }

  function addDesignLink(task: Task, option: TaskDesignLinkOption) {
    if (task.designSpecificationLinks.some((link) => link.designExternalId === option.designExternalId)) {
      return;
    }

    updateTask(task, {
      designSpecificationLinks: [
        ...task.designSpecificationLinks.map(linkToInput),
        { targetType: option.targetType, designExternalId: option.designExternalId },
      ],
    });
    setLinkQuery("");
  }

  function removeDesignLink(task: Task, designExternalId: string) {
    updateTask(task, {
      designSpecificationLinks: task.designSpecificationLinks
        .filter((link) => link.designExternalId !== designExternalId)
        .map(linkToInput),
    });
  }

  function moveDesignLink(task: Task, index: number, direction: -1 | 1) {
    const nextLinks = task.designSpecificationLinks.map(linkToInput);
    const nextIndex = index + direction;

    if (nextIndex < 0 || nextIndex >= nextLinks.length) {
      return;
    }

    [nextLinks[index], nextLinks[nextIndex]] = [nextLinks[nextIndex], nextLinks[index]];
    updateTask(task, { designSpecificationLinks: nextLinks });
  }

  function finishSelectedTask(task: Task) {
    invoke<DashboardPayload>("finish_task", {
      input: {
        projectId,
        taskId: task.id,
        completionMemo,
        createdFiles: splitLines(createdFiles),
        changedFiles: splitLines(changedFiles),
      },
    })
      .then(onChange)
      .catch((reason) => onError(String(reason)));
  }

  function confirmSelectedTask(task: Task) {
    invoke<DashboardPayload>("confirm_task", { projectId, taskId: task.id })
      .then(onChange)
      .catch((reason) => onError(String(reason)));
  }

  return (
    <section className="task-workspace">
      <section className="task-list-panel">
        <div className="task-panel-heading">
          <h3>Tasks</h3>
          <button aria-label="Create task" onClick={addTask} title="Create task" type="button">
            <Plus size={17} />
          </button>
        </div>

        <div className="task-state-filters" aria-label="Task state filters">
          {(["open", "finished", "confirmed"] as TaskState[]).map((state) => (
            <label key={state}>
              <input
                checked={visibleStates[state]}
                onChange={(event) => setVisibleStates((current) => ({ ...current, [state]: event.target.checked }))}
                type="checkbox"
              />
              <span>{state}</span>
            </label>
          ))}
        </div>

        <div className="task-list">
          {visibleTasks.length === 0 ? (
            <div className="empty-state">No tasks match the current filters</div>
          ) : (
            visibleTasks.map((task) => (
              <button
                className={selectedTask?.id === task.id ? "task-list-item active" : "task-list-item"}
                key={task.id}
                onClick={() => setSelectedTaskId(task.id)}
                type="button"
              >
                <span className={`pill task-state-${task.state}`}>{task.state}</span>
                <strong>Task Id {task.id} {task.title}</strong>
                <small>{task.designSpecificationLinks.length} design links</small>
              </button>
            ))
          )}
        </div>

        <section className="post-task-command-panel">
          <h4>Post-Task Commands</h4>
          {postTaskCommands.map((command) => (
            <div className="command-row compact" key={command.id}>
              <PlayCircle size={16} />
              <div>
                <span>{command.label}</span>
                <code>{command.command}</code>
              </div>
            </div>
          ))}
        </section>
      </section>

      <section className="task-detail-panel">
        {selectedTask ? (
          <>
            <div className="task-detail-heading">
              <div>
                <p className="eyebrow">Task Id {selectedTask.id}</p>
                <h3>{selectedTask.title}</h3>
              </div>
              <div className="task-detail-actions">
                {selectedTask.state === "finished" ? (
                  <button aria-label="Confirm task" onClick={() => confirmSelectedTask(selectedTask)} title="Confirm task" type="button">
                    <Check size={17} />
                  </button>
                ) : null}
                <button
                  aria-label={`Delete Task Id ${selectedTask.id}`}
                  className="danger-icon-button"
                  onClick={() => deleteSelectedTask(selectedTask)}
                  title="Delete task"
                  type="button"
                >
                  <Trash2 size={17} />
                </button>
              </div>
            </div>

            <div className="task-editor-form">
              <label>
                <span>Title</span>
                <input
                  defaultValue={selectedTask.title}
                  key={`task-title-${selectedTask.id}`}
                  onBlur={(event) => updateTask(selectedTask, { title: event.target.value })}
                />
              </label>

              <label>
                <span>State</span>
                <select
                  value={selectedTask.state}
                  onChange={(event) => updateTask(selectedTask, { state: event.target.value as TaskState })}
                >
                  <option value="open">open</option>
                  <option value="finished">finished</option>
                  <option value="confirmed">confirmed</option>
                </select>
              </label>

              <label className="task-description-label">
                <span>Description</span>
                <textarea
                  defaultValue={selectedTask.description}
                  key={`task-description-${selectedTask.id}`}
                  onBlur={(event) => updateTask(selectedTask, { description: event.target.value })}
                />
              </label>
            </div>

            <section className="task-detail-section">
              <div className="task-section-heading">
                <h4>Design Specification Links</h4>
              </div>

              <div className="task-link-list">
                {selectedTask.designSpecificationLinks.length === 0 ? (
                  <div className="empty-state compact">No design specifications linked</div>
                ) : (
                  selectedTask.designSpecificationLinks.map((link, index) => (
                    <div className="task-link-row" key={link.id}>
                      <button
                        aria-label={`Open ${link.title} in Design`}
                        onClick={() => onOpenDesignLink(link)}
                        title="Open in Design"
                        type="button"
                      >
                        <ExternalLink size={16} />
                      </button>
                      <div>
                        <strong>{link.title}</strong>
                        <span>{link.targetType} / {link.designExternalId}</span>
                      </div>
                      <button
                        aria-label="Move link up"
                        disabled={index === 0}
                        onClick={() => moveDesignLink(selectedTask, index, -1)}
                        title="Move up"
                        type="button"
                      >
                        <ArrowUp size={16} />
                      </button>
                      <button
                        aria-label="Move link down"
                        disabled={index === selectedTask.designSpecificationLinks.length - 1}
                        onClick={() => moveDesignLink(selectedTask, index, 1)}
                        title="Move down"
                        type="button"
                      >
                        <ArrowDown size={16} />
                      </button>
                      <button
                        aria-label={`Remove ${link.title}`}
                        onClick={() => removeDesignLink(selectedTask, link.designExternalId)}
                        title="Remove link"
                        type="button"
                      >
                        <X size={16} />
                      </button>
                    </div>
                  ))
                )}
              </div>

              <label className="task-link-search">
                <Search size={16} />
                <input
                  value={linkQuery}
                  onChange={(event) => setLinkQuery(event.target.value)}
                  placeholder="Search C4 elements, relationships, or UML"
                />
              </label>
              {linkQuery.trim() ? (
                <div className="task-link-results">
                  {designLinkOptions.map((option) => (
                    <button key={`${option.targetType}-${option.designExternalId}`} onClick={() => addDesignLink(selectedTask, option)} type="button">
                      <Link2 size={15} />
                      <span>{option.title}</span>
                      <code>{option.targetType}</code>
                    </button>
                  ))}
                </div>
              ) : null}
            </section>

            <section className="task-detail-section">
              <div className="task-section-heading">
                <h4>Completion</h4>
                {selectedTask.state !== "confirmed" ? (
                  <button onClick={() => finishSelectedTask(selectedTask)} type="button">
                    <CheckCircle2 size={17} />
                    Finish
                  </button>
                ) : null}
              </div>
              <div className="task-completion-grid">
                <label>
                  <span>Memo</span>
                  <textarea value={completionMemo} onChange={(event) => setCompletionMemo(event.target.value)} />
                </label>
                <label>
                  <span>Created Files</span>
                  <textarea value={createdFiles} onChange={(event) => setCreatedFiles(event.target.value)} />
                </label>
                <label>
                  <span>Changed Files</span>
                  <textarea value={changedFiles} onChange={(event) => setChangedFiles(event.target.value)} />
                </label>
              </div>
            </section>
          </>
        ) : (
          <div className="empty-state">Create or reveal a task to start editing</div>
        )}
      </section>
    </section>
  );
}

type TaskDesignLinkOption = {
  targetType: "element" | "relationship" | "uml";
  designExternalId: string;
  title: string;
  summary: string;
};

function buildTaskDesignLinkOptions(
  elements: DesignElement[],
  relationships: DesignRelationship[],
  diagrams: DesignDiagram[],
  query: string,
): TaskDesignLinkOption[] {
  const terms = query
    .trim()
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean);

  if (terms.length === 0) {
    return [];
  }

  const options: TaskDesignLinkOption[] = [
    ...elements.map((element) => ({
      targetType: "element" as const,
      designExternalId: element.externalId,
      title: element.name,
      summary: `${element.externalId} ${element.elementType} ${element.description} ${element.technology} ${element.tags}`,
    })),
    ...relationships.map((relationship) => ({
      targetType: "relationship" as const,
      designExternalId: relationship.externalId,
      title: relationship.description,
      summary: `${relationship.externalId} ${relationship.sourceExternalId} ${relationship.destinationExternalId} ${relationship.technology} ${relationship.tags}`,
    })),
    ...diagrams
      .filter((diagram) => diagram.kind === "mermaid")
      .map((diagram) => ({
        targetType: "uml" as const,
        designExternalId: diagram.key,
        title: diagram.title,
        summary: `${diagram.key} ${diagram.diagramType} ${diagram.artifactLabel} ${diagram.attachedToExternalId ?? ""}`,
      })),
  ];

  return options
    .filter((option) => {
      const haystack = `${option.title} ${option.summary}`.toLowerCase();
      return terms.every((term) => haystack.includes(term));
    })
    .slice(0, 12);
}

function linkToInput(link: TaskDesignSpecificationLink): Pick<TaskDesignSpecificationLink, "targetType" | "designExternalId"> {
  return {
    targetType: link.targetType,
    designExternalId: link.designExternalId,
  };
}

function splitLines(value: string): string[] {
  return value
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
}

function QaView({
  projectId,
  qaChecks,
  qaJobs,
  tasks,
  designElements,
  designRelationships,
  diagrams,
  onChange,
  onError,
  onOpenDesignLink,
}: {
  projectId: string;
  qaChecks: QaCheck[];
  qaJobs: QaJob[];
  tasks: Task[];
  designElements: DesignElement[];
  designRelationships: DesignRelationship[];
  diagrams: DesignDiagram[];
  onChange: (payload: DashboardPayload) => void;
  onError: (message: string) => void;
  onOpenDesignLink: (link: QaJobDesignLink) => void;
}) {
  const [selectedJobId, setSelectedJobId] = React.useState<number | null>(qaJobs[0]?.id ?? null);
  const [visibleStates, setVisibleStates] = React.useState<Record<QaDerivedState, boolean>>({
    green: true,
    red: true,
    "needs-rerun": true,
    running: true,
  });
  const [showDisabled, setShowDisabled] = React.useState(false);
  const [tagFilter, setTagFilter] = React.useState("");
  const [designQuery, setDesignQuery] = React.useState("");
  const [taskQuery, setTaskQuery] = React.useState("");
  const [selectedJobRunId, setSelectedJobRunId] = React.useState<number | null>(qaJobs[0]?.runHistory[0]?.id ?? null);
  const [running, setRunning] = React.useState(false);

  const tagTerms = tagFilter
    .trim()
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean);
  const visibleJobs = qaJobs.filter((job) => {
    if (!visibleStates[job.derivedState]) {
      return false;
    }
    if (!showDisabled && !job.enabled) {
      return false;
    }
    return tagTerms.every((term) => job.tags.some((tag) => tag.toLowerCase().includes(term)));
  });
  const selectedJob =
    qaJobs.find((job) => job.id === selectedJobId && visibleJobs.some((visibleJob) => visibleJob.id === job.id)) ??
    visibleJobs[0] ??
    null;
  const selectedJobRun =
    selectedJob?.runHistory.find((jobRun) => jobRun.id === selectedJobRunId) ?? selectedJob?.runHistory[0] ?? null;
  const designLinkOptions = React.useMemo(
    () => buildTaskDesignLinkOptions(designElements, designRelationships, diagrams, designQuery),
    [designElements, designRelationships, diagrams, designQuery],
  );
  const taskLinkOptions = React.useMemo(() => buildQaTaskLinkOptions(tasks, taskQuery), [tasks, taskQuery]);

  React.useEffect(() => {
    if (!selectedJob && visibleJobs[0]) {
      setSelectedJobId(visibleJobs[0].id);
    }
  }, [selectedJob, visibleJobs]);

  React.useEffect(() => {
    if (selectedJob?.runHistory.length && !selectedJobRun) {
      setSelectedJobRunId(selectedJob.runHistory[0].id);
    }
  }, [selectedJob, selectedJobRun]);

  function addJob() {
    invoke<DashboardPayload>("create_qa_job", {
      input: {
        projectId,
        name: "New QA Job",
        description: "",
        command: "cargo check --manifest-path src-tauri/Cargo.toml",
        workingDirectory: "",
        shell: "powershell",
        timeoutSeconds: 120,
        enabled: true,
        designSpecificationLinks: [],
        taskIds: [],
        tags: ["local"],
      },
    })
      .then((updatedPayload) => {
        const newestJob = updatedPayload.qaJobs.reduce<QaJob | null>(
          (newest, job) => (!newest || job.number > newest.number ? job : newest),
          null,
        );
        setSelectedJobId(newestJob?.id ?? null);
        onChange(updatedPayload);
      })
      .catch((reason) => onError(String(reason)));
  }

  function updateJob(
    job: QaJob,
    changes: Partial<
      Pick<QaJob, "name" | "description" | "command" | "workingDirectory" | "shell" | "timeoutSeconds" | "enabled">
    > & {
      designSpecificationLinks?: Array<Pick<QaJobDesignLink, "targetType" | "designExternalId">>;
      taskIds?: number[];
      tags?: string[];
    },
  ) {
    invoke<DashboardPayload>("update_qa_job", {
      input: {
        projectId,
        qaJobId: job.id,
        ...changes,
      },
    })
      .then((updatedPayload) => {
        setSelectedJobId(job.id);
        onChange(updatedPayload);
      })
      .catch((reason) => onError(String(reason)));
  }

  function deleteSelectedJob(job: QaJob) {
    if (!window.confirm(`Delete QA job #${job.number}: ${job.name}?`)) {
      return;
    }

    invoke<DashboardPayload>("delete_qa_job", { projectId, qaJobId: job.id })
      .then((updatedPayload) => {
        setSelectedJobId(updatedPayload.qaJobs.find((candidate) => candidate.enabled || showDisabled)?.id ?? null);
        onChange(updatedPayload);
      })
      .catch((reason) => onError(String(reason)));
  }

  function runQuery(query: QaJobQuery) {
    setRunning(true);
    invoke<DashboardPayload>("run_qa_jobs", {
      input: {
        projectId,
        query,
        triggerSource: "dashboard",
      },
      })
      .then((updatedPayload) => {
        const refreshedJob =
          selectedJobId === null ? null : updatedPayload.qaJobs.find((job) => job.id === selectedJobId);
        setSelectedJobRunId(refreshedJob?.runHistory[0]?.id ?? null);
        onChange(updatedPayload);
      })
      .catch((reason) => onError(String(reason)))
      .finally(() => setRunning(false));
  }

  function addDesignLink(job: QaJob, option: TaskDesignLinkOption) {
    if (job.designSpecificationLinks.some((link) => link.designExternalId === option.designExternalId)) {
      return;
    }

    updateJob(job, {
      designSpecificationLinks: [
        ...job.designSpecificationLinks.map(qaDesignLinkToInput),
        { targetType: option.targetType, designExternalId: option.designExternalId },
      ],
    });
    setDesignQuery("");
  }

  function removeDesignLink(job: QaJob, designExternalId: string) {
    updateJob(job, {
      designSpecificationLinks: job.designSpecificationLinks
        .filter((link) => link.designExternalId !== designExternalId)
        .map(qaDesignLinkToInput),
    });
  }

  function moveDesignLink(job: QaJob, index: number, direction: -1 | 1) {
    const nextLinks = job.designSpecificationLinks.map(qaDesignLinkToInput);
    const nextIndex = index + direction;
    if (nextIndex < 0 || nextIndex >= nextLinks.length) {
      return;
    }
    [nextLinks[index], nextLinks[nextIndex]] = [nextLinks[nextIndex], nextLinks[index]];
    updateJob(job, { designSpecificationLinks: nextLinks });
  }

  function addTaskLink(job: QaJob, task: Task) {
    if (job.taskLinks.some((link) => link.taskId === task.id)) {
      return;
    }

    updateJob(job, { taskIds: [...job.taskLinks.map((link) => link.taskId), task.id] });
    setTaskQuery("");
  }

  function removeTaskLink(job: QaJob, taskId: number) {
    updateJob(job, { taskIds: job.taskLinks.filter((link) => link.taskId !== taskId).map((link) => link.taskId) });
  }

  return (
    <section className="qa-workspace">
      <section className="qa-list-panel">
        <div className="task-panel-heading">
          <h3>QA Jobs</h3>
          <button aria-label="Create QA job" onClick={addJob} title="Create QA job" type="button">
            <Plus size={17} />
          </button>
        </div>

        <div className="qa-state-filters" aria-label="QA state filters">
          {(["green", "red", "needs-rerun", "running"] as QaDerivedState[]).map((state) => (
            <label key={state}>
              <input
                checked={visibleStates[state]}
                onChange={(event) => setVisibleStates((current) => ({ ...current, [state]: event.target.checked }))}
                type="checkbox"
              />
              <span>{state}</span>
            </label>
          ))}
        </div>

        <label className="task-link-search">
          <Search size={16} />
          <input value={tagFilter} onChange={(event) => setTagFilter(event.target.value)} placeholder="Filter tags" />
        </label>

        <label className="settings-toggle-row">
          <input checked={showDisabled} onChange={(event) => setShowDisabled(event.target.checked)} type="checkbox" />
          <span>disabled</span>
        </label>

        <div className="task-list">
          {visibleJobs.length === 0 ? (
            <div className="empty-state">No QA jobs match the current filters</div>
          ) : (
            visibleJobs.map((job) => (
              <button
                className={selectedJob?.id === job.id ? "task-list-item active" : "task-list-item"}
                key={job.id}
                onClick={() => setSelectedJobId(job.id)}
                type="button"
              >
                <span className={`pill qa-state-${job.derivedState}`}>{job.derivedState}</span>
                <strong>#{job.number} {job.name}</strong>
                <small>{job.tags.join(", ") || "untagged"}</small>
              </button>
            ))
          )}
        </div>

        <div className="qa-run-actions">
          <button disabled={running || visibleJobs.length === 0} onClick={() => runQuery({ jobIds: visibleJobs.map((job) => job.id) })} type="button">
            <PlayCircle size={16} />
            Visible
          </button>
          <button disabled={running} onClick={() => runQuery({ states: ["red", "needs-rerun"] })} type="button">
            <Activity size={16} />
            Red/Stale
          </button>
        </div>

        <section className="post-task-command-panel">
          <h4>Legacy Gates</h4>
          {qaChecks.map((check) => (
            <div className="command-row compact" key={check.id}>
              <CheckCircle2 size={16} />
              <div>
                <span>{check.label}</span>
                <code>{check.command}</code>
              </div>
            </div>
          ))}
        </section>
      </section>

      <section className="qa-detail-panel">
        {selectedJob ? (
          <>
            <div className="task-detail-heading">
              <div>
                <p className="eyebrow">QA #{selectedJob.number}</p>
                <h3>{selectedJob.name}</h3>
              </div>
              <div className="task-detail-actions">
                <button disabled={running} onClick={() => runQuery({ jobIds: [selectedJob.id] })} title="Run QA job" type="button">
                  <PlayCircle size={17} />
                </button>
                <button
                  aria-label={`Delete QA job #${selectedJob.number}`}
                  className="danger-icon-button"
                  onClick={() => deleteSelectedJob(selectedJob)}
                  title="Delete QA job"
                  type="button"
                >
                  <Trash2 size={17} />
                </button>
              </div>
            </div>

            <div className="qa-editor-form">
              <label>
                <span>Name</span>
                <input
                  defaultValue={selectedJob.name}
                  key={`qa-name-${selectedJob.id}`}
                  onBlur={(event) => updateJob(selectedJob, { name: event.target.value })}
                />
              </label>
              <label>
                <span>Shell</span>
                <select
                  value={selectedJob.shell}
                  onChange={(event) => updateJob(selectedJob, { shell: event.target.value })}
                >
                  <option value="powershell">powershell</option>
                  <option value="cmd">cmd</option>
                  <option value="pwsh">pwsh</option>
                  <option value="sh">sh</option>
                  <option value="bash">bash</option>
                </select>
              </label>
              <label>
                <span>Timeout</span>
                <input
                  defaultValue={selectedJob.timeoutSeconds}
                  key={`qa-timeout-${selectedJob.id}`}
                  min={1}
                  onBlur={(event) => updateJob(selectedJob, { timeoutSeconds: Number(event.target.value) || 120 })}
                  type="number"
                />
              </label>
              <label className="qa-enabled-toggle">
                <span>Enabled</span>
                <input
                  checked={selectedJob.enabled}
                  onChange={(event) => updateJob(selectedJob, { enabled: event.target.checked })}
                  type="checkbox"
                />
              </label>
              <label className="task-description-label">
                <span>Command</span>
                <textarea
                  defaultValue={selectedJob.command}
                  key={`qa-command-${selectedJob.id}`}
                  onBlur={(event) => updateJob(selectedJob, { command: event.target.value })}
                />
              </label>
              <label>
                <span>Working Directory</span>
                <input
                  defaultValue={selectedJob.workingDirectory}
                  key={`qa-cwd-${selectedJob.id}`}
                  onBlur={(event) => updateJob(selectedJob, { workingDirectory: event.target.value })}
                  placeholder="project root"
                />
              </label>
              <label>
                <span>Tags</span>
                <input
                  defaultValue={selectedJob.tags.join(", ")}
                  key={`qa-tags-${selectedJob.id}`}
                  onBlur={(event) => updateJob(selectedJob, { tags: splitCommaList(event.target.value) })}
                />
              </label>
              <label className="task-description-label">
                <span>Description</span>
                <textarea
                  defaultValue={selectedJob.description}
                  key={`qa-description-${selectedJob.id}`}
                  onBlur={(event) => updateJob(selectedJob, { description: event.target.value })}
                />
              </label>
            </div>

            <section className="qa-link-grid">
              <section className="task-detail-section">
                <div className="task-section-heading">
                  <h4>Design Links</h4>
                </div>
                <div className="task-link-list">
                  {selectedJob.designSpecificationLinks.length === 0 ? (
                    <div className="empty-state compact">No design specifications linked</div>
                  ) : (
                    selectedJob.designSpecificationLinks.map((link, index) => (
                      <div className="task-link-row" key={link.id}>
                        <button onClick={() => onOpenDesignLink(link)} title="Open in Design" type="button">
                          <ExternalLink size={16} />
                        </button>
                        <div>
                          <strong>{link.title}</strong>
                          <span>{link.targetType} / {link.designExternalId}</span>
                        </div>
                        <button disabled={index === 0} onClick={() => moveDesignLink(selectedJob, index, -1)} title="Move up" type="button">
                          <ArrowUp size={16} />
                        </button>
                        <button
                          disabled={index === selectedJob.designSpecificationLinks.length - 1}
                          onClick={() => moveDesignLink(selectedJob, index, 1)}
                          title="Move down"
                          type="button"
                        >
                          <ArrowDown size={16} />
                        </button>
                        <button onClick={() => removeDesignLink(selectedJob, link.designExternalId)} title="Remove link" type="button">
                          <X size={16} />
                        </button>
                      </div>
                    ))
                  )}
                </div>
                <label className="task-link-search">
                  <Search size={16} />
                  <input value={designQuery} onChange={(event) => setDesignQuery(event.target.value)} placeholder="Search design" />
                </label>
                {designQuery.trim() ? (
                  <div className="task-link-results">
                    {designLinkOptions.map((option) => (
                      <button key={`${option.targetType}-${option.designExternalId}`} onClick={() => addDesignLink(selectedJob, option)} type="button">
                        <Link2 size={15} />
                        <span>{option.title}</span>
                        <code>{option.targetType}</code>
                      </button>
                    ))}
                  </div>
                ) : null}
              </section>

              <section className="task-detail-section">
                <div className="task-section-heading">
                  <h4>Task Links</h4>
                </div>
                <div className="task-link-list">
                  {selectedJob.taskLinks.length === 0 ? (
                    <div className="empty-state compact">No tasks linked</div>
                  ) : (
                    selectedJob.taskLinks.map((link) => (
                      <div className="qa-task-link-row" key={link.id}>
                        <div>
                          <strong>Task Id {link.taskId} {link.title}</strong>
                          <span>{link.state}</span>
                        </div>
                        <button onClick={() => removeTaskLink(selectedJob, link.taskId)} title="Remove task" type="button">
                          <X size={16} />
                        </button>
                      </div>
                    ))
                  )}
                </div>
                <label className="task-link-search">
                  <Search size={16} />
                  <input value={taskQuery} onChange={(event) => setTaskQuery(event.target.value)} placeholder="Search tasks" />
                </label>
                {taskQuery.trim() ? (
                  <div className="task-link-results">
                    {taskLinkOptions.map((task) => (
                      <button key={task.id} onClick={() => addTaskLink(selectedJob, task)} type="button">
                        <Link2 size={15} />
                        <span>Task Id {task.id} {task.title}</span>
                        <code>{task.state}</code>
                      </button>
                    ))}
                  </div>
                ) : null}
              </section>
            </section>
          </>
        ) : (
          <div className="empty-state">Create or reveal a QA job to start editing</div>
        )}

        <section className="qa-history-panel">
          <div className="task-section-heading">
            <h4>Job History</h4>
          </div>
          <div className="qa-history-layout">
            <div className="qa-run-list">
              {!selectedJob ? (
                <div className="empty-state compact">No QA job selected</div>
              ) : selectedJob.runHistory.length === 0 ? (
                <div className="empty-state compact">No results for this job</div>
              ) : (
                selectedJob.runHistory.map((jobRun) => (
                  <button
                    className={selectedJobRun?.id === jobRun.id ? "qa-run-item active" : "qa-run-item"}
                    key={jobRun.id}
                    onClick={() => setSelectedJobRunId(jobRun.id)}
                    type="button"
                  >
                    <span className={`pill qa-run-${jobRun.status}`}>{jobRun.status}</span>
                    <strong>Run #{jobRun.qaRunId}</strong>
                    <small>{jobRun.finishedAt ?? jobRun.startedAt}</small>
                  </button>
                ))
              )}
            </div>
            <div className="qa-console">
              {selectedJobRun && selectedJob ? (
                <article key={selectedJobRun.id}>
                  <div>
                    <span className={`pill qa-run-${selectedJobRun.status}`}>{selectedJobRun.status}</span>
                    <strong>{selectedJob.name}</strong>
                    <small>{selectedJobRun.durationMs ?? 0} ms</small>
                  </div>
                  <pre>{selectedJobRun.output || "(no output)"}</pre>
                </article>
              ) : (
                <div className="empty-state compact">No console output</div>
              )}
            </div>
          </div>
        </section>
      </section>
    </section>
  );
}

function buildQaTaskLinkOptions(tasks: Task[], query: string): Task[] {
  const terms = query
    .trim()
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean);

  if (terms.length === 0) {
    return [];
  }

  return tasks
    .filter((task) => {
      const haystack = `task id ${task.id} ${task.title} ${task.description} ${task.state}`.toLowerCase();
      return terms.every((term) => haystack.includes(term));
    })
    .slice(0, 12);
}

function qaDesignLinkToInput(link: QaJobDesignLink): Pick<QaJobDesignLink, "targetType" | "designExternalId"> {
  return {
    targetType: link.targetType,
    designExternalId: link.designExternalId,
  };
}

function splitCommaList(value: string): string[] {
  return value
    .split(",")
    .map((item) => item.trim())
    .filter(Boolean);
}

function RulesView({
  projectId,
  rules,
  ruleTemplates,
  onChange,
  onError,
  onSettingsChange,
}: {
  projectId: string;
  rules: Rule[];
  ruleTemplates: RuleTemplate[];
  onChange: (payload: DashboardPayload) => void;
  onError: (message: string) => void;
  onSettingsChange: (settings: AppSettings) => void;
}) {
  const [selectedRuleId, setSelectedRuleId] = React.useState<number | null>(rules[0]?.id ?? null);
  const [selectedTemplateId, setSelectedTemplateId] = React.useState<string>(ruleTemplates[0]?.id ?? "");

  React.useEffect(() => {
    if (rules.length === 0) {
      setSelectedRuleId(null);
      return;
    }

    if (!selectedRuleId || !rules.some((rule) => rule.id === selectedRuleId)) {
      setSelectedRuleId(rules[0].id);
    }
  }, [rules, selectedRuleId]);

  React.useEffect(() => {
    if (ruleTemplates.length === 0) {
      setSelectedTemplateId("");
      return;
    }

    if (!selectedTemplateId || !ruleTemplates.some((template) => template.id === selectedTemplateId)) {
      setSelectedTemplateId(ruleTemplates[0].id);
    }
  }, [ruleTemplates, selectedTemplateId]);

  const selectedRule = rules.find((rule) => rule.id === selectedRuleId) ?? null;
  const selectedTemplate = ruleTemplates.find((template) => template.id === selectedTemplateId) ?? null;

  function addRule() {
    invoke<DashboardPayload>("create_rule", {
      input: {
        projectId,
        name: "New Rule",
        enabled: true,
        intend: "implementation",
        hook: "task.start",
        prompt: "",
      },
    })
      .then((updatedPayload) => {
        const newestRule = updatedPayload.rules.reduce<Rule | null>(
          (newest, rule) => (!newest || rule.id > newest.id ? rule : newest),
          null,
        );
        setSelectedRuleId(newestRule?.id ?? null);
        onChange(updatedPayload);
      })
      .catch((reason) => onError(String(reason)));
  }

  function updateRule(rule: Rule, changes: Partial<Rule>) {
    const updatedRule = {
      ...rule,
      ...changes,
    };

    invoke<DashboardPayload>("update_rule", {
      input: {
        projectId,
        id: updatedRule.id,
        name: updatedRule.name.trim() || "New Rule",
        enabled: updatedRule.enabled,
        intend: updatedRule.intend,
        hook: updatedRule.hook,
        prompt: updatedRule.prompt,
      },
    })
      .then((updatedPayload) => {
        setSelectedRuleId(updatedRule.id);
        onChange(updatedPayload);
      })
      .catch((reason) => onError(String(reason)));
  }

  function remove(ruleId: number) {
    invoke<DashboardPayload>("delete_rule", { projectId, ruleId })
      .then((updatedPayload) => {
        setSelectedRuleId(updatedPayload.rules[0]?.id ?? null);
        onChange(updatedPayload);
      })
      .catch((reason) => onError(String(reason)));
  }

  function saveSelectedRuleAsTemplate() {
    if (!selectedRule) {
      return;
    }

    invoke<AppSettings>("save_rule_template", {
      input: {
        projectId,
        ruleId: selectedRule.id,
      },
    })
      .then((updatedSettings) => {
        const newestTemplate = updatedSettings.ruleTemplates[updatedSettings.ruleTemplates.length - 1] ?? null;
        setSelectedTemplateId(newestTemplate?.id ?? "");
        onSettingsChange(updatedSettings);
      })
      .catch((reason) => onError(String(reason)));
  }

  function createRuleFromSelectedTemplate() {
    if (!selectedTemplate) {
      return;
    }

    invoke<DashboardPayload>("create_rule_from_template", {
      input: {
        projectId,
        templateId: selectedTemplate.id,
      },
    })
      .then((updatedPayload) => {
        const newestRule = updatedPayload.rules.reduce<Rule | null>(
          (newest, rule) => (!newest || rule.id > newest.id ? rule : newest),
          null,
        );
        setSelectedRuleId(newestRule?.id ?? null);
        onChange(updatedPayload);
      })
      .catch((reason) => onError(String(reason)));
  }

  function removeSelectedTemplate() {
    if (!selectedTemplate) {
      return;
    }

    invoke<AppSettings>("delete_rule_template", { templateId: selectedTemplate.id })
      .then((updatedSettings) => {
        setSelectedTemplateId(updatedSettings.ruleTemplates[0]?.id ?? "");
        onSettingsChange(updatedSettings);
      })
      .catch((reason) => onError(String(reason)));
  }

  return (
    <section className="rules-grid">
      <section className="rules-list-panel">
        <div className="rules-panel-heading">
          <h3>Rules</h3>
          <button onClick={addRule} type="button">
            <Plus size={17} />
            Add
          </button>
        </div>
        <div className="rule-list">
          {rules.length === 0 ? (
            <div className="empty-state">No rules configured</div>
          ) : (
            rules.map((rule) => (
              <button
                className={rule.id === selectedRuleId ? "rule-list-item active" : "rule-list-item"}
                key={rule.id}
                onClick={() => setSelectedRuleId(rule.id)}
                type="button"
              >
                <strong>{rule.name || "Unnamed rule"}</strong>
                <div>
                  <span>{rule.intend}</span>
                  <code>{rule.hook}</code>
                </div>
              </button>
            ))
          )}
        </div>

        <section className="rule-template-panel">
          <div className="rule-template-heading">
            <h4>Templates</h4>
            <button disabled={!selectedRule} onClick={saveSelectedRuleAsTemplate} title="Save selected rule as template" type="button">
              <Wand2 size={16} />
              Save
            </button>
          </div>
          <select
            disabled={ruleTemplates.length === 0}
            value={selectedTemplateId}
            onChange={(event) => setSelectedTemplateId(event.target.value)}
          >
            {ruleTemplates.length === 0 ? (
              <option value="">No templates</option>
            ) : (
              ruleTemplates.map((template) => (
                <option key={template.id} value={template.id}>
                  {template.name}
                </option>
              ))
            )}
          </select>
          <div className="rule-template-actions">
            <button disabled={!selectedTemplate} onClick={createRuleFromSelectedTemplate} title="Create rule from selected template" type="button">
              <Plus size={16} />
              Create
            </button>
            <button
              className="danger-icon-button"
              disabled={!selectedTemplate}
              onClick={removeSelectedTemplate}
              title="Delete selected template"
              type="button"
            >
              <Trash2 size={16} />
            </button>
          </div>
          {selectedTemplate ? (
            <div className="rule-template-meta">
              <span>{selectedTemplate.intend}</span>
              <code>{selectedTemplate.hook}</code>
            </div>
          ) : null}
        </section>
      </section>

      <section className="rules-editor-panel">
        {selectedRule ? (
          <>
            <div className="rules-panel-heading">
              <h3>Edit Rule</h3>
              <button
                aria-label={`Delete ${selectedRule.name}`}
                className="danger-icon-button"
                onClick={() => remove(selectedRule.id)}
                title="Delete rule"
                type="button"
              >
                <Trash2 size={17} />
              </button>
            </div>

            <div className="rule-editor-form">
              <label>
                <span>Name</span>
                <input
                  defaultValue={selectedRule.name}
                  key={`name-${selectedRule.id}`}
                  onBlur={(event) => updateRule(selectedRule, { name: event.target.value })}
                  placeholder="Rule name"
                />
              </label>

              <label className="checkbox-row">
                <span>Enabled</span>
                <input
                  checked={selectedRule.enabled}
                  onChange={(event) => updateRule(selectedRule, { enabled: event.target.checked })}
                  type="checkbox"
                />
              </label>

              <label>
                <span>Intend</span>
                <select
                  value={selectedRule.intend}
                  onChange={(event) => updateRule(selectedRule, { intend: event.target.value as RuleIntend })}
                >
                  <option value="general">general</option>
                  <option value="design">design</option>
                  <option value="implementation">implementation</option>
                </select>
              </label>

              <label>
                <span>Hook</span>
                <select
                  value={selectedRule.hook}
                  onChange={(event) => updateRule(selectedRule, { hook: event.target.value as RuleHook })}
                >
                  <option value="run.start">run.start</option>
                  <option value="task.start">task.start</option>
                  <option value="task.end">task.end</option>
                  <option value="run.end">run.end</option>
                </select>
              </label>

              <label className="prompt-editor-label">
                <span>Prompt</span>
                <MarkdownEditor
                  key={selectedRule.id}
                  value={selectedRule.prompt}
                  onBlur={(value) => updateRule(selectedRule, { prompt: value })}
                />
              </label>
            </div>
          </>
        ) : (
          <div className="empty-state">Add a rule to start editing</div>
        )}
      </section>
    </section>
  );
}

function MemoryView({
  projectId,
  memory,
  onChange,
  onError,
}: {
  projectId: string;
  memory: ProjectMemory;
  onChange: (payload: DashboardPayload) => void;
  onError: (message: string) => void;
}) {
  function saveRule(rule: string) {
    invoke<DashboardPayload>("update_memory_rule", {
      input: {
        projectId,
        rule,
      },
    })
      .then(onChange)
      .catch((reason) => onError(String(reason)));
  }

  function saveMemory(nextMemory: string) {
    invoke<DashboardPayload>("update_memory", {
      input: {
        projectId,
        memory: nextMemory,
      },
    })
      .then(onChange)
      .catch((reason) => onError(String(reason)));
  }

  return (
    <section className="memory-grid">
      <section className="memory-panel">
        <div className="rules-panel-heading">
          <h3>Memory Rule</h3>
        </div>
        <MarkdownEditor
          key={`memory-rule-${memory.updatedAt}`}
          value={memory.rule}
          onBlur={saveRule}
          placeholder="Write the long-term memory protocol in Markdown..."
          minHeight="360px"
          maxHeight="460px"
          height="460px"
        />
      </section>

      <section className="memory-panel">
        <div className="rules-panel-heading">
          <h3>Current Memory</h3>
        </div>
        <MarkdownEditor
          key={`memory-body-${memory.updatedAt}`}
          value={memory.memory}
          onBlur={saveMemory}
          placeholder="Current project memory..."
          minHeight="360px"
          maxHeight="460px"
          height="460px"
        />
      </section>
    </section>
  );
}

function MarkdownEditor({
  value,
  onBlur,
  placeholder = "Write the injected agent instruction in Markdown...",
  minHeight = "440px",
  maxHeight = "620px",
  height = "560px",
}: {
  value: string;
  onBlur: (value: string) => void;
  placeholder?: string;
  minHeight?: string;
  maxHeight?: string;
  height?: string;
}) {
  const textareaRef = React.useRef<HTMLTextAreaElement>(null);
  const editorRef = React.useRef<EasyMDE | null>(null);
  const savedValueRef = React.useRef(value);
  const onBlurRef = React.useRef(onBlur);

  React.useEffect(() => {
    onBlurRef.current = onBlur;
  }, [onBlur]);

  React.useEffect(() => {
    if (!textareaRef.current) {
      return;
    }

    const editor = new EasyMDE({
      autoDownloadFontAwesome: false,
      autofocus: false,
      autoRefresh: { delay: 300 },
      autosave: {
        enabled: false,
        uniqueId: "adashi-rule-editor-disabled",
      },
      element: textareaRef.current,
      forceSync: true,
      initialValue: value,
      lineNumbers: false,
      lineWrapping: true,
      maxHeight,
      minHeight,
      nativeSpellcheck: true,
      placeholder,
      previewImagesInEditor: false,
      promptURLs: false,
      sideBySideFullscreen: false,
      spellChecker: false,
      status: false,
      styleSelectedText: false,
      toolbar: ([
        { name: "heading-1", action: EasyMDE.toggleHeading1, className: "adashi-mde-heading", title: "Heading", text: "H1" },
        { name: "bold", action: EasyMDE.toggleBold, className: "adashi-mde-bold", title: "Bold", text: "B" },
        { name: "italic", action: EasyMDE.toggleItalic, className: "adashi-mde-italic", title: "Italic", text: "I" },
        "|",
        { name: "quote", action: EasyMDE.toggleBlockquote, className: "adashi-mde-quote", title: "Quote", text: ">" },
        { name: "unordered-list", action: EasyMDE.toggleUnorderedList, className: "adashi-mde-list", title: "Bullet list", text: "- list" },
        { name: "ordered-list", action: EasyMDE.toggleOrderedList, className: "adashi-mde-ordered", title: "Numbered list", text: "1. list" },
        "|",
        { name: "code", action: EasyMDE.toggleCodeBlock, className: "adashi-mde-code", title: "Code block", text: "{ }" },
        { name: "link", action: EasyMDE.drawLink, className: "adashi-mde-link", title: "Link", text: "link" },
        "|",
        { name: "preview", action: EasyMDE.togglePreview, className: "adashi-mde-preview no-disable", title: "Preview", text: "Preview", noDisable: true },
      ] as EasyMDE.Options["toolbar"]),
      toolbarTips: true,
      uploadImage: false,
    });

    function commit() {
      const nextValue = editor.value();

      if (nextValue !== savedValueRef.current) {
        savedValueRef.current = nextValue;
        onBlurRef.current(nextValue);
      }
    }

    editor.codemirror.on("blur", commit);
    editorRef.current = editor;

    window.requestAnimationFrame(() => editor.codemirror.refresh());

    return () => {
      commit();
      editor.codemirror.off("blur", commit);
      editor.toTextArea();
      editorRef.current = null;
    };
  }, []);

  return (
    <div className="markdown-editor-host" style={{ height, minHeight }}>
      <textarea className="markdown-editor-source" ref={textareaRef} defaultValue={value} />
    </div>
  );
}

function Panel({ title, children }: React.PropsWithChildren<{ title: string }>) {
  return (
    <section className="data-panel">
      <h3>{title}</h3>
      {children}
    </section>
  );
}

function StructurizrFrame({
  workspace,
  viewKey,
  zoomPercent,
  onZoomChange,
  onOpen,
  onSelect,
}: {
  workspace: string;
  viewKey: string;
  zoomPercent: number;
  onZoomChange: (zoomPercent: number) => void;
  onOpen?: (entity: { type: DesignEntityType; externalId: string }) => void;
  onSelect?: (externalId: string) => void;
}) {
  const frameRef = React.useRef<HTMLIFrameElement>(null);

  const postWorkspace = React.useCallback(() => {
    frameRef.current?.contentWindow?.postMessage(
      {
        type: "adashi:structurizr-workspace",
        workspace,
        viewKey,
      },
      "*",
    );
  }, [viewKey, workspace]);

  const postZoom = React.useCallback(() => {
    frameRef.current?.contentWindow?.postMessage(
      zoomPercent === 0
        ? {
            type: "adashi:structurizr-fit",
          }
        : {
            type: "adashi:structurizr-zoom",
            zoomPercent,
          },
      "*",
    );
  }, [zoomPercent]);

  React.useEffect(() => {
    postWorkspace();
  }, [postWorkspace]);

  React.useEffect(() => {
    postZoom();
  }, [postZoom]);

  React.useEffect(() => {
    function receiveMessage(event: MessageEvent) {
      if (!event.data || typeof event.data !== "object") {
        return;
      }

      if (event.data.type === "adashi:structurizr-selection") {
        const ids: unknown[] = Array.isArray(event.data.ids) ? event.data.ids : [];
        const firstId = ids.find((id): id is string => typeof id === "string");

        if (firstId) {
          onSelect?.(firstId);
        }
      }

      if (event.data.type === "adashi:structurizr-open" && typeof event.data.id === "string") {
        const entityType = event.data.entityType === "relationship" ? "relationship" : "element";
        onOpen?.({ type: entityType, externalId: event.data.id });
      }

      if (event.data.type === "adashi:structurizr-zoom-changed" && typeof event.data.zoomPercent === "number") {
        onZoomChange(Math.round(event.data.zoomPercent));
      }
    }

    window.addEventListener("message", receiveMessage);
    return () => window.removeEventListener("message", receiveMessage);
  }, [onOpen, onSelect, onZoomChange]);

  return (
    <iframe
      ref={frameRef}
      className="diagram-frame"
      src="/structurizr/viewer.html"
      title="Structurizr C4 diagram"
      onLoad={postWorkspace}
    />
  );
}

function MermaidPanel({ diagram }: { diagram: string }) {
  const ref = React.useRef<HTMLDivElement>(null);

  React.useEffect(() => {
    let cancelled = false;

    async function render() {
      if (!ref.current) {
        return;
      }

      if (!diagram) {
        ref.current.textContent = "No UML artifact is attached to this selection.";
        return;
      }

      const id = `mermaid-${Date.now()}`;
      const result = await mermaid.render(id, diagram);

      if (!cancelled && ref.current) {
        ref.current.innerHTML = result.svg;
      }
    }

    render().catch((reason) => {
      if (ref.current) {
        ref.current.textContent = String(reason);
      }
    });

    return () => {
      cancelled = true;
    };
  }, [diagram]);

  return <div className="mermaid-host" ref={ref} />;
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
