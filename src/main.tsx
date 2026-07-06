import React from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import EasyMDE from "easymde";
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
  ScrollText,
  Settings,
  ServerCog,
  Trash2,
} from "lucide-react";
import mermaid from "mermaid";
import "easymde/dist/easymde.min.css";
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
  body: string;
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
};

type DashboardPayload = {
  projectId: string;
  projectName: string;
  projectFolder: string;
  revision: number;
  workspaceName: string;
  workspaceDescription: string;
  structurizrWorkspace: string;
  structurizrViewKey: string;
  diagrams: DesignDiagram[];
  tasks: Task[];
  guidelines: Guideline[];
  postTaskCommands: Command[];
  qaChecks: QaCheck[];
  rules: Rule[];
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
  const [activeDiagram, setActiveDiagram] = React.useState<DiagramKind>("structurizr");
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
        ) : activeView === "rules" ? (
          <RulesView
            projectId={payload.projectId}
            rules={payload.rules}
            onChange={(updatedPayload) => {
              setPayload((currentPayload) =>
                currentPayload ? mergeDashboardPayload(currentPayload, updatedPayload) : updatedPayload,
              );
            }}
            onError={setError}
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
          <TasksView tasks={payload.tasks} postTaskCommands={payload.postTaskCommands} />
        ) : activeView === "qa" ? (
          <QaView qaChecks={payload.qaChecks} />
        ) : (
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
        )}
      </section>
    </main>
  );
}

function mergeDashboardPayload(current: DashboardPayload, next: DashboardPayload): DashboardPayload {
  return {
    ...current,
    ...next,
    diagrams: reconcileById(current.diagrams, next.diagrams),
    tasks: reconcileById(current.tasks, next.tasks),
    guidelines: reconcileById(current.guidelines, next.guidelines),
    postTaskCommands: reconcileById(current.postTaskCommands, next.postTaskCommands),
    qaChecks: reconcileById(current.qaChecks, next.qaChecks),
    rules: reconcileById(current.rules, next.rules),
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

function TasksView({ tasks, postTaskCommands }: { tasks: Task[]; postTaskCommands: Command[] }) {
  return (
    <section className="tasks-grid">
      <Panel title="Task Queue">
        {tasks.map((task) => (
          <div className="row-item" key={task.id}>
            <span className={`pill ${task.status}`}>{task.status}</span>
            <span>{task.title}</span>
            <strong>P{task.priority}</strong>
          </div>
        ))}
      </Panel>

      <Panel title="Post-Task Commands">
        {postTaskCommands.map((command) => (
          <div className="command-row" key={command.id}>
            <PlayCircle size={17} />
            <div>
              <span>{command.label}</span>
              <code>{command.command}</code>
            </div>
          </div>
        ))}
      </Panel>
    </section>
  );
}

function QaView({ qaChecks }: { qaChecks: QaCheck[] }) {
  return (
    <section className="qa-grid">
      <Panel title="QA Gates">
        {qaChecks.map((check) => (
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
  );
}

function RulesView({
  projectId,
  rules,
  onChange,
  onError,
}: {
  projectId: string;
  rules: Rule[];
  onChange: (payload: DashboardPayload) => void;
  onError: (message: string) => void;
}) {
  const [selectedRuleId, setSelectedRuleId] = React.useState<number | null>(rules[0]?.id ?? null);

  React.useEffect(() => {
    if (rules.length === 0) {
      setSelectedRuleId(null);
      return;
    }

    if (!selectedRuleId || !rules.some((rule) => rule.id === selectedRuleId)) {
      setSelectedRuleId(rules[0].id);
    }
  }, [rules, selectedRuleId]);

  const selectedRule = rules.find((rule) => rule.id === selectedRuleId) ?? null;

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
