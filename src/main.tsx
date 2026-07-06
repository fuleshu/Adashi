import React from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import {
  Activity,
  Bot,
  Braces,
  CheckCircle2,
  Database,
  Folder,
  FileCode2,
  GitBranch,
  ListChecks,
  Network,
  PlayCircle,
  Plus,
  Settings,
  ServerCog,
  Trash2,
} from "lucide-react";
import mermaid from "mermaid";
import "./styles.css";

type DiagramKind = "mermaid" | "structurizr";

type DesignDiagram = {
  id: number;
  kind: DiagramKind;
  key: string;
  title: string;
  source: string;
};

type Task = {
  id: number;
  title: string;
  status: string;
  priority: number;
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
};

type DashboardPayload = {
  projectId: string;
  projectName: string;
  projectFolder: string;
  workspaceName: string;
  workspaceDescription: string;
  structurizrWorkspace: string;
  structurizrViewKey: string;
  diagrams: DesignDiagram[];
  tasks: Task[];
  guidelines: Guideline[];
  postTaskCommands: Command[];
  qaChecks: QaCheck[];
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
  const [activeDiagram, setActiveDiagram] = React.useState<DiagramKind>("structurizr");
  const [activeView, setActiveView] = React.useState<"design" | "settings">("design");
  const [error, setError] = React.useState<string | null>(null);

  const loadDashboard = React.useCallback((projectId?: string | null) => {
    setPayload(null);
    invoke<DashboardPayload>("get_dashboard", { projectId })
      .then(setPayload)
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

  const mermaidDiagram = payload.diagrams.find((diagram) => diagram.kind === "mermaid");

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
          <button className={activeView === "design" ? "active" : ""} onClick={() => setActiveView("design")} type="button">
            <ListChecks size={18} />
            Tasks
          </button>
          <button className={activeView === "settings" ? "active" : ""} onClick={() => setActiveView("settings")} type="button">
            <Settings size={18} />
            Settings
          </button>
          <button className={activeView === "design" ? "active" : ""} onClick={() => setActiveView("design")} type="button">
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
            <p className="path-line">{payload.projectFolder}</p>
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
            onError={setError}
          />
        ) : (
          <>
        <section id="design" className="workspace-grid">
          <div className="viewer-panel">
            <div className="panel-heading">
              <div>
                <p className="eyebrow">Design View</p>
                <h3>{activeDiagram === "structurizr" ? "Structurizr C4" : mermaidDiagram?.title}</h3>
              </div>
              <div className="segmented" role="tablist" aria-label="Diagram viewer">
                <button
                  className={activeDiagram === "structurizr" ? "active" : ""}
                  onClick={() => setActiveDiagram("structurizr")}
                  type="button"
                >
                  <Braces size={16} />
                  C4
                </button>
                <button
                  className={activeDiagram === "mermaid" ? "active" : ""}
                  onClick={() => setActiveDiagram("mermaid")}
                  type="button"
                >
                  <GitBranch size={16} />
                  UML
                </button>
              </div>
            </div>
            {activeDiagram === "structurizr" ? (
              <StructurizrFrame workspace={payload.structurizrWorkspace} viewKey={payload.structurizrViewKey} />
            ) : (
              <MermaidPanel diagram={mermaidDiagram?.source ?? ""} />
            )}
          </div>

          <div className="brief-panel">
            <h3>Agent Workbench</h3>
            <p>
              This seed schema separates design artifacts, task injection, coding guidelines, post-task commands,
              and QA checks so the MCP layer can expose each area cleanly.
            </p>
            <div className="brief-list">
              {payload.guidelines.map((item) => (
                <article key={item.id}>
                  <FileCode2 size={18} />
                  <div>
                    <h4>{item.title}</h4>
                    <p>{item.body}</p>
                  </div>
                </article>
              ))}
            </div>
          </div>
        </section>

        <section id="tasks" className="lower-grid">
          <Panel title="Task Queue">
            {payload.tasks.map((task) => (
              <div className="row-item" key={task.id}>
                <span className={`pill ${task.status}`}>{task.status}</span>
                <span>{task.title}</span>
                <strong>P{task.priority}</strong>
              </div>
            ))}
          </Panel>

          <Panel title="Post-Task Commands">
            {payload.postTaskCommands.map((command) => (
              <div className="command-row" key={command.id}>
                <PlayCircle size={17} />
                <div>
                  <span>{command.label}</span>
                  <code>{command.command}</code>
                </div>
              </div>
            ))}
          </Panel>

          <Panel title="QA Gates">
            {payload.qaChecks.map((check) => (
              <div className="command-row" key={check.id}>
                <CheckCircle2 size={17} />
                <div>
                  <span>{check.label}</span>
                  <code>{check.command}</code>
                </div>
              </div>
            ))}
          </Panel>
        </section>
          </>
        )}
      </section>
    </main>
  );
}

function SettingsView({
  activeProjectId,
  settings,
  onAdd,
  onDelete,
  onError,
}: {
  activeProjectId: string;
  settings: AppSettings;
  onAdd: (settings: AppSettings, projectId: string) => void;
  onDelete: (settings: AppSettings) => void;
  onError: (message: string) => void;
}) {
  const [name, setName] = React.useState("");
  const [folder, setFolder] = React.useState("");

  function submit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    invoke<AppSettings>("add_project", { name, folder })
      .then((updatedSettings) => {
        const projectId = updatedSettings.lastActiveProjectId ?? updatedSettings.projects[0]?.id;
        setName("");
        setFolder("");
        onAdd(updatedSettings, projectId);
      })
      .catch((reason) => onError(String(reason)));
  }

  function remove(projectId: string) {
    invoke<AppSettings>("delete_project", { projectId })
      .then(onDelete)
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
            <input value={folder} onChange={(event) => setFolder(event.target.value)} placeholder="C:\\src\\MyProject" />
          </label>
          <button type="submit">
            <Plus size={17} />
            Add
          </button>
        </form>
      </section>
    </section>
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

function StructurizrFrame({ workspace, viewKey }: { workspace: string; viewKey: string }) {
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

  React.useEffect(() => {
    postWorkspace();
  }, [postWorkspace]);

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
      if (!ref.current || !diagram) {
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
