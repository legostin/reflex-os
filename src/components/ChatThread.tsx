import { Fragment, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { DiffPanel } from "./DiffPanel";
import MemoryPanel from "./memory/MemoryPanel";
import { AutomationsScreen } from "./automations/AutomationsScreen";
import RecallView from "./memory/RecallView";
import {
  FileActionsDrawer,
  type DrawerTarget,
  type PathStatus,
} from "./files/FileActionsDrawer";
import { WidgetGrid, type WidgetSource } from "./widgets/WidgetGrid";
import { SuggesterModal } from "./projects/SuggesterModal";
import { BrowserScreen } from "./browser/BrowserScreen";
import { SettingsScreen } from "./settings/SettingsScreen";
import "./ChatThread.css";

type QuickContext = {
  frontmost_app: string | null;
  finder_target: string | null;
};

type Project = {
  id: string;
  name: string;
  root: string;
  created_at_ms: number;
  sandbox?: string;
  mcp_servers?: Record<string, any> | null;
  description?: string | null;
  agent_instructions?: string | null;
  skills?: string[];
  apps?: string[];
};

export type BrowserTabSnapshot = { url: string; title: string };

type ThreadCreated = {
  id: string;
  project_id: string;
  project_name: string;
  prompt: string;
  cwd: string;
  ctx: QuickContext;
  created_at_ms: number;
  plan_mode?: boolean;
  source?: string;
  browser_tabs?: BrowserTabSnapshot[];
};

type CodexEventPayload = {
  thread_id: string;
  seq: number;
  raw: string;
  stream: "stdout" | "stderr" | "error" | "user";
};

type CodexEndPayload = {
  thread_id: string;
  exit_code: number | null;
};

type ThreadRunningPayload = {
  thread_id: string;
};

type ThreadEvent = {
  seq: number;
  stream: CodexEventPayload["stream"];
  raw: string;
  parsed: any | null;
};

type Thread = {
  id: string;
  project_id: string;
  project_name: string;
  prompt: string;
  cwd: string;
  ctx: QuickContext;
  created_at_ms: number;
  events: ThreadEvent[];
  exit_code: number | null | undefined;
  done: boolean;
  session_id: string | null;
  title: string | null;
  goal: string | null;
  pending_questions: ThreadQuestion[];
  plan_mode: boolean;
  plan_confirmed: boolean;
  source: string;
  browser_tabs: BrowserTabSnapshot[];
};

type ThreadMetaUpdated = {
  thread_id: string;
  title?: string | null;
  goal?: string | null;
  plan_confirmed?: boolean;
};

type ThreadQuestion = {
  question_id: string;
  method: string;
  params: any;
  thread_id: string | null;
};

type StoredEvent = { seq: number; stream: string; ts_ms: number; raw: string };

type StoredThreadMeta = {
  id: string;
  project_id: string | null;
  prompt: string;
  cwd: string;
  frontmost_app: string | null;
  finder_target: string | null;
  created_at_ms: number;
  exit_code: number | null;
  done: boolean;
  session_id: string | null;
  title: string | null;
  goal: string | null;
  plan_mode?: boolean;
  plan_confirmed?: boolean;
  source?: string;
  browser_tabs?: BrowserTabSnapshot[];
};

type ProjectThread = {
  project: Project;
  thread: { meta: StoredThreadMeta; events: StoredEvent[] };
};

type Route =
  | { kind: "home" }
  | { kind: "project"; project_id: string }
  | { kind: "topic"; thread_id: string }
  | {
      kind: "apps";
      initialTemplate?: string;
      openCreate?: boolean;
      createRequestId?: number;
    }
  | { kind: "app"; app_id: string }
  | { kind: "memory"; project_id?: string; thread_id?: string }
  | { kind: "automations" }
  | { kind: "browser" }
  | { kind: "settings" };

function routeKey(r: Route): string {
  switch (r.kind) {
    case "home":
      return "home";
    case "apps":
      return "apps";
    case "project":
      return `project:${r.project_id}`;
    case "topic":
      return `topic:${r.thread_id}`;
    case "app":
      return `app:${r.app_id}`;
    case "memory":
      return r.thread_id
        ? `memory:${r.project_id ?? "global"}:${r.thread_id}`
        : r.project_id
          ? `memory:${r.project_id}`
          : "memory";
    case "automations":
      return "automations";
    case "browser":
      return "browser";
    case "settings":
      return "settings";
  }
}

function tabIcon(r: Route): string {
  switch (r.kind) {
    case "home":
      return "🏠";
    case "apps":
      return "🧩";
    case "project":
      return "📁";
    case "topic":
      return "💬";
    case "app":
      return "🪟";
    case "memory":
      return "M";
    case "automations":
      return "⏱";
    case "browser":
      return "🌐";
    case "settings":
      return "⚙";
  }
}

function tabLabel(
  r: Route,
  projects: Project[],
  threads: Thread[],
): string {
  switch (r.kind) {
    case "home":
      return "Home";
    case "apps":
      return "Apps";
    case "project": {
      const p = projects.find((x) => x.id === r.project_id);
      return p?.name ?? r.project_id;
    }
    case "topic": {
      const t = threads.find((x) => x.id === r.thread_id);
      return t?.title ?? t?.prompt?.slice(0, 40) ?? r.thread_id;
    }
    case "app":
      return r.app_id;
    case "memory": {
      if (r.thread_id) {
        const t = threads.find((x) => x.id === r.thread_id);
        return `Memory · ${t?.title ?? t?.prompt?.slice(0, 32) ?? r.thread_id}`;
      }
      if (!r.project_id) return "Memory";
      const p = projects.find((x) => x.id === r.project_id);
      return `Memory · ${p?.name ?? r.project_id}`;
    }
    case "automations":
      return "Automations";
    case "browser":
      return "Browser";
    case "settings":
      return "Settings";
  }
}

type PaneId = string;
type Pane = { id: PaneId; tabs: Route[]; activeKey: string };
type Layout = {
  panes: Pane[];
  paneSizes: Record<PaneId, number>;
  focusedPaneId: PaneId;
};

let paneSeq = 0;
const nextPaneId = (): PaneId => `p${++paneSeq}`;

const TAB_DRAG_TYPE = "application/reflex-tab";

const initialLayout = (): Layout => {
  const id = nextPaneId();
  return {
    panes: [{ id, tabs: [{ kind: "home" }], activeKey: "home" }],
    paneSizes: { [id]: 1 },
    focusedPaneId: id,
  };
};

function removeTabFromPane(p: Pane, key: string): Pane {
  const idx = p.tabs.findIndex((t) => routeKey(t) === key);
  if (idx === -1) return p;
  const newTabs = p.tabs.filter((_, i) => i !== idx);
  let newActive = p.activeKey;
  if (newActive === key) {
    const fb = newTabs[idx] ?? newTabs[idx - 1] ?? newTabs[0];
    newActive = fb ? routeKey(fb) : "";
  }
  return { ...p, tabs: newTabs, activeKey: newActive };
}

function compactLayout(prev: Layout, panes: Pane[]): Layout {
  const nonEmpty = panes.filter((p) => p.tabs.length > 0);
  if (nonEmpty.length === 0) {
    const id = nextPaneId();
    return {
      panes: [{ id, tabs: [{ kind: "home" }], activeKey: "home" }],
      paneSizes: { [id]: 1 },
      focusedPaneId: id,
    };
  }
  const sizes: Record<PaneId, number> = {};
  for (const p of nonEmpty) sizes[p.id] = prev.paneSizes[p.id] ?? 1;
  const focus = nonEmpty.some((p) => p.id === prev.focusedPaneId)
    ? prev.focusedPaneId
    : nonEmpty[0].id;
  return { panes: nonEmpty, paneSizes: sizes, focusedPaneId: focus };
}

type ServerLogLine = {
  seq: number;
  stream: "stdout" | "stderr" | "system";
  line: string;
  ts_ms: number;
};

type AppWidget = {
  id: string;
  name: string;
  entry: string;
  size?: string;
  description?: string | null;
};

type AppNetworkPolicy = {
  allowed_hosts?: string[];
};

type AppStep = {
  method: string;
  params?: any;
  save_as?: string | null;
};

type AppSchedule = {
  id: string;
  name: string;
  cron: string;
  enabled?: boolean;
  catch_up?: string;
  steps?: AppStep[];
};

type AppAction = {
  id: string;
  name: string;
  description?: string | null;
  params_schema?: any;
  paramsSchema?: any;
  public?: boolean;
  steps?: AppStep[];
};

type AppManifest = {
  id: string;
  name: string;
  icon?: string | null;
  description?: string | null;
  entry: string;
  permissions: string[];
  kind: string;
  created_at_ms: number;
  ready?: boolean;
  runtime?: string | null;
  server?: { command: string[]; ready_timeout_ms?: number | null } | null;
  network?: AppNetworkPolicy | null;
  schedules?: AppSchedule[];
  actions?: AppAction[];
  widgets?: AppWidget[];
};

type AppCapabilityFact = {
  key: string;
  label: string;
  value: string;
  title: string;
};

const APP_BRIDGE_HELPER_GROUPS = [
  {
    title: "Core",
    helpers: [
      "reflexInvoke",
      "reflexSystemContext",
      "reflexManifestGet",
      "reflexManifestUpdate",
      "reflexCapabilities",
    ],
  },
  {
    title: "Agent",
    helpers: [
      "reflexAgentAsk",
      "reflexAgentStartTopic",
      "reflexAgentTask",
      "reflexAgentStream",
      "reflexAgentStreamAbort",
    ],
  },
  {
    title: "Data / IO",
    helpers: [
      "reflexStorageGet",
      "reflexStorageSet",
      "reflexFsRead",
      "reflexFsWrite",
      "reflexNetFetch",
      "reflexDialogOpenFile",
      "reflexDialogSaveFile",
      "reflexNotifyShow",
    ],
  },
  {
    title: "Projects",
    helpers: [
      "reflexProjectsList",
      "reflexTopicsList",
      "reflexSkillsList",
      "reflexMcpServers",
      "reflexBrowserReadOutline",
      "reflexBrowserScreenshot",
    ],
  },
  {
    title: "Automation",
    helpers: [
      "reflexSchedulerList",
      "reflexSchedulerRunNow",
      "reflexSchedulerRuns",
      "reflexMemoryRecall",
      "reflexAppsInvoke",
      "reflexAppsListActions",
      "reflexEventOn",
      "reflexEventEmit",
    ],
  },
] as const;

function summarizeManifestValues(
  values: string[],
  empty: string,
  overflowLabel: string,
): string {
  if (values.length === 0) return empty;
  if (values.length <= 2) return values.join(", ");
  return `${values.length} ${overflowLabel}`;
}

function previewJsonValue(value: unknown): string {
  if (value == null) return "null";
  if (typeof value === "string") return value.slice(0, 240);
  try {
    return JSON.stringify(value, null, 2).slice(0, 240);
  } catch {
    return String(value).slice(0, 240);
  }
}

function actionParamsSchema(action: AppAction): unknown {
  return action.params_schema ?? action.paramsSchema ?? null;
}

function isJsonObject(value: unknown): value is Record<string, any> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}

function sampleValueFromJsonSchema(schema: unknown, depth = 0): unknown {
  if (!isJsonObject(schema) || depth > 5) return {};
  if ("default" in schema) return schema.default;
  if ("const" in schema) return schema.const;
  if (Array.isArray(schema.enum) && schema.enum.length > 0) return schema.enum[0];

  const typeValue = Array.isArray(schema.type) ? schema.type[0] : schema.type;
  if (typeValue === "object" || isJsonObject(schema.properties)) {
    const out: Record<string, unknown> = {};
    const properties = isJsonObject(schema.properties) ? schema.properties : {};
    for (const [key, childSchema] of Object.entries(properties)) {
      out[key] = sampleValueFromJsonSchema(childSchema, depth + 1);
    }
    return out;
  }
  if (typeValue === "array") {
    const minItems =
      typeof schema.minItems === "number" ? Math.max(0, schema.minItems) : 0;
    if (minItems <= 0) return [];
    return Array.from({ length: Math.min(minItems, 3) }, () =>
      sampleValueFromJsonSchema(schema.items, depth + 1),
    );
  }
  if (typeValue === "boolean") return false;
  if (typeValue === "integer" || typeValue === "number") return 0;
  if (typeValue === "null") return null;
  return "";
}

function defaultActionParamsJson(action: AppAction): string {
  const schema = actionParamsSchema(action);
  const sample = schema ? sampleValueFromJsonSchema(schema) : {};
  return JSON.stringify(sample, null, 2);
}

function buildAppCapabilityFacts(
  manifest: AppManifest | null,
  serverPort: number | null,
): AppCapabilityFact[] {
  if (!manifest) return [];

  const runtime = manifest.runtime === "server" ? "server" : "static";
  const permissions = manifest.permissions ?? [];
  const allowedHosts = manifest.network?.allowed_hosts ?? [];
  const actions = manifest.actions ?? [];
  const schedules = manifest.schedules ?? [];
  const widgets = manifest.widgets ?? [];
  const enabledSchedules = schedules.filter((s) => s.enabled !== false).length;
  const serverCommand = manifest.server?.command?.join(" ");

  return [
    {
      key: "runtime",
      label: "runtime",
      value: runtime === "server" && serverPort ? `server :${serverPort}` : runtime,
      title:
        runtime === "server"
          ? serverCommand
            ? `server command: ${serverCommand}`
            : "server runtime"
          : `entry: ${manifest.entry}`,
    },
    {
      key: "permissions",
      label: "permissions",
      value: summarizeManifestValues(permissions, "none", "permissions"),
      title: permissions.length ? permissions.join(", ") : "no bridge permissions",
    },
    {
      key: "network",
      label: "network",
      value: summarizeManifestValues(allowedHosts, "none", "hosts"),
      title: allowedHosts.length
        ? `allowed hosts: ${allowedHosts.join(", ")}`
        : "no allowed network hosts",
    },
    {
      key: "actions",
      label: "actions",
      value: summarizeManifestValues(
        actions.map((a) => a.name || a.id),
        "none",
        "actions",
      ),
      title: actions.length
        ? actions.map((a) => `${a.name || a.id}${a.public ? " (public)" : ""}`).join(", ")
        : "no manifest actions",
    },
    {
      key: "schedules",
      label: "schedules",
      value:
        schedules.length === 0
          ? "none"
          : `${enabledSchedules}/${schedules.length} active`,
      title: schedules.length
        ? schedules
            .map((s) => `${s.name || s.id}: ${s.cron}${s.enabled === false ? " (paused)" : ""}`)
            .join(", ")
        : "no manifest schedules",
    },
    {
      key: "widgets",
      label: "widgets",
      value: summarizeManifestValues(
        widgets.map((w) => w.name || w.id),
        "none",
        "widgets",
      ),
      title: widgets.length
        ? widgets.map((w) => `${w.name || w.id}: ${w.size ?? "small"}`).join(", ")
        : "no dashboard widgets",
    },
  ];
}

function buildAppCatalogCapabilityFacts(
  manifest: AppManifest,
): AppCapabilityFact[] {
  return buildAppCapabilityFacts(manifest, null).filter(
    (fact) => fact.key === "runtime" || fact.value !== "none",
  );
}

type AppServerStatus = {
  running: boolean;
  port: number | null;
  exit_code: number | null;
};

function fromProjectThread(pt: ProjectThread): Thread {
  return {
    id: pt.thread.meta.id,
    project_id: pt.project.id,
    project_name: pt.project.name,
    prompt: pt.thread.meta.prompt,
    cwd: pt.thread.meta.cwd,
    ctx: {
      frontmost_app: pt.thread.meta.frontmost_app,
      finder_target: pt.thread.meta.finder_target,
    },
    created_at_ms: pt.thread.meta.created_at_ms,
    events: pt.thread.events.map((e) => ({
      seq: e.seq,
      stream: (e.stream as ThreadEvent["stream"]) ?? "stdout",
      raw: e.raw,
      parsed: tryParse(e.raw),
    })),
    exit_code: pt.thread.meta.exit_code,
    done: pt.thread.meta.done,
    session_id: pt.thread.meta.session_id,
    title: pt.thread.meta.title,
    goal: pt.thread.meta.goal,
    pending_questions: [],
    plan_mode: !!pt.thread.meta.plan_mode,
    plan_confirmed: !!pt.thread.meta.plan_confirmed,
    source: pt.thread.meta.source ?? "quick",
    browser_tabs: pt.thread.meta.browser_tabs ?? [],
  };
}

function tryParse(s: string): any | null {
  try {
    return JSON.parse(s);
  } catch {
    return null;
  }
}

function upsertThread(prev: Thread[], next: Thread): Thread[] {
  const idx = prev.findIndex((t) => t.id === next.id);
  if (idx === -1) return [...prev, next];
  const merged = { ...prev[idx], ...next, events: prev[idx].events };
  const copy = [...prev];
  copy[idx] = merged;
  return copy;
}

function appendEvent(
  prev: Thread[],
  thread_id: string,
  ev: ThreadEvent,
): Thread[] {
  return prev.map((t) => {
    if (t.id !== thread_id) return t;
    if (t.events.some((e) => e.seq === ev.seq)) return t;
    const events = [...t.events, ev].sort((a, b) => a.seq - b.seq);
    return { ...t, events };
  });
}

export default function ChatThread() {
  const [threads, setThreads] = useState<Thread[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [layout, setLayout] = useState<Layout>(initialLayout);
  const [draggingTab, setDraggingTab] = useState(false);
  const [newProjectPath, setNewProjectPath] = useState<string | null>(null);
  const [newProjectDescription, setNewProjectDescription] = useState("");
  const [creatingProject, setCreatingProject] = useState(false);
  const [suggesterProjectId, setSuggesterProjectId] = useState<string | null>(
    null,
  );
  const [installedAppsLite, setInstalledAppsLite] = useState<
    { id: string; name: string; icon?: string | null }[]
  >([]);
  const [activeProjectId, setActiveProjectIdState] = useState<string | null>(
    null,
  );
  const containerRef = useRef<HTMLDivElement>(null);

  const setActiveProject = async (id: string | null) => {
    setActiveProjectIdState(id);
    try {
      await invoke("set_active_project", { projectId: id });
    } catch (e) {
      console.error("[reflex] set_active_project failed", e);
    }
    // Browser session is re-bound when BrowserScreen remounts on key change —
    // no need to eagerly start the sidecar here.
  };

  const focusPane = (paneId: PaneId) =>
    setLayout((prev) =>
      prev.focusedPaneId === paneId ? prev : { ...prev, focusedPaneId: paneId },
    );

  const activateTab = (paneId: PaneId, key: string) =>
    setLayout((prev) => ({
      ...prev,
      panes: prev.panes.map((p) =>
        p.id === paneId ? { ...p, activeKey: key } : p,
      ),
      focusedPaneId: paneId,
    }));

  // Navigate within focused pane. If route already lives in another pane, just focus it.
  const navigate = (r: Route) => {
    const k = routeKey(r);
    setLayout((prev) => {
      const focused = prev.panes.find((p) => p.id === prev.focusedPaneId);
      if (focused?.tabs.some((t) => routeKey(t) === k)) {
        return {
          ...prev,
          panes: prev.panes.map((p) =>
            p.id === prev.focusedPaneId
              ? { ...p, activeKey: k, tabs: p.tabs.map((t) => (routeKey(t) === k ? r : t)) }
              : p,
          ),
        };
      }
      const other = prev.panes.find((p) => p.tabs.some((t) => routeKey(t) === k));
      if (other) {
        return {
          ...prev,
          panes: prev.panes.map((p) =>
            p.id === other.id
              ? { ...p, activeKey: k, tabs: p.tabs.map((t) => (routeKey(t) === k ? r : t)) }
              : p,
          ),
          focusedPaneId: other.id,
        };
      }
      return {
        ...prev,
        panes: prev.panes.map((p) =>
          p.id === prev.focusedPaneId
            ? { ...p, tabs: [...p.tabs, r], activeKey: k }
            : p,
        ),
      };
    });
  };

  const addPane = () => {
    setLayout((prev) => {
      const id = nextPaneId();
      return {
        panes: [
          ...prev.panes,
          { id, tabs: [{ kind: "home" }], activeKey: "home" },
        ],
        paneSizes: { ...prev.paneSizes, [id]: 1 },
        focusedPaneId: id,
      };
    });
  };

  const closeTab = (paneId: PaneId, key: string) => {
    setLayout((prev) => {
      const updated = prev.panes.map((p) =>
        p.id === paneId ? removeTabFromPane(p, key) : p,
      );
      return compactLayout(prev, updated);
    });
  };

  const closePane = (paneId: PaneId) => {
    setLayout((prev) => {
      if (prev.panes.length === 1) return prev;
      const next = prev.panes.filter((p) => p.id !== paneId);
      return compactLayout(prev, next);
    });
  };

  const moveTab = (fromPaneId: PaneId, key: string, toPaneId: PaneId) => {
    if (fromPaneId === toPaneId) return;
    setLayout((prev) => {
      const from = prev.panes.find((p) => p.id === fromPaneId);
      const route = from?.tabs.find((t) => routeKey(t) === key);
      if (!from || !route) return prev;
      const updated = prev.panes.map((p) => {
        if (p.id === fromPaneId) return removeTabFromPane(p, key);
        if (p.id === toPaneId) {
          if (p.tabs.some((t) => routeKey(t) === key))
            return { ...p, activeKey: key };
          return { ...p, tabs: [...p.tabs, route], activeKey: key };
        }
        return p;
      });
      const next = compactLayout(prev, updated);
      return {
        ...next,
        focusedPaneId: next.panes.some((p) => p.id === toPaneId)
          ? toPaneId
          : next.focusedPaneId,
      };
    });
  };

  const moveTabToNewPane = (fromPaneId: PaneId, key: string) => {
    setLayout((prev) => {
      const from = prev.panes.find((p) => p.id === fromPaneId);
      const route = from?.tabs.find((t) => routeKey(t) === key);
      if (!from || !route) return prev;
      const newId = nextPaneId();
      const updated = prev.panes.map((p) =>
        p.id === fromPaneId ? removeTabFromPane(p, key) : p,
      );
      const compacted = compactLayout(prev, updated);
      return {
        panes: [...compacted.panes, { id: newId, tabs: [route], activeKey: key }],
        paneSizes: { ...compacted.paneSizes, [newId]: 1 },
        focusedPaneId: newId,
      };
    });
  };

  const onDividerMouseDown = (
    e: React.MouseEvent<HTMLDivElement>,
    leftId: PaneId,
    rightId: PaneId,
  ) => {
    e.preventDefault();
    const startX = e.clientX;
    const cw = containerRef.current?.getBoundingClientRect().width ?? 1;
    const totalWeight = Object.values(layout.paneSizes).reduce(
      (a, b) => a + b,
      0,
    );
    const startLeft = layout.paneSizes[leftId] ?? 1;
    const startRight = layout.paneSizes[rightId] ?? 1;
    const onMove = (ev: MouseEvent) => {
      const dx = ev.clientX - startX;
      const dxWeight = (dx / cw) * totalWeight;
      setLayout((prev) => ({
        ...prev,
        paneSizes: {
          ...prev.paneSizes,
          [leftId]: Math.max(0.15, startLeft + dxWeight),
          [rightId]: Math.max(0.15, startRight - dxWeight),
        },
      }));
    };
    const onUp = () => {
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
    };
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
  };

  // Loaders for app threads — register the thread in `threads[]` and return its id.
  // The AppViewer keeps its own nested-tab state for which threads to show in its side panel.
  const resolveExistingAppThread = async (
    appId: string,
  ): Promise<string | null> => {
    try {
      const pt = await invoke<ProjectThread>("read_app_thread", { appId });
      setThreads((prev) => upsertThread(prev, fromProjectThread(pt)));
      return pt.thread.meta.id;
    } catch (e) {
      console.error("[reflex] read_app_thread failed", e);
      return null;
    }
  };

  const resolveNewAppThread = async (
    appId: string,
  ): Promise<string | null> => {
    try {
      const pt = await invoke<ProjectThread>("create_app_thread", { appId });
      setThreads((prev) => upsertThread(prev, fromProjectThread(pt)));
      return pt.thread.meta.id;
    } catch (e) {
      console.error("[reflex] create_app_thread failed", e);
      return null;
    }
  };

  // Used by Inspector and Auto-error-fix: dispatch a revise with a prebuilt prompt.
  // Backend continues the latest app thread; we return its id so AppViewer can attach it.
  const applyAppRevise = async (
    appId: string,
    instruction: string,
  ): Promise<string | null> => {
    try {
      const res = await invoke<{ thread_id: string }>("app_revise", {
        appId,
        instruction,
      });
      // Make sure the topic is loaded into `threads[]` so TopicScreen can render it.
      try {
        const pt = await invoke<ProjectThread>("read_app_thread", { appId });
        setThreads((prev) => upsertThread(prev, fromProjectThread(pt)));
      } catch {}
      return res.thread_id;
    } catch (e) {
      console.error("[reflex] app_revise failed", e);
      return null;
    }
  };

  const createNewProject = async () => {
    try {
      const path = await invoke<string | null>("pick_directory", {
        title: "Выбор папки проекта",
      });
      if (!path) return;
      setNewProjectPath(path);
      setNewProjectDescription("");
    } catch (e) {
      console.error("[reflex] pick_directory failed", e);
    }
  };

  const submitNewProject = async (withDescription: boolean) => {
    if (!newProjectPath || creatingProject) return;
    setCreatingProject(true);
    try {
      const description = withDescription
        ? newProjectDescription.trim() || null
        : null;
      const p = await invoke<Project>("create_project", {
        root: newProjectPath,
        description,
      });
      setProjects((prev) => {
        const idx = prev.findIndex((x) => x.id === p.id);
        if (idx === -1) return [...prev, p];
        const copy = [...prev];
        copy[idx] = p;
        return copy;
      });
      setNewProjectPath(null);
      setNewProjectDescription("");
      navigate({ kind: "project", project_id: p.id });
      if (description) {
        try {
          const apps = await invoke<
            { id: string; name: string; icon?: string | null }[]
          >("list_apps");
          setInstalledAppsLite(apps);
        } catch (e) {
          console.warn("[reflex] list_apps for suggester failed", e);
        }
        setSuggesterProjectId(p.id);
      }
    } catch (e) {
      console.error("[reflex] create project failed", e);
    } finally {
      setCreatingProject(false);
    }
  };

  const openInSidePane = (url: string) => {
    setLayout((prev) => {
      const browserPane = prev.panes.find((p) =>
        p.tabs.some((t) => t.kind === "browser"),
      );
      if (browserPane) {
        return {
          ...prev,
          panes: prev.panes.map((p) =>
            p.id === browserPane.id
              ? { ...p, activeKey: "browser" }
              : p,
          ),
          focusedPaneId: browserPane.id,
        };
      }
      const id = nextPaneId();
      return {
        panes: [
          ...prev.panes,
          { id, tabs: [{ kind: "browser" } as Route], activeKey: "browser" },
        ],
        paneSizes: { ...prev.paneSizes, [id]: 1 },
        focusedPaneId: id,
      };
    });
    void invoke("browser_tab_open", { url }).catch((e) =>
      console.error("[reflex] browser_tab_open from link failed", e),
    );
  };

  const openLinkFromThread = (
    threadId: string,
    url: string,
    _ev: React.MouseEvent<HTMLAnchorElement>,
  ) => {
    const thread = threads.find((t) => t.id === threadId);
    if (
      !thread ||
      !activeProjectId ||
      thread.project_id !== activeProjectId
    ) {
      window.open(url, "_blank", "noopener,noreferrer");
      return;
    }
    openInSidePane(url);
  };

  const createNewTopic = async (
    projectId: string,
    prompt: string,
    planMode: boolean,
    options?: {
      source?: string;
      browserTabs?: BrowserTabSnapshot[];
    },
  ) => {
    const project = projects.find((p) => p.id === projectId);
    const ctx = {
      frontmost_app: null as string | null,
      finder_target: project?.root ?? null,
    };
    await invoke("submit_quick", {
      prompt,
      ctx,
      projectId,
      planMode,
      source: options?.source,
      browserTabs: options?.browserTabs,
    });
    // backend emits reflex://thread-created which our listener will route into the focused pane.
  };

  const focusedPane =
    layout.panes.find((p) => p.id === layout.focusedPaneId) ?? layout.panes[0];
  const currentRoute: Route =
    focusedPane.tabs.find((r) => routeKey(r) === focusedPane.activeKey) ??
    focusedPane.tabs[0] ?? { kind: "home" };

  useEffect(() => {
    let mounted = true;

    const refreshProjects = () => {
      invoke<Project[]>("list_projects")
        .then((p) => {
          if (mounted) setProjects(p);
        })
        .catch((e) => console.error("[reflex] list_projects failed", e));
    };

    refreshProjects();

    invoke<string | null>("get_active_project")
      .then((id) => {
        if (!mounted) return;
        setActiveProjectIdState(id ?? null);
      })
      .catch((e) => console.warn("[reflex] get_active_project failed", e));

    invoke<ProjectThread[]>("list_threads")
      .then((stored) => {
        if (!mounted) return;
        setThreads((prev) => {
          let next = prev;
          for (const s of stored) {
            next = upsertThread(next, fromProjectThread(s));
          }
          next = next.slice().sort((a, b) => a.created_at_ms - b.created_at_ms);
          return next;
        });
      })
      .catch((e) => console.error("[reflex] list_threads failed", e));

    const created = listen<ThreadCreated>("reflex://thread-created", (e) => {
      const t: Thread = {
        ...e.payload,
        events: [],
        exit_code: undefined,
        done: false,
        session_id: null,
        title: null,
        goal: null,
        pending_questions: [],
        plan_mode: !!e.payload.plan_mode,
        plan_confirmed: false,
        source: e.payload.source ?? "quick",
        browser_tabs: e.payload.browser_tabs ?? [],
      };
      setThreads((prev) => upsertThread(prev, t));
      // refresh project list (project may have just been created) and jump to topic
      refreshProjects();
      navigate({ kind: "topic", thread_id: t.id });
    });
    const metaUpdated = listen<ThreadMetaUpdated>(
      "reflex://thread-meta-updated",
      (e) => {
        setThreads((prev) =>
          prev.map((t) =>
            t.id === e.payload.thread_id
              ? {
                  ...t,
                  title: e.payload.title ?? t.title,
                  goal: e.payload.goal ?? t.goal,
                  plan_confirmed:
                    e.payload.plan_confirmed ?? t.plan_confirmed,
                }
              : t,
          ),
        );
      },
    );
    const question = listen<ThreadQuestion>("reflex://thread-question", (e) => {
      const q = e.payload;
      if (!q.thread_id) {
        console.warn("[reflex] question without thread_id", q);
        return;
      }
      const tid = q.thread_id;
      setThreads((prev) =>
        prev.map((t) =>
          t.id === tid
            ? { ...t, pending_questions: [...(t.pending_questions ?? []), q] }
            : t,
        ),
      );
      navigate({ kind: "topic", thread_id: tid });
    });
    const evt = listen<CodexEventPayload>("reflex://codex-event", (e) => {
      const ev: ThreadEvent = {
        seq: e.payload.seq,
        stream: e.payload.stream,
        raw: e.payload.raw,
        parsed: tryParse(e.payload.raw),
      };
      setThreads((prev) => appendEvent(prev, e.payload.thread_id, ev));
    });
    const end = listen<CodexEndPayload>("reflex://codex-end", (e) => {
      setThreads((prev) =>
        prev.map((t) =>
          t.id === e.payload.thread_id
            ? { ...t, exit_code: e.payload.exit_code, done: true }
            : t,
        ),
      );
    });
    const running = listen<ThreadRunningPayload>(
      "reflex://thread-running",
      (e) => {
        setThreads((prev) =>
          prev.map((t) =>
            t.id === e.payload.thread_id
              ? { ...t, done: false, exit_code: undefined }
              : t,
          ),
        );
      },
    );

    const onResolved = (e: Event) => {
      const detail = (e as CustomEvent).detail as {
        thread_id: string;
        question_id: string;
      };
      setThreads((prev) =>
        prev.map((t) =>
          t.id === detail.thread_id
            ? {
                ...t,
                pending_questions: (t.pending_questions ?? []).filter(
                  (q) => q.question_id !== detail.question_id,
                ),
              }
            : t,
        ),
      );
    };
    window.addEventListener("reflex-question-resolved", onResolved);

    return () => {
      mounted = false;
      created.then((u) => u());
      evt.then((u) => u());
      end.then((u) => u());
      running.then((u) => u());
      metaUpdated.then((u) => u());
      question.then((u) => u());
      window.removeEventListener("reflex-question-resolved", onResolved);
    };
  }, []);

  const onProjectUpdated = (p: Project) =>
    setProjects((prev) => {
      const idx = prev.findIndex((x) => x.id === p.id);
      if (idx === -1) return [...prev, p];
      const copy = [...prev];
      copy[idx] = p;
      return copy;
    });

  useEffect(() => {
    if (projects.length === 0) {
      if (activeProjectId !== null) setActiveProjectIdState(null);
      return;
    }
    if (
      activeProjectId &&
      projects.some((p) => p.id === activeProjectId)
    ) {
      return;
    }
    void setActiveProject(projects[0].id);
  }, [projects, activeProjectId]);

  const projectsLoadedRef = useRef(false);
  const firstRunPromptedRef = useRef(false);
  useEffect(() => {
    if (firstRunPromptedRef.current) return;
    if (!projectsLoadedRef.current) {
      projectsLoadedRef.current = true;
      return;
    }
    if (projects.length === 0 && !newProjectPath && !creatingProject) {
      firstRunPromptedRef.current = true;
      void createNewProject();
    }
  }, [projects, newProjectPath, creatingProject]);

  const openAppIds = useMemo(() => {
    const ids = new Set<string>();
    for (const pane of layout.panes) {
      for (const tab of pane.tabs) {
        if (tab.kind === "app") ids.add(tab.app_id);
      }
    }
    return ids;
  }, [layout.panes]);

  const renderRoute = (r: Route) => {
    switch (r.kind) {
      case "home":
        return (
          <HomeScreen
            projects={projects}
            threads={threads}
            openAppIds={openAppIds}
            onSelectProject={(id) =>
              navigate({ kind: "project", project_id: id })
            }
            onSelectTopic={(id) => navigate({ kind: "topic", thread_id: id })}
            onSelectApp={(id) => navigate({ kind: "app", app_id: id })}
            onOpenApps={() => navigate({ kind: "apps" })}
            onCreateProject={() => void createNewProject()}
          />
        );
      case "project":
        return (
          <ProjectScreen
            projectId={r.project_id}
            projects={projects}
            threads={threads}
            onSelectTopic={(id) => navigate({ kind: "topic", thread_id: id })}
            onProjectUpdated={onProjectUpdated}
            onCreateTopic={(prompt, planMode) =>
              createNewTopic(r.project_id, prompt, planMode)
            }
            onOpenApp={(id) => navigate({ kind: "app", app_id: id })}
          />
        );
      case "topic":
        return (
          <TopicScreen
            thread_id={r.thread_id}
            threads={threads}
            projects={projects}
            onOpenLink={openLinkFromThread}
          />
        );
      case "apps":
        return (
          <AppsScreen
            initialTemplate={r.initialTemplate}
            openCreate={r.openCreate}
            createRequestId={r.createRequestId}
            onOpenApp={(id) => navigate({ kind: "app", app_id: id })}
            onOpenTopic={(id) => navigate({ kind: "topic", thread_id: id })}
          />
        );
      case "app":
        return (
          <AppViewer
            appId={r.app_id}
            threads={threads}
            onResolveExistingThread={() => resolveExistingAppThread(r.app_id)}
            onResolveNewThread={() => resolveNewAppThread(r.app_id)}
            onApplyRevise={(instr) => applyAppRevise(r.app_id, instr)}
            onDeleted={() => navigate({ kind: "apps" })}
          />
        );
      case "memory": {
        const thread = r.thread_id
          ? threads.find((t) => t.id === r.thread_id)
          : null;
        const projectId = r.project_id ?? thread?.project_id;
        const project = projectId
          ? projects.find((p) => p.id === projectId)
          : null;
        const projectRoot = project?.root ?? thread?.cwd ?? null;
        return (
          <MemoryPanel
            projectRoot={projectRoot}
            threadId={thread?.id ?? null}
            initialScope={thread ? "topic" : project ? "project" : "global"}
            initialView={thread ? "recall" : "notes"}
            initialRecallQuery={thread ? mostRecentTopicPrompt(thread) : ""}
          />
        );
      }
      case "automations":
        return (
          <AutomationsScreen
            onCreateAutomation={() =>
              navigate({
                kind: "apps",
                initialTemplate: "automation",
                openCreate: true,
                createRequestId: Date.now(),
              })
            }
          />
        );
      case "browser":
        return (
          <BrowserScreen
            key={activeProjectId ?? "_none"}
            projectId={activeProjectId}
            projectName={
              projects.find((p) => p.id === activeProjectId)?.name ?? null
            }
            onStartChat={async (prompt, browserTabs) => {
              if (!activeProjectId) return;
              await createNewTopic(activeProjectId, prompt, false, {
                source: "browser",
                browserTabs,
              });
            }}
          />
        );
      case "settings":
        return <SettingsScreen />;
    }
  };

  return (
    <div className="chat-root">
      <div className="chat-titlebar" data-tauri-drag-region />
      <Header
        route={currentRoute}
        threads={threads}
        projects={projects}
        activeProjectId={activeProjectId}
        onSelectActiveProject={(id) => void setActiveProject(id)}
        onNavigate={navigate}
        onAddPane={addPane}
        onCreateProject={() => void createNewProject()}
      />
      <div className="panes-container" ref={containerRef}>
        {layout.panes.map((pane, idx) => (
          <Fragment key={pane.id}>
            {idx > 0 && (
              <div
                className="pane-divider"
                onMouseDown={(e) =>
                  onDividerMouseDown(e, layout.panes[idx - 1].id, pane.id)
                }
              />
            )}
            <PaneView
              pane={pane}
              size={layout.paneSizes[pane.id] ?? 1}
              focused={pane.id === layout.focusedPaneId}
              canClose={layout.panes.length > 1}
              projects={projects}
              threads={threads}
              renderRoute={renderRoute}
              onActivateTab={(key) => activateTab(pane.id, key)}
              onCloseTab={(key) => closeTab(pane.id, key)}
              onClosePane={() => closePane(pane.id)}
              onFocus={() => focusPane(pane.id)}
              onTabDragStart={() => setDraggingTab(true)}
              onTabDragEnd={() => setDraggingTab(false)}
              onTabDrop={(fromPaneId, key) =>
                moveTab(fromPaneId, key, pane.id)
              }
            />
          </Fragment>
        ))}
        <NewPaneDropZone
          active={draggingTab}
          onDrop={(fromPaneId, key) => {
            setDraggingTab(false);
            moveTabToNewPane(fromPaneId, key);
          }}
        />
      </div>
      {newProjectPath && (
        <div
          className="modal-backdrop"
          onClick={() => !creatingProject && setNewProjectPath(null)}
        >
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <h2 className="modal-title">Новый проект</h2>
            <p className="modal-hint">
              <code>{newProjectPath}</code>
              <br />
              Опиши зачем этот проект и что хочешь делать. Это поможет агенту
              предлагать утилиты и подсказки.
            </p>
            <textarea
              className="modal-input"
              placeholder="Например: следить за весом и активностью; собирать новости по AI каждое утро…"
              value={newProjectDescription}
              onChange={(e) => setNewProjectDescription(e.currentTarget.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                  e.preventDefault();
                  void submitNewProject(true);
                }
              }}
              autoFocus
              rows={5}
            />
            <div className="modal-actions">
              <button
                className="modal-btn"
                disabled={creatingProject}
                onClick={() => void submitNewProject(false)}
              >
                Пропустить
              </button>
              <button
                className="modal-btn modal-btn-primary"
                disabled={creatingProject || !newProjectDescription.trim()}
                onClick={() => void submitNewProject(true)}
              >
                {creatingProject ? "Создаю…" : "Создать (⌘↵)"}
              </button>
            </div>
          </div>
        </div>
      )}
      {suggesterProjectId && (
        <SuggesterModal
          projectId={suggesterProjectId}
          installedApps={installedAppsLite}
          onClose={() => setSuggesterProjectId(null)}
          onApplied={() => {
            void invoke<Project[]>("list_projects").then((list) =>
              setProjects(list),
            );
          }}
        />
      )}
    </div>
  );
}

function Header({
  route,
  threads,
  projects,
  activeProjectId,
  onSelectActiveProject,
  onNavigate,
  onAddPane,
  onCreateProject,
}: {
  route: Route;
  threads: Thread[];
  projects: Project[];
  activeProjectId: string | null;
  onSelectActiveProject: (id: string) => void;
  onNavigate: (r: Route) => void;
  onAddPane: () => void;
  onCreateProject: () => void;
}) {
  const crumbs: { label: string; route: Route | null }[] = [
    { label: "Reflex", route: { kind: "home" } },
  ];
  if (route.kind === "project") {
    const p = projects.find((x) => x.id === route.project_id);
    crumbs.push({ label: p?.name ?? route.project_id, route: null });
  } else if (route.kind === "topic") {
    const t = threads.find((x) => x.id === route.thread_id);
    if (t) {
      crumbs.push({
        label: t.project_name,
        route: { kind: "project", project_id: t.project_id },
      });
      crumbs.push({ label: t.id, route: null });
    } else {
      crumbs.push({ label: route.thread_id, route: null });
    }
  } else if (route.kind === "apps") {
    crumbs.push({ label: "Apps", route: null });
  } else if (route.kind === "app") {
    crumbs.push({ label: "Apps", route: { kind: "apps" } });
    crumbs.push({ label: route.app_id, route: null });
  } else if (route.kind === "memory") {
    const thread = route.thread_id
      ? threads.find((x) => x.id === route.thread_id)
      : null;
    const projectId = route.project_id ?? thread?.project_id;
    if (projectId) {
      const p = projects.find((x) => x.id === projectId);
      crumbs.push({
        label: p?.name ?? projectId,
        route: { kind: "project", project_id: projectId },
      });
    }
    if (thread) {
      crumbs.push({
        label: thread.title ?? thread.prompt.slice(0, 32) ?? thread.id,
        route: { kind: "topic", thread_id: thread.id },
      });
    }
    crumbs.push({ label: "Memory", route: null });
  }

  return (
    <header className="chat-header">
      <nav className="chat-breadcrumbs">
        {crumbs.map((c, i) => (
          <span key={i} className="chat-crumb">
            {c.route ? (
              <button
                className="chat-crumb-link"
                onClick={() => onNavigate(c.route!)}
              >
                {c.label}
              </button>
            ) : (
              <span className="chat-crumb-current">{c.label}</span>
            )}
            {i < crumbs.length - 1 && <span className="chat-crumb-sep">›</span>}
          </span>
        ))}
      </nav>
      <div className="chat-header-actions">
        <button
          className="header-tab"
          onClick={onCreateProject}
          title="Новый проект"
        >
          + Project
        </button>
        <button
          className="header-tab"
          onClick={onAddPane}
          title="Добавить панель"
        >
          + Pane
        </button>
        <button
          className={`header-tab ${route.kind === "apps" || route.kind === "app" ? "active" : ""}`}
          onClick={() => onNavigate({ kind: "apps" })}
        >
          Apps
        </button>
        <button
          className={`header-tab ${route.kind === "memory" ? "active" : ""}`}
          onClick={() => {
            const routeThreadId =
              route.kind === "topic" || route.kind === "memory"
                ? route.thread_id
                : undefined;
            const activeThread = routeThreadId
              ? threads.find((t) => t.id === routeThreadId)
              : null;
            const projectId =
              route.kind === "project"
                ? route.project_id
                : activeThread
                  ? activeThread.project_id
                  : route.kind === "memory"
                    ? route.project_id
                    : undefined;
            onNavigate({
              kind: "memory",
              project_id: projectId,
              thread_id: activeThread?.id,
            });
          }}
          title="Memory"
        >
          Memory
        </button>
        <button
          className={`header-tab ${route.kind === "automations" ? "active" : ""}`}
          onClick={() => onNavigate({ kind: "automations" })}
          title="Расписания и история запусков"
        >
          Automations
        </button>
        <select
          className="header-tab header-project-select"
          value={activeProjectId ?? ""}
          onChange={(e) => {
            const v = e.currentTarget.value;
            if (v) onSelectActiveProject(v);
          }}
          disabled={projects.length === 0}
          title="Активный проект"
        >
          {projects.length === 0 ? (
            <option value="">Нет проектов</option>
          ) : (
            <>
              {!activeProjectId && (
                <option value="" disabled>
                  Выбери проект
                </option>
              )}
              {projects.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
            </>
          )}
        </select>
        <button
          className={`header-tab ${route.kind === "browser" ? "active" : ""}`}
          onClick={() => onNavigate({ kind: "browser" })}
          title="Встроенный браузер"
        >
          Browser
        </button>
        <button
          className={`header-tab ${route.kind === "settings" ? "active" : ""}`}
          onClick={() => onNavigate({ kind: "settings" })}
          title="Настройки и логи"
        >
          ⚙
        </button>
        <span className="chat-subtitle">
          {threads.length} thread{threads.length === 1 ? "" : "s"} ·{" "}
          {projects.length} project{projects.length === 1 ? "" : "s"}
        </span>
      </div>
    </header>
  );
}

function PaneView({
  pane,
  size,
  focused,
  canClose,
  projects,
  threads,
  renderRoute,
  onActivateTab,
  onCloseTab,
  onClosePane,
  onFocus,
  onTabDragStart,
  onTabDragEnd,
  onTabDrop,
}: {
  pane: Pane;
  size: number;
  focused: boolean;
  canClose: boolean;
  projects: Project[];
  threads: Thread[];
  renderRoute: (r: Route) => React.ReactNode;
  onActivateTab: (key: string) => void;
  onCloseTab: (key: string) => void;
  onClosePane: () => void;
  onFocus: () => void;
  onTabDragStart: () => void;
  onTabDragEnd: () => void;
  onTabDrop: (fromPaneId: PaneId, key: string) => void;
}) {
  const onDragOver = (e: React.DragEvent<HTMLDivElement>) => {
    if (e.dataTransfer.types.includes(TAB_DRAG_TYPE)) {
      e.preventDefault();
      e.dataTransfer.dropEffect = "move";
    }
  };
  const onDrop = (e: React.DragEvent<HTMLDivElement>) => {
    const data = e.dataTransfer.getData(TAB_DRAG_TYPE);
    if (!data) return;
    e.preventDefault();
    try {
      const { paneId, key } = JSON.parse(data);
      onTabDrop(paneId, key);
    } catch {}
  };
  return (
    <div
      className={`pane ${focused ? "pane-focused" : ""}`}
      style={{ flex: size }}
      onMouseDownCapture={onFocus}
      onDragOver={onDragOver}
      onDrop={onDrop}
    >
      <PaneTabsRow
        paneId={pane.id}
        tabs={pane.tabs}
        activeKey={pane.activeKey}
        projects={projects}
        threads={threads}
        canClosePane={canClose}
        onActivate={onActivateTab}
        onClose={onCloseTab}
        onClosePane={onClosePane}
        onTabDragStart={onTabDragStart}
        onTabDragEnd={onTabDragEnd}
      />
      <main className="pane-body">
        {pane.tabs.map((r) => {
          const k = routeKey(r);
          return (
            <div key={k} className="route-pane" hidden={k !== pane.activeKey}>
              {renderRoute(r)}
            </div>
          );
        })}
      </main>
    </div>
  );
}

function PaneTabsRow({
  paneId,
  tabs,
  activeKey,
  projects,
  threads,
  canClosePane,
  onActivate,
  onClose,
  onClosePane,
  onTabDragStart,
  onTabDragEnd,
}: {
  paneId: PaneId;
  tabs: Route[];
  activeKey: string;
  projects: Project[];
  threads: Thread[];
  canClosePane: boolean;
  onActivate: (key: string) => void;
  onClose: (key: string) => void;
  onClosePane: () => void;
  onTabDragStart: () => void;
  onTabDragEnd: () => void;
}) {
  return (
    <nav className="tabs-row">
      {tabs.map((r) => {
        const k = routeKey(r);
        const active = k === activeKey;
        const label = tabLabel(r, projects, threads);
        return (
          <div
            key={k}
            className={`tab ${active ? "active" : ""}`}
            draggable
            onDragStart={(e) => {
              e.dataTransfer.setData(
                TAB_DRAG_TYPE,
                JSON.stringify({ paneId, key: k }),
              );
              e.dataTransfer.effectAllowed = "move";
              onTabDragStart();
            }}
            onDragEnd={onTabDragEnd}
            onClick={() => onActivate(k)}
            onMouseDown={(e) => {
              if (e.button === 1) {
                e.preventDefault();
                onClose(k);
              }
            }}
            title={label}
          >
            <span className="tab-icon">{tabIcon(r)}</span>
            <span className="tab-label">{label}</span>
            <button
              className="tab-close"
              onClick={(e) => {
                e.stopPropagation();
                onClose(k);
              }}
              title="Закрыть"
              aria-label="Закрыть таб"
            >
              ×
            </button>
          </div>
        );
      })}
      {canClosePane && (
        <button
          className="pane-close-btn"
          onClick={onClosePane}
          title="Закрыть панель"
          aria-label="Закрыть панель"
        >
          ⨯
        </button>
      )}
    </nav>
  );
}

function NewPaneDropZone({
  active,
  onDrop,
}: {
  active: boolean;
  onDrop: (fromPaneId: PaneId, key: string) => void;
}) {
  const [over, setOver] = useState(false);
  return (
    <div
      className={`pane-newzone ${active ? "armed" : ""} ${over ? "over" : ""}`}
      onDragOver={(e) => {
        if (e.dataTransfer.types.includes(TAB_DRAG_TYPE)) {
          e.preventDefault();
          e.dataTransfer.dropEffect = "move";
          if (!over) setOver(true);
        }
      }}
      onDragLeave={() => setOver(false)}
      onDrop={(e) => {
        const data = e.dataTransfer.getData(TAB_DRAG_TYPE);
        setOver(false);
        if (!data) return;
        e.preventDefault();
        try {
          const { paneId, key } = JSON.parse(data);
          onDrop(paneId, key);
        } catch {}
      }}
    />
  );
}

const TEMPLATES: {
  id: string;
  icon: string;
  name: string;
  description: string;
  placeholder: string;
  badges: string[];
}[] = [
  {
    id: "blank",
    icon: "📄",
    name: "Blank",
    description: "Пустой app, codex решает структуру",
    placeholder:
      "Например: счётчик с кнопкой сохранения в storage; виджет погоды; …",
    badges: ["static", "custom"],
  },
  {
    id: "chat",
    icon: "💬",
    name: "Chat utility",
    description: "Чат с агентом, стриминг ответа",
    placeholder:
      "Например: ассистент по моему календарю; помощник с переводом; …",
    badges: ["agent.stream", "storage"],
  },
  {
    id: "dashboard",
    icon: "📊",
    name: "Dashboard",
    description: "Данные через agent.task в виде таблицы/карточек",
    placeholder:
      "Например: статус всех проектов из ~/projects; список последних коммитов; …",
    badges: ["agent.task", "cards/table"],
  },
  {
    id: "form",
    icon: "📝",
    name: "Form tool",
    description: "Поля → Run → результат через agent.task",
    placeholder:
      "Например: переписать текст в нужном стиле; сгенерить regex по описанию; …",
    badges: ["form", "agent.task"],
  },
  {
    id: "api-client",
    icon: "🌐",
    name: "API client",
    description: "Запросы к внешнему API через net.fetch",
    placeholder:
      "Например: показать issues из github.com/owner/repo; конвертер валют через open.er-api.com; …",
    badges: ["net.fetch", "network"],
  },
  {
    id: "automation",
    icon: "⏱",
    name: "Automation",
    description: "Расписание, action и виджет для фоновой задачи",
    placeholder:
      "Например: раз в час проверять важные письма и сохранять краткую сводку; каждое утро собирать статус проектов; …",
    badges: ["schedules", "actions", "widgets"],
  },
  {
    id: "node-server",
    icon: "🚀",
    name: "Node server",
    description: "runtime=server: своё backend на Node.js stdlib",
    placeholder:
      "Например: WebSocket-чат комната; sqlite-просмотрщик; превью markdown; …",
    badges: ["server", "stdlib"],
  },
];

const SKILL_PRESETS = [
  {
    id: "build-web-apps:frontend-app-builder",
    label: "Web apps",
  },
  {
    id: "build-web-apps:react-best-practices",
    label: "React",
  },
  {
    id: "playwright",
    label: "Browser QA",
  },
  {
    id: "openai-docs",
    label: "OpenAI docs",
  },
  {
    id: "github:gh-fix-ci",
    label: "GitHub CI",
  },
  {
    id: "build-ios-apps:ios-debugger-agent",
    label: "iOS debug",
  },
  {
    id: "build-macos-apps:build-run-debug",
    label: "macOS debug",
  },
  {
    id: "game-studio:game-playtest",
    label: "Game QA",
  },
] as const;

type TrashEntry = {
  trash_id: string;
  original_id: string;
  original_name: string;
  original_icon: string | null;
  original_description: string | null;
  deleted_at_ms: number;
  original_root: string;
};

function formatAgo(ms: number): string {
  if (ms < 60_000) return "только что";
  const min = Math.floor(ms / 60_000);
  if (min < 60) return `${min} мин назад`;
  const h = Math.floor(min / 60);
  if (h < 24) return `${h} ч назад`;
  const d = Math.floor(h / 24);
  return `${d} дн назад`;
}

function AppsScreen({
  initialTemplate,
  openCreate,
  createRequestId,
  onOpenApp,
  onOpenTopic,
}: {
  initialTemplate?: string;
  openCreate?: boolean;
  createRequestId?: number;
  onOpenApp: (id: string) => void;
  onOpenTopic: (id: string) => void;
}) {
  const [items, setItems] = useState<AppManifest[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [importing, setImporting] = useState(false);
  const [showModal, setShowModal] = useState(false);
  const [step, setStep] = useState<"template" | "describe">("template");
  const [template, setTemplate] = useState<string>("blank");
  const [description, setDescription] = useState("");
  const [trash, setTrash] = useState<TrashEntry[]>([]);
  const [showTrash, setShowTrash] = useState(false);
  const [busyId, setBusyId] = useState<string | null>(null);

  useEffect(() => {
    if (!openCreate) return;
    const nextTemplate = TEMPLATES.some((t) => t.id === initialTemplate)
      ? initialTemplate!
      : "blank";
    setTemplate(nextTemplate);
    setStep("describe");
    setShowModal(true);
  }, [initialTemplate, openCreate, createRequestId]);

  async function importBundle() {
    if (importing) return;
    setImporting(true);
    setError(null);
    try {
      const path = await invoke<string | null>("pick_open_file", {
        title: "Импорт .reflexapp",
        filterName: "Reflex App",
        filterExtensions: ["reflexapp", "zip"],
      });
      if (!path) return;
      const manifest = await invoke<AppManifest>("app_import", {
        zipPath: path,
      });
      setShowModal(false);
      setTimeout(() => void refresh(), 500);
      onOpenApp(manifest.id);
    } catch (e) {
      setError(String(e));
    } finally {
      setImporting(false);
    }
  }

  async function refresh() {
    try {
      const [list, t] = await Promise.all([
        invoke<AppManifest[]>("list_apps"),
        invoke<TrashEntry[]>("list_trashed_apps"),
      ]);
      setItems(list);
      setTrash(t);
    } catch (e) {
      setError(String(e));
    }
  }

  async function deleteApp(appId: string, appName: string) {
    if (busyId) return;
    if (!window.confirm(`Переместить "${appName}" в корзину?`)) return;
    setBusyId(appId);
    setError(null);
    try {
      await invoke("delete_app", { appId });
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyId(null);
    }
  }

  async function restoreApp(trashId: string) {
    if (busyId) return;
    setBusyId(trashId);
    setError(null);
    try {
      await invoke("restore_app", { trashId });
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyId(null);
    }
  }

  async function purgeApp(trashId: string, name: string) {
    if (busyId) return;
    if (
      !window.confirm(`Удалить "${name}" окончательно? Это действие необратимо.`)
    )
      return;
    setBusyId(trashId);
    setError(null);
    try {
      await invoke("purge_trashed_app", { trashId });
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyId(null);
    }
  }

  useEffect(() => {
    let alive = true;
    let timer: ReturnType<typeof setInterval> | null = null;
    const tick = () => {
      Promise.all([
        invoke<AppManifest[]>("list_apps"),
        invoke<TrashEntry[]>("list_trashed_apps"),
      ])
        .then(([list, t]) => {
          if (!alive) return;
          setItems(list);
          setTrash(t);
          const stillCreating = list.some((a) => a.ready === false);
          if (!stillCreating && timer) {
            clearInterval(timer);
            timer = null;
          }
        })
        .catch((e) => alive && setError(String(e)));
    };
    tick();
    timer = setInterval(tick, 3000);
    const u = listen("reflex://apps-changed", () => tick());
    return () => {
      alive = false;
      if (timer) clearInterval(timer);
      u.then((un) => un());
    };
  }, []);

  async function submitCreate() {
    const text = description.trim();
    if (!text || creating) return;
    setCreating(true);
    setError(null);
    try {
      const res = await invoke<{ app_id: string; thread_id: string }>(
        "create_app",
        { description: text, template },
      );
      setShowModal(false);
      setDescription("");
      setStep("template");
      setTemplate("blank");
      // refresh list a bit later (codex still working)
      setTimeout(() => void refresh(), 1500);
      // navigate to creation thread so user sees codex working
      onOpenTopic(res.thread_id);
    } catch (e) {
      console.error("[reflex] create_app failed", e);
      setError(String(e));
    } finally {
      setCreating(false);
    }
  }

  return (
    <div className="apps-root">
      <header className="apps-header">
        <div className="apps-header-row">
          <h1 className="section-title">Apps</h1>
          <div className="apps-header-buttons">
            <button
              className="apps-create-btn"
              onClick={() => setShowModal(true)}
            >
              + New App
            </button>
            <button
              className="apps-trash-btn"
              onClick={() => setShowTrash((v) => !v)}
              title="Удалённые приложения"
            >
              🗑 Корзина{trash.length > 0 ? ` (${trash.length})` : ""}
            </button>
          </div>
        </div>
        <p className="apps-hint">
          Утилиты, общающиеся с агентом через мост Reflex. Опиши что хочешь —
          codex напишет.
        </p>
      </header>
      {error && <div className="apps-error">{error}</div>}
      {showTrash && (
        <section className="apps-trash">
          <h3 className="apps-trash-title">Корзина</h3>
          {trash.length === 0 ? (
            <div className="apps-trash-empty">Пусто.</div>
          ) : (
            <ul className="apps-trash-list">
              {trash.map((t) => {
                const ageMs = Date.now() - t.deleted_at_ms;
                const ageStr = formatAgo(ageMs);
                return (
                  <li key={t.trash_id} className="apps-trash-row">
                    <span className="apps-trash-icon">
                      {t.original_icon ?? "🧩"}
                    </span>
                    <div className="apps-trash-info">
                      <div className="apps-trash-name">{t.original_name}</div>
                      <div className="apps-trash-meta">
                        удалено {ageStr} ·{" "}
                        <code>{t.original_id}</code>
                      </div>
                    </div>
                    <div className="apps-trash-actions">
                      <button
                        className="apps-trash-action"
                        disabled={busyId === t.trash_id}
                        onClick={() => void restoreApp(t.trash_id)}
                        title="Восстановить"
                      >
                        ↩ Восстановить
                      </button>
                      <button
                        className="apps-trash-action apps-trash-purge"
                        disabled={busyId === t.trash_id}
                        onClick={() =>
                          void purgeApp(t.trash_id, t.original_name)
                        }
                        title="Удалить навсегда"
                      >
                        ✕ Навсегда
                      </button>
                    </div>
                  </li>
                );
              })}
            </ul>
          )}
        </section>
      )}
      {items.length === 0 ? (
        <div className="chat-empty">
          <p>Apps пока нет.</p>
        </div>
      ) : (
        <div className="apps-grid">
          {items.map((a) => {
            const isReady = a.ready !== false;
            const capabilityFacts = buildAppCatalogCapabilityFacts(a);
            return (
              <div
                key={a.id}
                role="button"
                tabIndex={0}
                className={`apps-card ${isReady ? "" : "apps-card-creating"}`}
                onClick={() => onOpenApp(a.id)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") onOpenApp(a.id);
                }}
                title={isReady ? "Открыть" : "Codex ещё пишет файлы — клик чтобы посмотреть"}
              >
                <button
                  className="apps-card-delete"
                  onClick={(ev) => {
                    ev.stopPropagation();
                    void deleteApp(a.id, a.name);
                  }}
                  disabled={busyId === a.id}
                  title="В корзину"
                >
                  ✕
                </button>
                <div className="apps-card-icon">{a.icon ?? "🧩"}</div>
                <div className="apps-card-name">
                  {a.name}
                  {!isReady && (
                    <span className="apps-card-badge">creating…</span>
                  )}
                </div>
                {a.description && (
                  <div className="apps-card-desc">{a.description}</div>
                )}
                {capabilityFacts.length > 0 && (
                  <div className="apps-card-capabilities">
                    {capabilityFacts.map((fact) => (
                      <span
                        key={fact.key}
                        className="apps-capability"
                        title={fact.title}
                      >
                        <span className="apps-capability-label">
                          {fact.label}
                        </span>
                        <span className="apps-capability-value">
                          {fact.value}
                        </span>
                      </span>
                    ))}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}
      {showModal && (
        <div
          className="modal-backdrop"
          onClick={() => !creating && setShowModal(false)}
        >
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            {step === "template" ? (
              <>
                <h2 className="modal-title">Новый Reflex app</h2>
                <p className="modal-hint">Выбери шаблон под задачу.</p>
                <div className="template-grid">
                  {TEMPLATES.map((t) => (
                    <button
                      key={t.id}
                      className={`template-card ${template === t.id ? "active" : ""}`}
                      onClick={() => setTemplate(t.id)}
                    >
                      <div className="template-icon">{t.icon}</div>
                      <div className="template-name">{t.name}</div>
                      <div className="template-desc">{t.description}</div>
                      <div className="template-badges">
                        {t.badges.map((badge) => (
                          <span key={badge}>{badge}</span>
                        ))}
                      </div>
                    </button>
                  ))}
                </div>
                <div className="modal-actions">
                  <button
                    className="modal-btn"
                    onClick={() => setShowModal(false)}
                  >
                    Отмена
                  </button>
                  <button
                    className="modal-btn"
                    onClick={() => void importBundle()}
                    disabled={importing}
                    title="Импортировать .reflexapp бандл"
                  >
                    {importing ? "…" : "📥 Import .reflexapp"}
                  </button>
                  <button
                    className="modal-btn modal-btn-primary"
                    onClick={() => setStep("describe")}
                  >
                    Дальше →
                  </button>
                </div>
              </>
            ) : (
              <>
                <h2 className="modal-title">
                  {TEMPLATES.find((t) => t.id === template)?.icon}{" "}
                  {TEMPLATES.find((t) => t.id === template)?.name}
                </h2>
                <p className="modal-hint">
                  Опиши что должен делать app. Codex напишет файлы в фоне.
                </p>
                <textarea
                  className="modal-input"
                  placeholder={
                    TEMPLATES.find((t) => t.id === template)?.placeholder ?? ""
                  }
                  value={description}
                  onChange={(e) => setDescription(e.currentTarget.value)}
                  autoFocus
                  rows={5}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                      e.preventDefault();
                      void submitCreate();
                    }
                  }}
                />
                <div className="modal-actions">
                  <button
                    className="modal-btn"
                    disabled={creating}
                    onClick={() => setStep("template")}
                  >
                    ← Назад
                  </button>
                  <button
                    className="modal-btn"
                    disabled={creating}
                    onClick={() => setShowModal(false)}
                  >
                    Отмена
                  </button>
                  <button
                    className="modal-btn modal-btn-primary"
                    disabled={creating || !description.trim()}
                    onClick={() => void submitCreate()}
                  >
                    {creating ? "Создаю…" : "Создать (⌘↵)"}
                  </button>
                </div>
              </>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

type AppStatus = {
  has_changes: boolean;
  revision: number;
  last_commit_message: string | null;
  entry_exists: boolean;
};

type InspectorPick = {
  selector: string;
  tagName: string;
  id: string | null;
  classes: string[];
  text: string;
  outerHTML: string;
  computedStyle: Record<string, string>;
};

type RuntimeErrorPayload = {
  message: string;
  filename: string;
  lineno: number;
  colno: number;
  stack: string;
};

function AppViewer({
  appId,
  threads,
  onResolveExistingThread,
  onResolveNewThread,
  onApplyRevise,
  onDeleted,
}: {
  appId: string;
  threads: Thread[];
  onResolveExistingThread: () => Promise<string | null>;
  onResolveNewThread: () => Promise<string | null>;
  onApplyRevise: (instruction: string) => Promise<string | null>;
  onDeleted?: () => void;
}) {
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const rootRef = useRef<HTMLDivElement>(null);
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [reloadKey, setReloadKey] = useState(0);
  const [busy, setBusy] = useState<null | "save" | "revert" | "restart">(null);
  const [error, setError] = useState<string | null>(null);
  const prevHasChangesRef = useRef(false);
  const [manifest, setManifest] = useState<AppManifest | null>(null);
  const [serverPort, setServerPort] = useState<number | null>(null);
  const [serverState, setServerState] = useState<
    "idle" | "starting" | "running" | "failed" | "crashed"
  >("idle");
  const [serverError, setServerError] = useState<string | null>(null);
  const [logs, setLogs] = useState<ServerLogLine[]>([]);
  const [logsOpen, setLogsOpen] = useState(false);
  const isServerRuntimeRef = useRef(false);

  // Nested tabs (threads attached inside this app's view).
  const [nestedTabs, setNestedTabs] = useState<string[]>([]);
  const [activeNested, setActiveNested] = useState<string | null>(null);
  const [nestedFraction, setNestedFraction] = useState(0.45);
  const [openingNested, setOpeningNested] = useState<"edit" | "new" | null>(null);
  const [showDiff, setShowDiff] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [bridgeOpen, setBridgeOpen] = useState(false);
  const [actionBusy, setActionBusy] = useState<string | null>(null);
  const [actionResult, setActionResult] = useState<{
    name: string;
    runId: string | null;
    preview: string;
  } | null>(null);
  const [actionDraft, setActionDraft] = useState<{
    action: AppAction;
    paramsText: string;
  } | null>(null);
  const [inspecting, setInspecting] = useState(false);
  const [pick, setPick] = useState<InspectorPick | null>(null);
  const [pickInstruction, setPickInstruction] = useState("");
  const [lastError, setLastError] = useState<RuntimeErrorPayload | null>(null);
  const [reviseBusy, setReviseBusy] = useState(false);

  const isServerRuntime = manifest?.runtime === "server";
  isServerRuntimeRef.current = isServerRuntime;

  const attachNested = (tid: string) => {
    setNestedTabs((prev) => (prev.includes(tid) ? prev : [...prev, tid]));
    setActiveNested(tid);
  };

  const closeNested = (tid: string) => {
    setNestedTabs((prev) => {
      const idx = prev.indexOf(tid);
      if (idx === -1) return prev;
      const next = prev.filter((t) => t !== tid);
      if (activeNested === tid) {
        setActiveNested(next[idx] ?? next[idx - 1] ?? next[0] ?? null);
      }
      return next;
    });
  };

  const handleEditClick = async () => {
    if (openingNested) return;
    setOpeningNested("edit");
    try {
      const tid = await onResolveExistingThread();
      if (tid) attachNested(tid);
    } finally {
      setOpeningNested(null);
    }
  };

  const handleNewThreadClick = async () => {
    if (openingNested) return;
    setOpeningNested("new");
    try {
      const tid = await onResolveNewThread();
      if (tid) attachNested(tid);
    } finally {
      setOpeningNested(null);
    }
  };

  const toggleInspecting = () => {
    const next = !inspecting;
    setInspecting(next);
    iframeRef.current?.contentWindow?.postMessage(
      { source: "reflex", type: "inspector.toggle", on: next },
      "*",
    );
  };

  const dispatchRevise = async (instruction: string) => {
    if (reviseBusy) return;
    setReviseBusy(true);
    try {
      const tid = await onApplyRevise(instruction);
      if (tid) attachNested(tid);
    } finally {
      setReviseBusy(false);
    }
  };

  const submitInspectorPick = async () => {
    if (!pick) return;
    const text = pickInstruction.trim();
    if (!text) return;
    const summary = `Доработай элемент по селектору \`${pick.selector || pick.tagName}\`.\n\nКонтекст:\n\`\`\`html\n${pick.outerHTML}\n\`\`\`\n\nИЗМЕНЕНИЕ: ${text}`;
    await dispatchRevise(summary);
    setPick(null);
    setPickInstruction("");
  };

  const submitErrorFix = async () => {
    if (!lastError) return;
    const summary = `App упал с ошибкой:\n\nMessage: ${lastError.message}\nLocation: ${lastError.filename}:${lastError.lineno}:${lastError.colno}\nStack:\n\`\`\`\n${lastError.stack || "(no stack)"}\n\`\`\`\n\nПочини этот баг.`;
    await dispatchRevise(summary);
    setLastError(null);
  };

  const onNestedDividerMouseDown = (e: React.MouseEvent<HTMLDivElement>) => {
    e.preventDefault();
    const startX = e.clientX;
    const cw = rootRef.current?.getBoundingClientRect().width ?? 1;
    const startFraction = nestedFraction;
    const onMove = (ev: MouseEvent) => {
      const dx = ev.clientX - startX;
      // dragging right shrinks the nested panel (which is on the right)
      const next = Math.max(0.18, Math.min(0.82, startFraction - dx / cw));
      setNestedFraction(next);
    };
    const onUp = () => {
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
    };
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
  };

  const hasNested = nestedTabs.length > 0;

  useEffect(() => {
    let alive = true;
    invoke<AppManifest>("read_app_manifest", { appId })
      .then((m) => {
        if (alive) setManifest(m);
      })
      .catch((e) => {
        if (alive) setServerError(String(e));
      });
    return () => {
      alive = false;
    };
  }, [appId]);

  // Start/stop server-runtime app while this AppViewer is mounted.
  useEffect(() => {
    if (manifest?.runtime !== "server") return;
    let cancelled = false;
    setServerState("starting");
    setServerError(null);
    invoke<number>("app_server_start", { appId })
      .then(async (port) => {
        if (cancelled) {
          void invoke("app_server_stop", { appId }).catch(() => {});
          return;
        }
        setServerPort(port);
        setServerState("running");
        // catchup logs from buffer
        try {
          const snap = await invoke<{ lines: ServerLogLine[] }>(
            "app_server_logs",
            { appId },
          );
          setLogs(snap.lines);
        } catch {}
      })
      .catch((e) => {
        if (cancelled) return;
        setServerError(String(e));
        setServerState("failed");
      });
    return () => {
      cancelled = true;
      void invoke("app_server_stop", { appId }).catch(() => {});
    };
  }, [appId, manifest?.runtime]);

  // Proxy agent.stream events into iframe via postMessage.
  useEffect(() => {
    const sendToIframe = (data: any) => {
      iframeRef.current?.contentWindow?.postMessage(data, "*");
    };
    const tokenUn = listen<{
      stream_id: string;
      app_id: string;
      token: string;
    }>("reflex://app-stream-token", (e) => {
      if (e.payload.app_id !== appId) return;
      sendToIframe({
        source: "reflex",
        type: "stream.token",
        streamId: e.payload.stream_id,
        token: e.payload.token,
      });
    });
    const doneUn = listen<{
      stream_id: string;
      app_id: string;
      result: string | null;
    }>("reflex://app-stream-done", (e) => {
      if (e.payload.app_id !== appId) return;
      sendToIframe({
        source: "reflex",
        type: "stream.done",
        streamId: e.payload.stream_id,
        result: e.payload.result,
      });
    });
    const eventUn = listen<{
      topic: string;
      from_app: string;
      data: unknown;
    }>(`reflex://app-event/${appId}`, (e) => {
      sendToIframe({
        source: "reflex",
        type: "event",
        topic: e.payload.topic,
        fromApp: e.payload.from_app,
        data: e.payload.data,
      });
    });
    return () => {
      tokenUn.then((u) => u());
      doneUn.then((u) => u());
      eventUn.then((u) => u());
      void invoke("app_invoke", {
        appId,
        method: "events.clearSubscriptions",
        params: {},
      }).catch(() => {});
    };
  }, [appId]);

  // Stream server logs (live).
  useEffect(() => {
    if (manifest?.runtime !== "server") return;
    const unlisten = listen<{
      app_id: string;
      stream: ServerLogLine["stream"];
      seq: number;
      line: string;
      ts_ms: number;
    }>("reflex://app-server-log", (e) => {
      if (e.payload.app_id !== appId) return;
      setLogs((prev) => {
        const next = [...prev, e.payload];
        if (next.length > 500) next.splice(0, next.length - 500);
        return next;
      });
    });
    return () => {
      unlisten.then((u) => u());
    };
  }, [appId, manifest?.runtime]);

  // Healthcheck: poll server status; flip to "crashed" if process died.
  useEffect(() => {
    if (manifest?.runtime !== "server") return;
    let alive = true;
    const tick = async () => {
      try {
        const s = await invoke<{
          running: boolean;
          port: number | null;
          exit_code: number | null;
        }>("app_server_status", { appId });
        if (!alive) return;
        if (!s.running && (serverState === "running" || serverState === "starting")) {
          setServerState("crashed");
          setServerError(
            s.exit_code != null ? `exit code ${s.exit_code}` : "process exited",
          );
        } else if (s.running && serverState === "crashed") {
          setServerState("running");
          setServerError(null);
        }
      } catch {}
    };
    const timer = setInterval(tick, 3000);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, [appId, manifest?.runtime, serverState]);

  async function restartServer() {
    if (busy) return;
    setBusy("restart");
    setServerError(null);
    try {
      const port = await invoke<number>("app_server_restart", { appId });
      setServerPort(port);
      setServerState("running");
      setReloadKey((k) => k + 1);
    } catch (e) {
      setServerError(String(e));
      setServerState("failed");
    } finally {
      setBusy(null);
    }
  }

  async function executeManifestAction(action: AppAction, actionParams: unknown) {
    if (actionBusy) return;
    setActionBusy(action.id);
    setActionResult(null);
    setError(null);
    try {
      const result = await invoke<any>("app_invoke", {
        appId,
        method: "apps.invoke",
        params: {
          app_id: appId,
          action_id: action.id,
          params: actionParams,
        },
      });
      setActionResult({
        name: action.name || action.id,
        runId: result?.run_id ?? result?.runId ?? null,
        preview: previewJsonValue(result?.result ?? result),
      });
    } catch (e) {
      setError(String(e));
    } finally {
      setActionBusy(null);
    }
  }

  function runManifestAction(action: AppAction) {
    if (actionBusy) return;
    if (actionParamsSchema(action)) {
      setActionDraft({
        action,
        paramsText: defaultActionParamsJson(action),
      });
      setActionResult(null);
      setError(null);
      return;
    }
    void executeManifestAction(action, {});
  }

  function submitActionDraft() {
    if (!actionDraft || actionBusy) return;
    let params: unknown;
    try {
      params = JSON.parse(actionDraft.paramsText.trim() || "{}");
    } catch (e) {
      setError(`Invalid action params JSON: ${String(e)}`);
      return;
    }
    const action = actionDraft.action;
    setActionDraft(null);
    void executeManifestAction(action, params);
  }

  async function refreshStatus() {
    try {
      const s = await invoke<AppStatus>("app_status", { appId });
      setStatus(s);
      // detect false → true transition (codex finished writing changes)
      if (s.has_changes && !prevHasChangesRef.current) {
        if (isServerRuntimeRef.current) {
          // server runtime — restart the process so the new code is picked up
          void restartServer();
        } else {
          setReloadKey((k) => k + 1);
        }
      }
      prevHasChangesRef.current = s.has_changes;
      return s;
    } catch (e) {
      console.error("[reflex] app_status", e);
      return null;
    }
  }

  useEffect(() => {
    let alive = true;
    refreshStatus();
    // Lower-frequency status polling for git revision/last commit msg.
    // Critical-path reloads come via the file watcher below.
    const timer = setInterval(() => {
      if (alive) void refreshStatus();
    }, 5000);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, [appId]);

  // File watcher: reload iframe (static) or restart server when files change.
  useEffect(() => {
    let alive = true;
    void invoke("app_watch_start", { appId }).catch((e) =>
      console.error("[reflex] app_watch_start", e),
    );
    const unlisten = listen<{ app_id: string; paths: string[] }>(
      "reflex://app-files-changed",
      (e) => {
        if (!alive) return;
        if (e.payload.app_id !== appId) return;
        if (isServerRuntimeRef.current) {
          void restartServer();
        } else {
          setReloadKey((k) => k + 1);
        }
        // refresh status immediately so has_changes/revision update
        void refreshStatus();
      },
    );
    return () => {
      alive = false;
      void invoke("app_watch_stop", { appId }).catch(() => {});
      unlisten.then((u) => u());
    };
  }, [appId]);

  useEffect(() => {
    const onMessage = async (ev: MessageEvent) => {
      const msg = ev.data;
      if (!msg || msg.source !== "reflex-app") return;
      if (msg.type === "inspector.pick") {
        setPick(msg.payload as InspectorPick);
        setPickInstruction("");
        setInspecting(false);
        return;
      }
      if (msg.type === "runtime.error") {
        const payload = msg.payload as RuntimeErrorPayload;
        // dedupe: don't replace if same message
        setLastError((prev) =>
          prev && prev.message === payload.message && prev.stack === payload.stack
            ? prev
            : payload,
        );
        return;
      }
      if (msg.type === "request") {
        const { id, method, params } = msg as {
          id: number;
          method: string;
          params: any;
        };
        try {
          const result = await invoke("app_invoke", {
            appId,
            method,
            params: params ?? {},
          });
          iframeRef.current?.contentWindow?.postMessage(
            { source: "reflex", type: "response", id, result },
            "*",
          );
        } catch (e) {
          iframeRef.current?.contentWindow?.postMessage(
            { source: "reflex", type: "response", id, error: String(e) },
            "*",
          );
        }
      }
    };
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, [appId]);

  async function exportApp() {
    if (exporting) return;
    setExporting(true);
    setError(null);
    try {
      const target = await invoke<string | null>("pick_save_file", {
        title: "Сохранить .reflexapp",
        defaultName: `${appId}.reflexapp`,
        filterName: "Reflex App",
        filterExtensions: ["reflexapp", "zip"],
      });
      if (!target) return;
      await invoke("app_export", { appId, targetPath: target });
    } catch (e) {
      setError(String(e));
    } finally {
      setExporting(false);
    }
  }

  async function save() {
    if (busy) return;
    setBusy("save");
    setError(null);
    try {
      const msg = window.prompt("Сообщение для commit:", "revision") ?? "revision";
      await invoke("app_save", { appId, message: msg });
      await refreshStatus();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  }

  async function revert() {
    if (busy) return;
    if (!window.confirm("Откатиться к предыдущей версии? Несохранённые изменения будут потеряны.")) return;
    setBusy("revert");
    setError(null);
    try {
      await invoke("app_revert", { appId });
      await refreshStatus();
      setReloadKey((k) => k + 1);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  }

  const entry = manifest?.entry ?? "index.html";
  const src = isServerRuntime
    ? serverPort
      ? `reflexserver://${encodeURIComponent(appId)}/`
      : null
    : `reflexapp://localhost/${encodeURIComponent(appId)}/${entry}`;
  const sandbox = isServerRuntime
    ? "allow-scripts allow-forms allow-same-origin"
    : "allow-scripts allow-forms";
  const manifestFacts = useMemo(
    () => buildAppCapabilityFacts(manifest, serverPort),
    [manifest, serverPort],
  );

  return (
    <div
      ref={rootRef}
      className={`appviewer-root ${hasNested ? "appviewer-with-nested" : ""}`}
    >
      <div
        className="appviewer-main"
        style={hasNested ? { flexBasis: `${(1 - nestedFraction) * 100}%` } : undefined}
      >
      <header className="appviewer-header">
        <div className="appviewer-title">
          {appId}
          {status && (
            <span className="appviewer-rev">rev {status.revision}</span>
          )}
        </div>
        <div className="appviewer-actions">
          {isServerRuntime && (
            <button
              className="appviewer-btn"
              onClick={() => void restartServer()}
              disabled={busy !== null}
              title="Перезапустить сервер"
            >
              {busy === "restart" ? "…" : "↻ Restart"}
            </button>
          )}
          {isServerRuntime && (
            <button
              className="appviewer-btn"
              onClick={() => setLogsOpen((v) => !v)}
              title="Показать логи сервера"
            >
              {logsOpen ? "▾ Logs" : "▸ Logs"}
            </button>
          )}
          <button
            className={`appviewer-btn ${bridgeOpen ? "appviewer-btn-primary" : ""}`}
            onClick={() => setBridgeOpen((v) => !v)}
            title="Показать runtime overlay helpers"
          >
            {bridgeOpen ? "▾ Bridge" : "▸ Bridge"}
          </button>
          {!isServerRuntime && (
            <button
              className={`appviewer-btn ${inspecting ? "appviewer-btn-primary" : ""}`}
              onClick={toggleInspecting}
              disabled={busy !== null || reviseBusy}
              title="Кликни по элементу в app и опиши что изменить"
            >
              {inspecting ? "✕ Inspect" : "🎯 Inspect"}
            </button>
          )}
          <button
            className="appviewer-btn"
            onClick={() => void handleEditClick()}
            disabled={busy !== null || openingNested !== null}
            title="Открыть существующий тред для доработки (привяжется к этому app)"
          >
            {openingNested === "edit" ? "…" : "✏️ Edit"}
          </button>
          <button
            className="appviewer-btn"
            onClick={() => void handleNewThreadClick()}
            disabled={busy !== null || openingNested !== null}
            title="Создать новый тред для изолированных изменений"
          >
            {openingNested === "new" ? "…" : "🆕 New thread"}
          </button>
          <button
            className="appviewer-btn"
            onClick={() => void exportApp()}
            disabled={busy !== null || exporting}
            title="Экспортировать app в .reflexapp файл"
          >
            {exporting ? "…" : "📤 Export"}
          </button>
        </div>
      </header>

      {manifestFacts.length > 0 && (
        <div className="appviewer-capabilities" aria-label="Manifest capabilities">
          {manifestFacts.map((fact) => (
            <div
              key={fact.key}
              className="appviewer-capability"
              title={fact.title}
            >
              <span className="appviewer-capability-label">{fact.label}</span>
              <span className="appviewer-capability-value">{fact.value}</span>
            </div>
          ))}
        </div>
      )}

      {bridgeOpen && (
        <div className="appviewer-bridge-panel" aria-label="Runtime bridge helpers">
          {APP_BRIDGE_HELPER_GROUPS.map((group) => (
            <div className="appviewer-bridge-group" key={group.title}>
              <div className="appviewer-bridge-title">{group.title}</div>
              <div className="appviewer-bridge-list">
                {group.helpers.map((helper) => (
                  <code key={helper}>{helper}</code>
                ))}
              </div>
            </div>
          ))}
        </div>
      )}

      {(manifest?.actions?.length ?? 0) > 0 && (
        <div className="appviewer-action-strip" aria-label="Manifest actions">
          <div className="appviewer-action-strip-title">Actions</div>
          <div className="appviewer-action-buttons">
            {(manifest?.actions ?? []).map((action) => (
              <button
                key={action.id}
                className="appviewer-action-run"
                onClick={() => void runManifestAction(action)}
                disabled={actionBusy !== null}
                title={action.description ?? action.id}
              >
                {actionBusy === action.id ? "…" : action.name || action.id}
                {action.public && (
                  <span className="appviewer-action-public">public</span>
                )}
                {!!actionParamsSchema(action) && (
                  <span className="appviewer-action-public">params</span>
                )}
              </button>
            ))}
          </div>
          {actionResult && (
            <div className="appviewer-action-result" title={actionResult.runId ?? undefined}>
              <span>{actionResult.name}</span>
              <code>{actionResult.preview}</code>
            </div>
          )}
        </div>
      )}

      {actionDraft && (
        <div className="appviewer-action-editor" aria-label="Action params editor">
          <div className="appviewer-action-editor-head">
            <span>{actionDraft.action.name || actionDraft.action.id}</span>
            <code>{actionDraft.action.id}</code>
          </div>
          <textarea
            className="appviewer-action-editor-input"
            rows={5}
            value={actionDraft.paramsText}
            onChange={(e) =>
              setActionDraft((draft) =>
                draft
                  ? { ...draft, paramsText: e.currentTarget.value }
                  : draft,
              )
            }
            onKeyDown={(e) => {
              if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                e.preventDefault();
                submitActionDraft();
              }
            }}
          />
          <div className="appviewer-action-editor-actions">
            <button
              className="appviewer-btn"
              onClick={() => setActionDraft(null)}
              disabled={actionBusy !== null}
            >
              Cancel
            </button>
            <button
              className="appviewer-btn appviewer-btn-primary"
              onClick={submitActionDraft}
              disabled={actionBusy !== null}
            >
              Run
            </button>
          </div>
        </div>
      )}

      {status?.has_changes && (
        <div className="appviewer-banner appviewer-banner-warn">
          <span>Есть несохранённые изменения.</span>
          <div className="appviewer-banner-actions">
            <button
              className="appviewer-btn"
              onClick={() => setShowDiff(true)}
              disabled={busy !== null}
              title="Посмотреть diff и применить выборочно"
            >
              🔍 Diff
            </button>
            <button
              className="appviewer-btn appviewer-btn-primary"
              onClick={() => void save()}
              disabled={busy !== null}
            >
              Save
            </button>
            <button
              className="appviewer-btn appviewer-btn-danger"
              onClick={() => void revert()}
              disabled={busy !== null}
            >
              Revert
            </button>
            <button
              className="appviewer-btn"
              onClick={() => setReloadKey((k) => k + 1)}
              disabled={busy !== null}
            >
              Reload
            </button>
          </div>
        </div>
      )}

      {showDiff && (
        <DiffPanel
          appId={appId}
          onClose={() => setShowDiff(false)}
          onApplied={() => {
            setShowDiff(false);
            void refreshStatus();
          }}
        />
      )}

      {lastError && (
        <div className="appviewer-banner appviewer-banner-warn">
          <div className="appviewer-error-summary">
            <strong>App упал:</strong> {lastError.message}
            {lastError.filename && (
              <span className="appviewer-error-loc">
                {" · "}
                {lastError.filename.split("/").pop()}:{lastError.lineno}
              </span>
            )}
          </div>
          <div className="appviewer-banner-actions">
            <button
              className="appviewer-btn appviewer-btn-primary"
              onClick={() => void submitErrorFix()}
              disabled={reviseBusy}
              title="Отправить ошибку codex'у с просьбой починить"
            >
              {reviseBusy ? "…" : "✨ Fix"}
            </button>
            <button
              className="appviewer-btn"
              onClick={() => setLastError(null)}
              disabled={reviseBusy}
            >
              Dismiss
            </button>
          </div>
        </div>
      )}

      {pick && (
        <div className="inspector-card">
          <header className="inspector-card-header">
            <span className="inspector-card-tag">🎯 selected</span>
            <code className="inspector-card-selector">
              {pick.selector || pick.tagName}
            </code>
            <button
              className="inspector-card-close"
              onClick={() => setPick(null)}
              aria-label="Закрыть"
            >
              ×
            </button>
          </header>
          {pick.text && (
            <div className="inspector-card-preview">
              "{pick.text.slice(0, 80)}
              {pick.text.length > 80 ? "…" : ""}"
            </div>
          )}
          <textarea
            className="inspector-card-input"
            placeholder="Что изменить в этом элементе? (Cmd+Enter)"
            autoFocus
            rows={3}
            value={pickInstruction}
            onChange={(e) => setPickInstruction(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                e.preventDefault();
                void submitInspectorPick();
              }
            }}
          />
          <div className="inspector-card-actions">
            <button
              className="appviewer-btn appviewer-btn-primary"
              onClick={() => void submitInspectorPick()}
              disabled={reviseBusy || !pickInstruction.trim()}
            >
              {reviseBusy ? "Применяю…" : "Apply (⌘↵)"}
            </button>
          </div>
        </div>
      )}

      {error && <div className="apps-error">{error}</div>}

      {isServerRuntime && serverState !== "running" && (
        <div
          className={`appviewer-banner ${serverState === "failed" || serverState === "crashed" ? "appviewer-banner-warn" : "appviewer-banner-info"}`}
        >
          {serverState === "starting" && (
            <span>Запускаю локальный сервер app…</span>
          )}
          {serverState === "failed" && (
            <span>Сервер не стартовал: {serverError}</span>
          )}
          {serverState === "crashed" && (
            <span>Сервер упал: {serverError ?? "process exited"}</span>
          )}
          {(serverState === "failed" || serverState === "crashed") && (
            <div className="appviewer-banner-actions">
              <button
                className="appviewer-btn"
                onClick={() => void restartServer()}
                disabled={busy !== null}
              >
                Restart
              </button>
            </div>
          )}
        </div>
      )}

      {!isServerRuntime && status && !status.entry_exists ? (
        <div className="appviewer-stuck">
          <h3>Генерация не завершена</h3>
          <p>
            Codex ещё не записал <code>{entry}</code>. Возможно процесс прерван
            или сейчас в плане ждёт подтверждения.
          </p>
          <div className="appviewer-stuck-actions">
            <button
              className="appviewer-btn"
              onClick={() => setReloadKey((n) => n + 1)}
            >
              Проверить ещё раз
            </button>
            <button
              className="appviewer-btn appviewer-btn-danger"
              disabled={busy !== null}
              onClick={async () => {
                if (
                  !window.confirm(
                    `Переместить "${manifest?.name ?? appId}" в корзину?`,
                  )
                )
                  return;
                try {
                  await invoke("delete_app", { appId });
                  onDeleted?.();
                } catch (e) {
                  setError(String(e));
                }
              }}
            >
              В корзину
            </button>
          </div>
        </div>
      ) : src ? (
        <iframe
          ref={iframeRef}
          key={`${reloadKey}:${serverPort ?? "static"}`}
          className="app-iframe"
          src={src}
          sandbox={sandbox}
          title={appId}
        />
      ) : (
        <div className="app-iframe-placeholder" />
      )}

      {isServerRuntime && logsOpen && (
        <div className="server-logs">
          <div className="server-logs-header">
            <span>
              server logs
              {serverPort != null && (
                <span className="server-logs-port"> · :{serverPort}</span>
              )}
            </span>
            <button
              className="server-logs-clear"
              onClick={() => setLogs([])}
              title="Очистить локальный буфер логов"
            >
              clear
            </button>
          </div>
          <div className="server-logs-body">
            {logs.length === 0 ? (
              <div className="server-logs-empty">пусто</div>
            ) : (
              logs.map((l) => (
                <div
                  key={`${l.stream}-${l.seq}-${l.ts_ms}`}
                  className={`server-log-line server-log-${l.stream}`}
                >
                  {l.line}
                </div>
              ))
            )}
          </div>
        </div>
      )}
      </div>

      {hasNested && (
        <>
          <div
            className="appviewer-nested-divider"
            onMouseDown={onNestedDividerMouseDown}
          />
          <div
            className="appviewer-nested"
            style={{ flexBasis: `${nestedFraction * 100}%` }}
          >
            <nav className="nested-tabs">
              {nestedTabs.map((tid) => {
                const t = threads.find((x) => x.id === tid);
                const label =
                  t?.title ?? t?.prompt?.slice(0, 32) ?? tid;
                const active = activeNested === tid;
                return (
                  <div
                    key={tid}
                    className={`nested-tab ${active ? "active" : ""}`}
                    onClick={() => setActiveNested(tid)}
                    title={label}
                  >
                    <span className="nested-tab-label">💬 {label}</span>
                    <button
                      className="nested-tab-close"
                      onClick={(e) => {
                        e.stopPropagation();
                        closeNested(tid);
                      }}
                      aria-label="Закрыть"
                    >
                      ×
                    </button>
                  </div>
                );
              })}
            </nav>
            <div className="nested-body">
              {activeNested ? (
                <TopicScreen
                  thread_id={activeNested}
                  threads={threads}
                  projects={[]}
                />
              ) : (
                <div className="chat-empty">
                  <p>Выбери тред слева.</p>
                </div>
              )}
            </div>
          </div>
        </>
      )}
    </div>
  );
}

function HomeScreen({
  projects,
  threads,
  openAppIds,
  onSelectProject,
  onSelectTopic,
  onSelectApp,
  onOpenApps,
  onCreateProject,
}: {
  projects: Project[];
  threads: Thread[];
  openAppIds: Set<string>;
  onSelectProject: (id: string) => void;
  onSelectTopic: (id: string) => void;
  onSelectApp: (id: string) => void;
  onOpenApps: () => void;
  onCreateProject: () => void;
}) {
  const recent = threads
    .slice()
    .sort((a, b) => b.created_at_ms - a.created_at_ms)
    .slice(0, 5);

  return (
    <div className="home-root">
      <HomeAppsSection
        openAppIds={openAppIds}
        onSelectApp={onSelectApp}
        onOpenApps={onOpenApps}
      />
      <section>
        <div className="section-head">
          <h2 className="section-title">Projects</h2>
          <button className="apps-create-btn" onClick={onCreateProject}>
            + New Project
          </button>
        </div>
        {projects.length === 0 ? (
          <div className="home-empty-panel">
            <p>
              Создай первый проект кнопкой выше или открой Quick-панель (
              <kbd>⌘⇧Space</kbd>) поверх любой папки.
            </p>
          </div>
        ) : (
          <div className="project-grid">
            {projects.map((p) => {
              const projectThreads = threads.filter(
                (t) => t.project_id === p.id,
              );
              const count = projectThreads.length;
              const running = projectThreads.filter((t) => !t.done).length;
              return (
                <button
                  key={p.id}
                  className="project-card"
                  onClick={() => onSelectProject(p.id)}
                >
                  <div className="project-card-icon">📁</div>
                  <div className="project-card-name">
                    {p.name}
                    {running > 0 && (
                      <span className="project-card-running">
                        <span className="status-dot status-dot-running" />
                        {running}
                      </span>
                    )}
                  </div>
                  <div className="project-card-path" title={p.root}>
                    {p.root}
                  </div>
                  <div className="project-card-meta">
                    {count} topic{count === 1 ? "" : "s"}
                  </div>
                </button>
              );
            })}
          </div>
        )}
      </section>
      {recent.length > 0 && (
        <section>
          <h2 className="section-title">Recent</h2>
          <ul className="topic-list">
            {recent.map((t) => (
              <li key={t.id}>
                <button
                  className="topic-row topic-row-with-status"
                  onClick={() => onSelectTopic(t.id)}
                >
                  <StatusDot done={t.done} ok={t.exit_code === 0} />
                  <div className="topic-row-body">
                    <span className="topic-row-prompt">
                      {t.title ?? t.prompt}
                    </span>
                    <span className="topic-row-meta">
                      📁 {t.project_name} ·{" "}
                      {new Date(t.created_at_ms).toLocaleString()}
                    </span>
                  </div>
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}

function HomeAppsSection({
  openAppIds,
  onSelectApp,
  onOpenApps,
}: {
  openAppIds: Set<string>;
  onSelectApp: (id: string) => void;
  onOpenApps: () => void;
}) {
  const [items, setItems] = useState<AppManifest[]>([]);
  const [statuses, setStatuses] = useState<Record<string, AppServerStatus>>({});
  const [error, setError] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    let alive = true;

    const tick = async () => {
      try {
        const list = await invoke<AppManifest[]>("list_apps");
        const serverEntries = await Promise.all(
          list
            .filter((app) => app.ready !== false && app.runtime === "server")
            .map(async (app) => {
              try {
                const status = await invoke<AppServerStatus>(
                  "app_server_status",
                  { appId: app.id },
                );
                return [app.id, status] as const;
              } catch {
                return [
                  app.id,
                  { running: false, port: null, exit_code: null },
                ] as const;
              }
            }),
        );
        if (!alive) return;
        setItems(list);
        setStatuses(Object.fromEntries(serverEntries));
        setError(null);
        setLoaded(true);
      } catch (e) {
        if (alive) {
          setError(String(e));
          setLoaded(true);
        }
      }
    };

    void tick();
    const timer = setInterval(() => void tick(), 3000);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, []);

  const readyCount = items.filter((app) => app.ready !== false).length;
  const runningCount = items.filter((app) =>
    isHomeAppRunning(app, statuses, openAppIds),
  ).length;

  return (
    <section>
      <div className="section-head">
        <h2 className="section-title">
          Apps
          {runningCount > 0 && (
            <span className="section-badge running">
              {runningCount} running
            </span>
          )}
        </h2>
        <button className="apps-create-btn" onClick={onOpenApps}>
          Manage Apps
        </button>
      </div>
      {error && <div className="apps-error">{error}</div>}
      {!loaded ? (
        <div className="home-empty-panel">
          <p>Загружаю apps…</p>
        </div>
      ) : items.length === 0 ? (
        <div className="home-empty-panel">
          <p>Apps пока нет.</p>
          <button className="home-inline-action" onClick={onOpenApps}>
            Open Apps
          </button>
        </div>
      ) : (
        <div className="home-apps-grid">
          {items.map((app) => {
            const isReady = app.ready !== false;
            const isRunning = isHomeAppRunning(app, statuses, openAppIds);
            const statusLabel = !isReady
              ? "создаётся"
              : isRunning
                ? "запущено"
                : "не запущено";
            return (
              <button
                key={app.id}
                className={`home-app-card ${isReady ? "" : "home-app-card-disabled"}`}
                disabled={!isReady}
                onClick={() => isReady && onSelectApp(app.id)}
                title={isReady ? "Открыть app" : "Codex ещё пишет файлы…"}
              >
                <div className="home-app-card-top">
                  <span className="home-app-icon">{app.icon ?? "🧩"}</span>
                  <span
                    className={`home-app-status-dot ${isRunning ? "running" : ""}`}
                    aria-label={statusLabel}
                  />
                </div>
                <div className="home-app-name">{app.name}</div>
                {app.description && (
                  <div className="home-app-desc">{app.description}</div>
                )}
                <div className="home-app-meta">
                  <span>{statusLabel}</span>
                  {app.runtime === "server" && statuses[app.id]?.port && (
                    <span>:{statuses[app.id].port}</span>
                  )}
                </div>
              </button>
            );
          })}
        </div>
      )}
      {items.length > 0 && (
        <div className="home-apps-summary">
          {readyCount} ready · {runningCount} running
        </div>
      )}
    </section>
  );
}

function isHomeAppRunning(
  app: AppManifest,
  statuses: Record<string, AppServerStatus>,
  openAppIds: Set<string>,
) {
  if (app.ready === false) return false;
  if (app.runtime === "server") return statuses[app.id]?.running ?? false;
  return openAppIds.has(app.id);
}

type DirEntry = {
  name: string;
  path: string;
  kind: "file" | "directory" | "symlink";
  size: number | null;
  modified_ms: number | null;
  is_hidden: boolean;
};

function ProjectScreen({
  projectId,
  projects,
  threads,
  onSelectTopic,
  onProjectUpdated,
  onCreateTopic,
  onOpenApp,
}: {
  projectId: string;
  projects: Project[];
  threads: Thread[];
  onSelectTopic: (id: string) => void;
  onProjectUpdated: (p: Project) => void;
  onCreateTopic: (prompt: string, planMode: boolean) => Promise<void>;
  onOpenApp: (id: string) => void;
}) {
  const project = projects.find((p) => p.id === projectId);
  const [entries, setEntries] = useState<DirEntry[]>([]);
  const [showHidden, setShowHidden] = useState(false);
  const [showNewTopic, setShowNewTopic] = useState(false);
  const [newTopicPrompt, setNewTopicPrompt] = useState("");
  const [newTopicPlanMode, setNewTopicPlanMode] = useState(false);
  const [creatingTopic, setCreatingTopic] = useState(false);
  const [drawerTarget, setDrawerTarget] = useState<DrawerTarget | null>(null);
  const [statuses, setStatuses] = useState<Record<string, PathStatus>>({});
  const [statusTick, setStatusTick] = useState(0);
  const [topicError, setTopicError] = useState<string | null>(null);
  const [editDescription, setEditDescription] = useState(false);
  const [descriptionDraft, setDescriptionDraft] = useState("");
  const [installedApps, setInstalledApps] = useState<AppManifest[]>([]);
  const [showLinkPicker, setShowLinkPicker] = useState(false);
  const [profileEditing, setProfileEditing] = useState(false);
  const [profileInstructionsDraft, setProfileInstructionsDraft] = useState("");
  const [profileSkillsDraft, setProfileSkillsDraft] = useState("");
  const [profileSaving, setProfileSaving] = useState(false);
  const [profileError, setProfileError] = useState<string | null>(null);
  const [mcpEditing, setMcpEditing] = useState(false);
  const [mcpDraft, setMcpDraft] = useState("{}");
  const [mcpSaving, setMcpSaving] = useState(false);
  const [mcpError, setMcpError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    invoke<AppManifest[]>("list_apps")
      .then((list) => alive && setInstalledApps(list))
      .catch((e) => console.error("[reflex] list_apps failed", e));
    const u = listen("reflex://apps-changed", () => {
      invoke<AppManifest[]>("list_apps")
        .then((list) => alive && setInstalledApps(list))
        .catch(() => {});
    });
    return () => {
      alive = false;
      u.then((un) => un());
    };
  }, []);

  async function saveDescription(value: string) {
    if (!project) return;
    try {
      const updated = await invoke<Project>("update_project_description", {
        projectId: project.id,
        description: value.trim() || null,
      });
      onProjectUpdated(updated);
    } catch (e) {
      console.error("[reflex] update_project_description failed", e);
    }
  }

  async function linkApp(appId: string) {
    if (!project) return;
    try {
      const updated = await invoke<Project>("link_app_to_project", {
        projectId: project.id,
        appId,
      });
      onProjectUpdated(updated);
      setShowLinkPicker(false);
    } catch (e) {
      console.error("[reflex] link_app_to_project failed", e);
    }
  }

  async function unlinkApp(appId: string) {
    if (!project) return;
    try {
      const updated = await invoke<Project>("unlink_app_from_project", {
        projectId: project.id,
        appId,
      });
      onProjectUpdated(updated);
    } catch (e) {
      console.error("[reflex] unlink_app_from_project failed", e);
    }
  }

  async function submitNewTopic() {
    const text = newTopicPrompt.trim();
    if (!text || creatingTopic) return;
    setCreatingTopic(true);
    setTopicError(null);
    try {
      await onCreateTopic(text, newTopicPlanMode);
      setShowNewTopic(false);
      setNewTopicPrompt("");
    } catch (e) {
      setTopicError(String(e));
    } finally {
      setCreatingTopic(false);
    }
  }

  const topics = useMemo(
    () =>
      threads
        .filter((t) => t.project_id === projectId)
        .sort((a, b) => b.created_at_ms - a.created_at_ms),
    [threads, projectId],
  );

  const [entriesTick, setEntriesTick] = useState(0);

  useEffect(() => {
    if (!project) return;
    let alive = true;
    invoke<DirEntry[]>("list_directory", { path: project.root })
      .then((list) => {
        if (alive) setEntries(list);
      })
      .catch((e) => console.error("[reflex] list_directory failed", e));
    return () => {
      alive = false;
    };
  }, [project?.root, entriesTick]);

  useEffect(() => {
    if (!projectId) return;
    let unlisten: (() => void) | null = null;
    invoke("project_watch_start", { projectId }).catch((e) =>
      console.error("[reflex] project_watch_start", e),
    );
    listen<{ project_id: string }>("reflex://project-files-changed", (ev) => {
      if (ev.payload?.project_id !== projectId) return;
      setEntriesTick((n) => n + 1);
      setStatusTick((n) => n + 1);
    })
      .then((u) => {
        unlisten = u;
      })
      .catch((e) => console.error("[reflex] listen project-files-changed", e));
    return () => {
      unlisten?.();
      invoke("project_watch_stop", { projectId }).catch(() => {});
    };
  }, [projectId]);

  const visibleEntries = entries.filter(
    (e) => (showHidden || !e.is_hidden) && e.name !== ".reflex",
  );
  const runningCount = topics.filter((t) => !t.done).length;

  useEffect(() => {
    if (!project || visibleEntries.length === 0) {
      setStatuses({});
      return;
    }
    let alive = true;
    const paths = visibleEntries.map((e) => e.path);
    invoke<PathStatus[]>("memory_path_status_batch", {
      projectRoot: project.root,
      paths,
    })
      .then((arr) => {
        if (!alive) return;
        const map: Record<string, PathStatus> = {};
        arr.forEach((s) => {
          map[s.path] = s;
        });
        setStatuses(map);
      })
      .catch((e) => console.error("[reflex] memory_path_status_batch", e));
    return () => {
      alive = false;
    };
  }, [project?.root, visibleEntries.length, statusTick]);

  function openExternal(path: string) {
    invoke("reveal_in_finder", { path }).catch((e) =>
      console.error("[reflex] reveal_in_finder", e),
    );
  }

  const sandbox = project?.sandbox ?? "workspace-write";
  const browserOn = !!(
    project?.mcp_servers?.reflex_browser || project?.mcp_servers?.playwright
  );

  async function setSandbox(value: string) {
    if (!project) return;
    try {
      const updated = await invoke<Project>("update_project_sandbox", {
        projectId: project.id,
        sandbox: value,
      });
      onProjectUpdated(updated);
    } catch (e) {
      console.error("[reflex] update_project_sandbox failed", e);
    }
  }

  async function setBrowser(enabled: boolean) {
    if (!project) return;
    try {
      const updated = await invoke<Project>("update_project_browser", {
        projectId: project.id,
        enabled,
      });
      onProjectUpdated(updated);
    } catch (e) {
      console.error("[reflex] update_project_browser failed", e);
    }
  }

  function openAgentProfileEditor() {
    setProfileInstructionsDraft(project?.agent_instructions ?? "");
    setProfileSkillsDraft((project?.skills ?? []).join("\n"));
    setProfileError(null);
    setProfileEditing(true);
  }

  async function saveAgentProfile() {
    if (!project || profileSaving) return;
    const seen = new Set<string>();
    const skills = profileSkillsDraft
      .split(/[\n,]/)
      .map((s) => s.trim())
      .filter((s) => s.length > 0)
      .filter((s) => {
        const key = s.toLowerCase();
        if (seen.has(key)) return false;
        seen.add(key);
        return true;
      });
    setProfileSaving(true);
    setProfileError(null);
    try {
      const updated = await invoke<Project>("update_project_agent_profile", {
        projectId: project.id,
        agentInstructions: profileInstructionsDraft.trim() || null,
        skills,
      });
      onProjectUpdated(updated);
      setProfileEditing(false);
    } catch (e) {
      setProfileError(String(e));
    } finally {
      setProfileSaving(false);
    }
  }

  function openMcpEditor() {
    setMcpDraft(JSON.stringify(project?.mcp_servers ?? {}, null, 2));
    setMcpError(null);
    setMcpEditing(true);
  }

  async function saveMcpServers() {
    if (!project || mcpSaving) return;
    let parsed: unknown;
    try {
      parsed = JSON.parse(mcpDraft.trim() || "{}");
    } catch (e) {
      setMcpError(`JSON parse error: ${String(e)}`);
      return;
    }
    if (
      parsed !== null &&
      (typeof parsed !== "object" || Array.isArray(parsed))
    ) {
      setMcpError("MCP config must be a JSON object.");
      return;
    }
    setMcpSaving(true);
    setMcpError(null);
    try {
      const updated = await invoke<Project>("update_project_mcp_servers", {
        projectId: project.id,
        mcpServers: parsed,
      });
      onProjectUpdated(updated);
      setMcpEditing(false);
    } catch (e) {
      setMcpError(String(e));
    } finally {
      setMcpSaving(false);
    }
  }

  const mcpServerNames = Object.keys(project?.mcp_servers ?? {});
  const projectSkills = project?.skills ?? [];
  const linkedAppIds = project?.apps ?? [];
  const hasAgentProfile = !!(
    project?.agent_instructions?.trim() || projectSkills.length > 0
  );
  const profileSkillDraftSet = useMemo(() => {
    return new Set(
      profileSkillsDraft
        .split(/[\n,]/)
        .map((s) => s.trim().toLowerCase())
        .filter(Boolean),
    );
  }, [profileSkillsDraft]);

  function appendProfileSkill(skill: string) {
    setProfileSkillsDraft((prev) => {
      const seen = new Set(
        prev
          .split(/[\n,]/)
          .map((s) => s.trim().toLowerCase())
          .filter(Boolean),
      );
      if (seen.has(skill.toLowerCase())) return prev;
      return prev.trim() ? `${prev.trimEnd()}\n${skill}` : skill;
    });
  }

  return (
    <div className="project-root">
      <header className="project-header">
        <h1 className="project-title">📁 {project?.name ?? projectId}</h1>
        {project && (
          <div className="project-path">
            <button
              className="project-path-link"
              onClick={() => project && openExternal(project.root)}
              title="Открыть в Finder"
            >
              {project.root}
            </button>
          </div>
        )}
      </header>

      {project && (
        <section className="project-description">
          {editDescription ? (
            <textarea
              className="project-description-edit"
              value={descriptionDraft}
              autoFocus
              rows={4}
              placeholder="Опиши зачем этот проект, что хочешь делать"
              onChange={(e) => setDescriptionDraft(e.currentTarget.value)}
              onBlur={() => {
                void saveDescription(descriptionDraft);
                setEditDescription(false);
              }}
              onKeyDown={(e) => {
                if (e.key === "Escape") {
                  setEditDescription(false);
                } else if (
                  e.key === "Enter" &&
                  (e.metaKey || e.ctrlKey)
                ) {
                  e.preventDefault();
                  void saveDescription(descriptionDraft);
                  setEditDescription(false);
                }
              }}
            />
          ) : (
            <div
              className={`project-description-view ${project.description ? "" : "project-description-empty"}`}
              onClick={() => {
                setDescriptionDraft(project.description ?? "");
                setEditDescription(true);
              }}
              title="Кликни чтобы редактировать"
            >
              {project.description ?? "Без описания. Кликни чтобы заполнить."}
            </div>
          )}
        </section>
      )}

      {project && (
        <section className="project-context-grid" aria-label="Agent context">
          <article className="project-context-item">
            <span className="project-context-label">Sandbox</span>
            <strong>{sandbox}</strong>
          </article>
          <article className="project-context-item">
            <span className="project-context-label">Agent profile</span>
            <strong>{hasAgentProfile ? "custom" : "default"}</strong>
            {projectSkills.length > 0 && (
              <div className="project-context-chips">
                {projectSkills.slice(0, 4).map((skill) => (
                  <span key={skill}>{skill}</span>
                ))}
                {projectSkills.length > 4 && (
                  <span>+{projectSkills.length - 4}</span>
                )}
              </div>
            )}
          </article>
          <article className="project-context-item">
            <span className="project-context-label">MCP servers</span>
            <strong>{mcpServerNames.length}</strong>
            {mcpServerNames.length > 0 && (
              <div className="project-context-chips">
                {mcpServerNames.slice(0, 4).map((name) => (
                  <span key={name}>{name}</span>
                ))}
                {mcpServerNames.length > 4 && (
                  <span>+{mcpServerNames.length - 4}</span>
                )}
              </div>
            )}
          </article>
          <article className="project-context-item">
            <span className="project-context-label">Apps</span>
            <strong>{linkedAppIds.length}</strong>
          </article>
          <article className="project-context-item">
            <span className="project-context-label">Topics</span>
            <strong>{topics.length}</strong>
            {runningCount > 0 && (
              <span className="project-context-note">
                {runningCount} running
              </span>
            )}
          </article>
        </section>
      )}

      {project && (() => {
        const sources: WidgetSource[] = [];
        for (const id of linkedAppIds) {
          const app = installedApps.find((a) => a.id === id);
          if (!app || app.ready === false) continue;
          for (const w of app.widgets ?? []) {
            sources.push({
              appId: app.id,
              appName: app.name,
              appIcon: app.icon ?? null,
              widget: w,
            });
          }
        }
        if (sources.length === 0 && linkedAppIds.length === 0) return null;
        return (
          <section className="project-dashboard">
            <h2 className="section-title">Дашборд</h2>
            <WidgetGrid sources={sources} onOpenApp={onOpenApp} />
          </section>
        );
      })()}

      {project && (
        <section className="project-linked">
          <div className="section-head">
            <h2 className="section-title">Привязанные утилиты</h2>
            <button
              className="apps-create-btn"
              onClick={() => setShowLinkPicker(true)}
            >
              + Привязать утилиту
            </button>
          </div>
          {(() => {
            const linked = linkedAppIds
              .map((id) => installedApps.find((a) => a.id === id))
              .filter((a): a is AppManifest => !!a);
            if (linked.length === 0) {
              return (
                <div className="project-linked-empty">
                  Утилит пока нет. Привяжи существующую или создай новую.
                </div>
              );
            }
            return (
              <ul className="project-linked-list">
                {linked.map((a) => (
                  <li key={a.id} className="project-linked-row">
                    <span className="project-linked-icon">
                      {a.icon ?? "🧩"}
                    </span>
                    <div className="project-linked-info">
                      <div className="project-linked-name">{a.name}</div>
                      {a.description && (
                        <div className="project-linked-desc">
                          {a.description}
                        </div>
                      )}
                    </div>
                    <div className="project-linked-actions">
                      <button
                        className="apps-trash-action"
                        onClick={() => onOpenApp(a.id)}
                      >
                        Открыть
                      </button>
                      <button
                        className="apps-trash-action"
                        onClick={() => void unlinkApp(a.id)}
                      >
                        Отвязать
                      </button>
                    </div>
                  </li>
                ))}
              </ul>
            );
          })()}
        </section>
      )}

      {project && (
        <section className="project-settings">
          <div className="setting-row">
            <label className="setting-label">Sandbox</label>
            <select
              className="setting-select"
              value={sandbox}
              onChange={(e) => void setSandbox(e.currentTarget.value)}
            >
              <option value="read-only">read-only (безопасно)</option>
              <option value="workspace-write">
                workspace-write (по умолчанию)
              </option>
              <option value="danger-full-access">
                danger-full-access ⚠️
              </option>
            </select>
            {sandbox === "danger-full-access" && (
              <span className="setting-hint setting-hint-warn">
                Агент работает без macOS Seatbelt — полные права доступа.
              </span>
            )}
          </div>
          <div className="setting-row setting-row-block">
            <label className="setting-label">Agent profile</label>
            <div className="setting-mcp-summary">
              {hasAgentProfile ? (
                <>
                  {project?.agent_instructions?.trim() && (
                    <span className="setting-chip setting-chip-muted">
                      instructions
                    </span>
                  )}
                  {projectSkills.map((skill) => (
                    <span key={skill} className="setting-chip">
                      {skill}
                    </span>
                  ))}
                </>
              ) : (
                <span className="setting-empty">default Codex behavior</span>
              )}
              <button
                className="setting-action"
                onClick={openAgentProfileEditor}
              >
                Edit profile
              </button>
            </div>
            {profileEditing && (
              <div className="setting-editor">
                <label className="setting-editor-label">
                  Project instructions injected into every topic
                </label>
                <textarea
                  className="setting-textarea"
                  value={profileInstructionsDraft}
                  spellCheck={false}
                  onChange={(e) =>
                    setProfileInstructionsDraft(e.currentTarget.value)
                  }
                  rows={6}
                  placeholder="Example: Always prefer small focused changes. Use browser MCP for visual verification. Keep answers in Russian unless code/API names require English."
                />
                <label className="setting-editor-label">
                  Preferred skills, one per line or comma-separated
                </label>
                <textarea
                  className="setting-textarea setting-textarea-compact"
                  value={profileSkillsDraft}
                  spellCheck={false}
                  onChange={(e) =>
                    setProfileSkillsDraft(e.currentTarget.value)
                  }
                  rows={3}
                  placeholder={
                    "build-web-apps:react-best-practices\nplaywright\nopenai-docs"
                  }
                />
                <div className="setting-skill-presets">
                  {SKILL_PRESETS.map((skill) => {
                    const selected = profileSkillDraftSet.has(
                      skill.id.toLowerCase(),
                    );
                    return (
                      <button
                        key={skill.id}
                        className={`setting-skill-preset ${selected ? "selected" : ""}`}
                        type="button"
                        onClick={() => appendProfileSkill(skill.id)}
                        disabled={selected}
                        title={skill.id}
                      >
                        {selected ? "✓ " : "+ "}
                        {skill.label}
                      </button>
                    );
                  })}
                </div>
                {profileError && (
                  <div className="setting-error">{profileError}</div>
                )}
                <div className="setting-editor-actions">
                  <button
                    className="setting-action"
                    onClick={() => setProfileEditing(false)}
                    disabled={profileSaving}
                  >
                    Cancel
                  </button>
                  <button
                    className="setting-action setting-action-primary"
                    onClick={() => void saveAgentProfile()}
                    disabled={profileSaving}
                  >
                    {profileSaving ? "Saving…" : "Save"}
                  </button>
                </div>
              </div>
            )}
          </div>
          <div className="setting-row">
            <label className="setting-label">Browser MCP</label>
            <label className="setting-toggle">
              <input
                type="checkbox"
                checked={browserOn}
                onChange={(e) => void setBrowser(e.currentTarget.checked)}
              />
              {browserOn ? "включен" : "выключен"}
            </label>
            {browserOn && (
              <span className="setting-hint">
                Встроенный Reflex browser bridge подключится при следующем
                старте или resume треда.
              </span>
            )}
          </div>
          <div className="setting-row setting-row-block">
            <label className="setting-label">MCP servers</label>
            <div className="setting-mcp-summary">
              {mcpServerNames.length === 0 ? (
                <span className="setting-empty">none</span>
              ) : (
                mcpServerNames.map((name) => (
                  <span key={name} className="setting-chip">
                    {name}
                  </span>
                ))
              )}
              <button className="setting-action" onClick={openMcpEditor}>
                Edit JSON
              </button>
            </div>
            {mcpEditing && (
              <div className="setting-editor">
                <textarea
                  className="setting-textarea"
                  value={mcpDraft}
                  spellCheck={false}
                  onChange={(e) => setMcpDraft(e.currentTarget.value)}
                  rows={8}
                />
                {mcpError && <div className="setting-error">{mcpError}</div>}
                <div className="setting-editor-actions">
                  <button
                    className="setting-action"
                    onClick={() => setMcpEditing(false)}
                    disabled={mcpSaving}
                  >
                    Cancel
                  </button>
                  <button
                    className="setting-action setting-action-primary"
                    onClick={() => void saveMcpServers()}
                    disabled={mcpSaving}
                  >
                    {mcpSaving ? "Saving…" : "Save"}
                  </button>
                </div>
              </div>
            )}
          </div>
        </section>
      )}

      <section>
        <div className="section-head">
          <h2 className="section-title">
            Topics
            {runningCount > 0 && (
              <span className="section-badge running">
                {runningCount} running
              </span>
            )}
          </h2>
          <button
            className="apps-create-btn"
            onClick={() => setShowNewTopic(true)}
          >
            + New Topic
          </button>
        </div>
        {topics.length === 0 ? (
          <div className="section-empty">
            Топиков пока нет. Нажми <b>+ New Topic</b> или открой Quick-панель (
            <kbd>⌘⇧Space</kbd>) поверх этой папки.
          </div>
        ) : (
          <ul className="topic-list">
            {topics.map((t) => (
              <li key={t.id}>
                <button
                  className="topic-row topic-row-with-status"
                  onClick={() => onSelectTopic(t.id)}
                >
                  <StatusDot
                    done={t.done}
                    ok={t.exit_code === 0}
                  />
                  <div className="topic-row-body">
                    <span className="topic-row-prompt">
                      {t.title ?? t.prompt}
                    </span>
                    <span className="topic-row-meta">
                      {t.done
                        ? t.exit_code === 0
                          ? "done"
                          : `exit ${t.exit_code ?? "?"}`
                        : "running"}
                      {" · "}
                      {new Date(t.created_at_ms).toLocaleString()}
                    </span>
                  </div>
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section>
        <div className="section-head">
          <h2 className="section-title">Files</h2>
          <label className="section-toggle">
            <input
              type="checkbox"
              checked={showHidden}
              onChange={(e) => setShowHidden(e.currentTarget.checked)}
            />
            показать скрытые
          </label>
        </div>
        {visibleEntries.length === 0 ? (
          <div className="section-empty">Папка пуста.</div>
        ) : (
          <ul className="file-list">
            {visibleEntries.map((e) => {
              const s = statuses[e.path];
              const ignored =
                s?.class === "binary" ||
                s?.class === "toolarge" ||
                s?.class === "unsupported";
              const stateClass = !s
                ? ""
                : s.indexed && s.stale
                  ? "file-stale"
                  : s.indexed
                    ? "file-indexed"
                    : ignored
                      ? "file-ignored"
                      : "file-fresh";
              const dotTitle = !s
                ? ""
                : s.indexed && s.stale
                  ? "В RAG, но изменён после индексации — переиндексируй"
                  : s.indexed
                    ? `В RAG${s.indexed_under ? ` (${s.indexed_under} док.)` : ""}`
                    : ignored
                      ? "Не индексируется (бинарный / большой / неподдерживаемый)"
                      : "Можно проиндексировать";
              return (
                <li key={e.path}>
                  <button
                    className={`file-row ${stateClass}`}
                    onClick={(ev) => {
                      if (ev.altKey) {
                        openExternal(e.path);
                      } else {
                        setDrawerTarget(e);
                      }
                    }}
                    title={`${e.path}\n${dotTitle}\n(Alt+клик — открыть в Finder)`}
                  >
                    <span
                      className="file-status-dot"
                      aria-label={dotTitle}
                    />
                    <span className="file-icon">
                      {e.kind === "directory"
                        ? "📁"
                        : e.kind === "symlink"
                          ? "🔗"
                          : "📄"}
                    </span>
                    <span className="file-name">{e.name}</span>
                    {e.modified_ms != null && (
                      <span className="file-meta">
                        {new Date(e.modified_ms).toLocaleDateString()}
                      </span>
                    )}
                  </button>
                </li>
              );
            })}
          </ul>
        )}
      </section>

      {showLinkPicker && project && (
        <div
          className="modal-backdrop"
          onClick={() => setShowLinkPicker(false)}
        >
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <h2 className="modal-title">Привязать утилиту</h2>
            <p className="modal-hint">
              Утилиты, не привязанные к этому проекту. Клик по строке —
              привязать.
            </p>
            {(() => {
              const linkedIds = new Set(project.apps ?? []);
              const available = installedApps.filter(
                (a) => !linkedIds.has(a.id),
              );
              if (available.length === 0) {
                return (
                  <div className="project-linked-empty">
                    Все доступные утилиты уже привязаны.
                  </div>
                );
              }
              return (
                <ul className="project-linked-list">
                  {available.map((a) => (
                    <li
                      key={a.id}
                      className="project-linked-row project-linked-row-clickable"
                      onClick={() => void linkApp(a.id)}
                    >
                      <span className="project-linked-icon">
                        {a.icon ?? "🧩"}
                      </span>
                      <div className="project-linked-info">
                        <div className="project-linked-name">
                          {a.name}
                          {a.ready === false && (
                            <span className="apps-card-badge">creating…</span>
                          )}
                        </div>
                        {a.description && (
                          <div className="project-linked-desc">
                            {a.description}
                          </div>
                        )}
                      </div>
                    </li>
                  ))}
                </ul>
              );
            })()}
            <div className="modal-actions">
              <button
                className="modal-btn"
                onClick={() => setShowLinkPicker(false)}
              >
                Отмена
              </button>
            </div>
          </div>
        </div>
      )}

      {showNewTopic && (
        <div
          className="modal-backdrop"
          onClick={() => !creatingTopic && setShowNewTopic(false)}
        >
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <h2 className="modal-title">Новый топик в проекте</h2>
            <p className="modal-hint">
              Опиши задачу. Codex стартует тред в текущей папке проекта.
            </p>
            <textarea
              className="modal-input"
              placeholder="Например: добавь поддержку dark mode в config; проверь lints; перепиши парсер на nom…"
              value={newTopicPrompt}
              onChange={(e) => setNewTopicPrompt(e.currentTarget.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                  e.preventDefault();
                  void submitNewTopic();
                }
              }}
              autoFocus
              rows={5}
            />
            {topicError && <div className="apps-error">{topicError}</div>}
            <label className="plan-toggle">
              <input
                type="checkbox"
                checked={newTopicPlanMode}
                onChange={(e) => setNewTopicPlanMode(e.currentTarget.checked)}
              />
              <span>📋 Сначала план (codex составит план, ты подтвердишь)</span>
            </label>
            <div className="modal-actions">
              <button
                className="modal-btn"
                disabled={creatingTopic}
                onClick={() => setShowNewTopic(false)}
              >
                Отмена
              </button>
              <button
                className="modal-btn modal-btn-primary"
                disabled={creatingTopic || !newTopicPrompt.trim()}
                onClick={() => void submitNewTopic()}
              >
                {creatingTopic ? "Запускаю…" : "Создать (⌘↵)"}
              </button>
            </div>
          </div>
        </div>
      )}
      {project && (
        <FileActionsDrawer
          target={drawerTarget}
          projectRoot={project.root}
          onClose={() => setDrawerTarget(null)}
          onStartTopic={async (prompt, planMode) => {
            await onCreateTopic(prompt, planMode ?? false);
          }}
          onStatusChanged={() => setStatusTick((n) => n + 1)}
        />
      )}
    </div>
  );
}

function StatusDot({ done, ok }: { done: boolean; ok: boolean }) {
  if (!done) return <span className="status-dot status-dot-running" />;
  if (ok) return <span className="status-dot status-dot-ok" />;
  return <span className="status-dot status-dot-fail" />;
}

function TopicScreen({
  thread_id,
  threads,
  projects,
  onOpenLink,
}: {
  thread_id: string;
  threads: Thread[];
  projects: Project[];
  onOpenLink?: (
    threadId: string,
    url: string,
    ev: React.MouseEvent<HTMLAnchorElement>,
  ) => void;
}) {
  const thread = threads.find((t) => t.id === thread_id);
  const bottomRef = useRef<HTMLDivElement>(null);
  const [showRecall, setShowRecall] = useState(false);

  // Scroll to bottom on initial mount / when switching to this thread.
  useEffect(() => {
    const id = requestAnimationFrame(() => {
      bottomRef.current?.scrollIntoView({ block: "end" });
    });
    return () => cancelAnimationFrame(id);
  }, [thread_id]);

  if (!thread) {
    return (
      <div className="chat-empty">
        <p>Тред не найден или ещё грузится…</p>
      </div>
    );
  }

  const project = projects.find((p) => p.id === thread.project_id);
  const projectRoot = project?.root ?? thread.cwd ?? "";
  const recallQuery = mostRecentTopicPrompt(thread);

  return (
    <ol className="chat-list">
      <li className="chat-item-controls">
        <button
          type="button"
          className="header-tab"
          onClick={() => setShowRecall((v) => !v)}
          title="Toggle memory recall for this topic"
        >
          {showRecall ? "Hide recall" : "Recall"}
        </button>
      </li>
      {showRecall && projectRoot && (
        <li className="chat-recall-wrap">
          <RecallView
            projectRoot={projectRoot}
            threadId={thread.id}
            query={recallQuery}
          />
        </li>
      )}
      <ThreadCard thread={thread} onOpenLink={onOpenLink} />
      <div ref={bottomRef} />
    </ol>
  );
}

function mostRecentTopicPrompt(thread: Thread): string {
  // Prefer the most recent user-stream event text; fall back to the original prompt.
  for (let i = thread.events.length - 1; i >= 0; i--) {
    const ev = thread.events[i];
    if (ev.stream === "user") {
      const text = (ev.raw ?? "").trim();
      if (text) return text;
    }
  }
  return thread.prompt ?? "";
}

function isPlanApprovalText(text: string): boolean {
  const normalized = text.trim().toLowerCase();
  return /^(go|go!|run|start|yes|y|ok|okay|да|ок|старт|выполняй)$/.test(
    normalized,
  );
}

function ThreadCard({
  thread,
  onOpenLink,
}: {
  thread: Thread;
  onOpenLink?: (
    threadId: string,
    url: string,
    ev: React.MouseEvent<HTMLAnchorElement>,
  ) => void;
}) {
  const [followup, setFollowup] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Banner появляется только когда у треда уже есть выдача агента (план написан)
  // и тред в done-состоянии. На пустом или ещё работающем треде — скрыт.
  const latestUserSeq = thread.events.reduce(
    (max, ev) => (ev.stream === "user" ? Math.max(max, ev.seq) : max),
    0,
  );
  const hasAgentOutput = thread.events.some((ev) => {
    if (ev.seq <= latestUserSeq) return false;
    if (ev.stream !== "stdout") return false;
    const msg = ev.parsed?.msg ?? ev.parsed ?? {};
    const t: string = msg.type ?? ev.parsed?.type ?? "";
    if (t === "item.agentMessage.delta" || t === "agent_message_delta") return true;
    if (t === "agent_message") return true;
    if (t === "item.completed") {
      const item = msg.item ?? ev.parsed?.item ?? {};
      const it: string = (item.type ?? item.kind ?? "").toString().toLowerCase();
      if (it.includes("agentmessage") || it.includes("agent_message")) return true;
      if (it === "assistant" || it.includes("assistantmessage")) return true;
    }
    return false;
  });
  const showPlanBanner =
    thread.plan_mode &&
    !thread.plan_confirmed &&
    thread.done &&
    hasAgentOutput;

  const status = thread.done
    ? thread.exit_code === 0
      ? "done"
      : `exit ${thread.exit_code ?? "?"}`
    : "running";

  const running = !thread.done;
  const followupDisabled = submitting;

  async function sendFollowup() {
    const text = followup.trim();
    if (!text || followupDisabled) return;
    setError(null);
    setSubmitting(true);
    try {
      if (running) {
        try {
          await invoke("stop_thread", { threadId: thread.id });
        } catch (e) {
          console.warn("[reflex] stop before send failed", e);
        }
      }
      const confirmsPlan = showPlanBanner && isPlanApprovalText(text);
      const args: Record<string, unknown> = {
        projectId: thread.project_id,
        threadId: thread.id,
        prompt: confirmsPlan ? "go — выполняй план как описал." : text,
      };
      if (confirmsPlan) args.planConfirmed = true;
      await invoke("continue_thread", args);
      setFollowup("");
    } catch (e) {
      console.error("[reflex] continue_thread failed", e);
      setError(String(e));
    } finally {
      setSubmitting(false);
    }
  }

  async function confirmPlan() {
    if (!thread.done || submitting) return;
    setError(null);
    setSubmitting(true);
    try {
      await invoke("continue_thread", {
        projectId: thread.project_id,
        threadId: thread.id,
        prompt: "go — выполняй план как описал.",
        planConfirmed: true,
      });
    } catch (e) {
      console.error("[reflex] confirmPlan failed", e);
      setError(String(e));
    } finally {
      setSubmitting(false);
    }
  }

  async function stopThread() {
    if (stopping) return;
    setError(null);
    setStopping(true);
    try {
      await invoke("stop_thread", { threadId: thread.id });
    } catch (e) {
      console.error("[reflex] stop_thread failed", e);
      setError(String(e));
    } finally {
      setStopping(false);
    }
  }

  return (
    <li className="chat-item">
      <header className="chat-item-header">
        <span className="chat-item-id">{thread.id}</span>
        <span
          className={`chat-status chat-status-${thread.done ? (thread.exit_code === 0 ? "ok" : "fail") : "running"}`}
        >
          {status}
        </span>
        <time className="chat-item-time">
          {new Date(thread.created_at_ms).toLocaleTimeString()}
        </time>
      </header>
      {thread.title && <h2 className="chat-item-title">{thread.title}</h2>}
      {thread.goal && <p className="chat-item-goal">🎯 {thread.goal}</p>}
      <p className="chat-item-prompt">{thread.prompt}</p>
      {(thread.ctx.frontmost_app || thread.cwd) && (
        <div className="chat-item-ctx">
          {thread.ctx.frontmost_app && (
            <span className="chat-chip">{thread.ctx.frontmost_app}</span>
          )}
          <span className="chat-chip chat-chip-path" title={thread.cwd}>
            cwd: {thread.cwd}
          </span>
        </div>
      )}
      {thread.source === "browser" && thread.browser_tabs.length > 0 && (
        <div className="chat-item-ctx chat-item-tabs">
          <span className="chat-chip chat-chip-source">🌐 browser</span>
          {thread.browser_tabs.map((t, i) => (
            <a
              key={i}
              href={t.url}
              target="_blank"
              rel="noreferrer"
              className="chat-chip chat-chip-tab"
              title={t.url}
            >
              {t.title?.trim() || t.url}
            </a>
          ))}
        </div>
      )}
      <ul className="chat-events">
        {groupEvents(thread.events).map((it) => (
          <RenderRow
            key={`${it.kind}:${it.seq}`}
            item={it}
            onLinkClick={
              onOpenLink
                ? (url, ev) => onOpenLink(thread.id, url, ev)
                : undefined
            }
          />
        ))}
        {!thread.done && (thread.pending_questions ?? []).length === 0 && (
          <li className="chat-event chat-event-spinner">…</li>
        )}
      </ul>
      {(thread.pending_questions ?? []).map((q) => (
        <QuestionCard
          key={q.question_id}
          question={q}
          onResolved={(qid) => {
            // local removal
            // setThreads not available here; emit via window event or callback
            const ev = new CustomEvent("reflex-question-resolved", {
              detail: { thread_id: thread.id, question_id: qid },
            });
            window.dispatchEvent(ev);
          }}
        />
      ))}
      {showPlanBanner && (
        <div className="plan-banner">
          <div className="plan-banner-text">
            📋 <strong>Plan mode.</strong> Codex составил план — проверь и
            подтверди, или напиши в поле ниже что поправить.
          </div>
          <button
            className="appviewer-btn appviewer-btn-primary"
            disabled={submitting}
            onClick={() => void confirmPlan()}
          >
            {submitting ? "…" : "✓ Confirm & run"}
          </button>
        </div>
      )}
      <div className="chat-followup">
        {running && (
          <span className="chat-followup-running">Codex работает…</span>
        )}
        <input
          className="chat-followup-input"
          type="text"
          placeholder={
            running
              ? "Прервать и отправить новое сообщение…"
              : showPlanBanner
                ? "Поправь план или напиши `go`…"
                : "Продолжить тред…"
          }
          value={followup}
          onChange={(e) => setFollowup(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              void sendFollowup();
            }
          }}
          disabled={followupDisabled}
        />
        {running && (
          <button
            className="chat-followup-button chat-followup-stop"
            onClick={() => void stopThread()}
            disabled={stopping || submitting}
            title="Остановить агента без сообщения"
          >
            {stopping ? "…" : "Stop"}
          </button>
        )}
        <button
          className="chat-followup-button"
          onClick={() => void sendFollowup()}
          disabled={followupDisabled || !followup.trim()}
          title={
            running
              ? "Прервать агента и отправить сообщение"
              : "Отправить"
          }
        >
          {running ? "⤳" : "↵"}
        </button>
      </div>
      {error && <div className="chat-followup-error">{error}</div>}
    </li>
  );
}

const APPROVAL_METHODS = new Set([
  "applyPatchApproval",
  "execCommandApproval",
  "item/commandExecution/requestApproval",
  "item/fileChange/requestApproval",
  "item/permissions/requestApproval",
]);

const INPUT_METHODS = new Set([
  "item/tool/requestUserInput",
  "mcpServer/elicitation/request",
]);

function QuestionCard({
  question,
  onResolved,
}: {
  question: ThreadQuestion;
  onResolved: (id: string) => void;
}) {
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const isInput = INPUT_METHODS.has(question.method);
  const isApproval = APPROVAL_METHODS.has(question.method);

  async function respond(decision: string) {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await invoke("respond_to_question", {
        questionId: question.question_id,
        decision,
        text: text || null,
      });
      onResolved(question.question_id);
    } catch (e) {
      console.error("[reflex] respond_to_question failed", e);
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  const params = question.params ?? {};
  const reason: string | undefined = params.reason ?? undefined;
  const command = params.command;
  const cwd: string | undefined = params.cwd ?? undefined;
  const fileChanges = params.fileChanges;
  const grantRoot: string | undefined = params.grantRoot ?? undefined;
  const questions = params.questions;

  return (
    <div className="question-card">
      <header className="question-header">
        <span className="question-icon">❓</span>
        <span className="question-method">{question.method}</span>
      </header>
      {reason && <p className="question-reason">{reason}</p>}
      {command && (
        <div className="question-detail">
          <span className="question-detail-label">command</span>
          <code className="question-detail-cmd">
            {Array.isArray(command) ? command.join(" ") : String(command)}
          </code>
        </div>
      )}
      {cwd && (
        <div className="question-detail">
          <span className="question-detail-label">cwd</span>
          <code className="question-detail-cmd">{cwd}</code>
        </div>
      )}
      {fileChanges && (
        <div className="question-detail">
          <span className="question-detail-label">files</span>
          <code className="question-detail-cmd">
            {Object.keys(fileChanges).join(", ")}
          </code>
        </div>
      )}
      {grantRoot && (
        <div className="question-detail">
          <span className="question-detail-label">grant root</span>
          <code className="question-detail-cmd">{grantRoot}</code>
        </div>
      )}
      {Array.isArray(questions) && questions.length > 0 && (
        <ul className="question-list">
          {questions.map((q: any, i: number) => (
            <li key={i} className="question-detail">
              <span className="question-detail-label">q{i + 1}</span>
              <span>{q?.question ?? q?.prompt ?? JSON.stringify(q)}</span>
            </li>
          ))}
        </ul>
      )}
      {isInput && (
        <textarea
          className="question-input"
          rows={3}
          placeholder="Ответ агенту…"
          value={text}
          onChange={(e) => setText(e.currentTarget.value)}
        />
      )}
      <div className="question-actions">
        {isApproval && (
          <>
            <button
              className="question-btn question-btn-approve"
              disabled={busy}
              onClick={() => void respond("approved")}
            >
              Approve
            </button>
            <button
              className="question-btn"
              disabled={busy}
              onClick={() => void respond("approved_for_session")}
            >
              Approve for session
            </button>
            <button
              className="question-btn question-btn-deny"
              disabled={busy}
              onClick={() => void respond("denied")}
            >
              Deny
            </button>
          </>
        )}
        {isInput && (
          <button
            className="question-btn question-btn-approve"
            disabled={busy || !text.trim()}
            onClick={() => void respond("approved")}
          >
            Send
          </button>
        )}
        {!isApproval && !isInput && (
          <button
            className="question-btn question-btn-approve"
            disabled={busy}
            onClick={() => void respond("approved")}
          >
            Approve
          </button>
        )}
      </div>
      {error && <div className="question-error">{error}</div>}
    </div>
  );
}

const STDERR_NOISE = [
  "Reading additional input from stdin",
  "ERROR codex_core::session: failed to record rollout items",
];

const NOISE_MSG_TYPES = new Set([
  "thread.started",
  "turn.started",
  "session_configured",
  "task_started",
  "item.started",
  "agent_reasoning_delta",
  "agent_reasoning",
]);

type RenderItem =
  | { kind: "user"; seq: number; text: string }
  | { kind: "agent"; seq: number; text: string; partial: boolean }
  | {
      kind: "exec";
      seq: number;
      command: any;
      exitCode?: number | null;
      cwd?: string;
    }
  | { kind: "error"; seq: number; text: string };

function extractText(v: any): string | null {
  if (v == null) return null;
  if (typeof v === "string") return v;
  if (Array.isArray(v)) {
    const parts = v.map(extractText).filter((x): x is string => !!x);
    return parts.length ? parts.join("\n") : null;
  }
  if (typeof v === "object") {
    return (
      extractText(v.text) ??
      extractText(v.content) ??
      extractText(v.message) ??
      null
    );
  }
  return String(v);
}

function lowerType(item: any): string {
  return (item?.type ?? item?.kind ?? "").toString().toLowerCase();
}

function isReasoningItem(item: any): boolean {
  return lowerType(item).includes("reasoning");
}

function isAgentMessageItem(item: any): boolean {
  const t = lowerType(item);
  // codex app-server: "agentMessage" → "agentmessage"; legacy: "agent_message"
  return (
    t.includes("agentmessage") ||
    t.includes("agent_message") ||
    t.includes("assistantmessage") ||
    t.includes("assistant_message") ||
    t === "assistant" ||
    t === "agent"
  );
}

function isCommandItem(item: any): boolean {
  const t = lowerType(item);
  return (
    t.includes("commandexecution") ||
    t.includes("command_execution") ||
    t.includes("exec") ||
    t === "shell"
  );
}

function isFileChangeItem(item: any): boolean {
  const t = lowerType(item);
  return t.includes("filechange") || t.includes("file_change");
}

function groupEvents(events: ThreadEvent[]): RenderItem[] {
  const out: RenderItem[] = [];
  let deltaBuffer: { seq: number; text: string } | null = null;

  const flushDelta = () => {
    if (deltaBuffer && deltaBuffer.text.trim()) {
      out.push({
        kind: "agent",
        seq: deltaBuffer.seq,
        text: deltaBuffer.text,
        partial: true,
      });
    }
    deltaBuffer = null;
  };

  for (const ev of events) {
    if (ev.stream === "user") {
      flushDelta();
      let text = ev.raw;
      try {
        const parsed = JSON.parse(ev.raw);
        text = parsed?.text ?? parsed?.message ?? ev.raw;
      } catch {}
      out.push({ kind: "user", seq: ev.seq, text });
      continue;
    }
    if (ev.stream === "stderr" || ev.stream === "error") {
      if (STDERR_NOISE.some((n) => ev.raw.includes(n))) continue;
      flushDelta();
      out.push({ kind: "error", seq: ev.seq, text: ev.raw });
      continue;
    }

    const parsed = ev.parsed;
    const msg = parsed?.msg ?? parsed ?? {};
    const msgType: string = msg.type ?? parsed?.type ?? "event";

    if (NOISE_MSG_TYPES.has(msgType)) continue;

    // Agent text streaming: codex app-server emits "item.agentMessage.delta",
    // legacy codex exec emits "agent_message_delta".
    if (
      (msgType === "item.agentMessage.delta" ||
        msgType === "agent_message_delta") &&
      msg.delta != null
    ) {
      const piece = extractText(msg.delta) ?? "";
      if (deltaBuffer) deltaBuffer.text += piece;
      else deltaBuffer = { seq: ev.seq, text: piece };
      continue;
    }
    if (msgType === "agent_message" && msg.message) {
      deltaBuffer = null;
      const text = extractText(msg.message) ?? "";
      if (text.trim())
        out.push({ kind: "agent", seq: ev.seq, text, partial: false });
      continue;
    }
    if (msgType === "item.completed") {
      const item = msg.item ?? parsed?.item ?? {};
      if (isReasoningItem(item)) continue;
      if (isAgentMessageItem(item)) {
        deltaBuffer = null;
        const text = extractText(item) ?? "";
        if (text.trim())
          out.push({ kind: "agent", seq: ev.seq, text, partial: false });
        continue;
      }
      if (isCommandItem(item)) {
        flushDelta();
        out.push({
          kind: "exec",
          seq: ev.seq,
          command: item.command ?? item.cmd,
          exitCode: item.exit_code ?? item.exitCode ?? null,
          cwd: item.cwd,
        });
        continue;
      }
      if (isFileChangeItem(item)) {
        flushDelta();
        const changes =
          item.changes ?? item.files ?? item.fileChanges ?? null;
        const filesText = Array.isArray(changes)
          ? changes
              .map((c: any) =>
                typeof c === "string" ? c : c?.path ?? c?.file ?? JSON.stringify(c),
              )
              .join(", ")
          : changes && typeof changes === "object"
            ? Object.keys(changes).join(", ")
            : extractText(item) ?? "(file change)";
        out.push({
          kind: "agent",
          seq: ev.seq,
          text: `📝 _patched:_ ${filesText}`,
          partial: false,
        });
        continue;
      }
      // any other item.completed types — skip silently
      continue;
    }
    if (msgType === "exec_command_begin") {
      flushDelta();
      out.push({
        kind: "exec",
        seq: ev.seq,
        command: msg.command ?? msg.cmd,
        cwd: msg.cwd,
      });
      continue;
    }
    if (msgType === "exec_command_end") {
      for (let i = out.length - 1; i >= 0; i--) {
        const it = out[i];
        if (it.kind === "exec" && it.exitCode == null) {
          it.exitCode = msg.exit_code ?? null;
          break;
        }
      }
      continue;
    }
    if (msgType === "turn.completed" || msgType === "task_complete") {
      flushDelta();
      const turnObj = msg.turn ?? msg;
      const tail =
        turnObj.last_agent_message ??
        turnObj.lastAgentMessage ??
        msg.summary ??
        null;
      if (tail) {
        const text = extractText(tail) ?? String(tail);
        // Only emit if we don't already have any agent block from this turn.
        // Otherwise it duplicates.
        const sameSeq = out.some(
          (it) => it.kind === "agent" && Math.abs(it.seq - ev.seq) < 3,
        );
        if (text.trim() && !sameSeq)
          out.push({ kind: "agent", seq: ev.seq, text, partial: false });
      }
      continue;
    }
    // Everything else — quietly drop. Add to NOISE_MSG_TYPES if it's spammy.
  }
  flushDelta();
  return out;
}

type LinkClickHandler = (
  url: string,
  ev: React.MouseEvent<HTMLAnchorElement>,
) => void;

function makeMdComponents(onLinkClick?: LinkClickHandler) {
  return {
    a: ({ href, children }: any) => {
      const handleClick = (ev: React.MouseEvent<HTMLAnchorElement>) => {
        if (!href || !onLinkClick) return;
        // Let the user keep modifier-clicks for the system browser.
        if (
          ev.metaKey ||
          ev.ctrlKey ||
          ev.shiftKey ||
          ev.altKey ||
          ev.button !== 0
        ) {
          return;
        }
        ev.preventDefault();
        onLinkClick(String(href), ev);
      };
      return (
        <a
          href={href}
          target="_blank"
          rel="noreferrer"
          onClick={handleClick}
          onAuxClick={(ev) => {
            // middle-click — leave alone, browser opens in new window
            void ev;
          }}
        >
          {children}
        </a>
      );
    },
  };
}

const DEFAULT_MD_COMPONENTS = makeMdComponents();

function MarkdownText({
  text,
  onLinkClick,
}: {
  text: string;
  onLinkClick?: LinkClickHandler;
}) {
  const components = useMemo(
    () => (onLinkClick ? makeMdComponents(onLinkClick) : DEFAULT_MD_COMPONENTS),
    [onLinkClick],
  );
  return (
    <div className="md">
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={components}>
        {text}
      </ReactMarkdown>
    </div>
  );
}

function RenderRow({
  item,
  onLinkClick,
}: {
  item: RenderItem;
  onLinkClick?: LinkClickHandler;
}) {
  if (item.kind === "user") {
    return (
      <li className="chat-event chat-event-user">
        <span className="chat-event-label">user</span>
        <div className="chat-event-text">{item.text}</div>
      </li>
    );
  }
  if (item.kind === "agent") {
    return (
      <li
        className={`chat-event chat-event-message ${item.partial ? "chat-event-partial" : ""}`}
      >
        <span className="chat-event-label">
          agent{item.partial ? " · streaming" : ""}
        </span>
        <div className="chat-event-text">
          <MarkdownText text={item.text} onLinkClick={onLinkClick} />
        </div>
      </li>
    );
  }
  if (item.kind === "exec") {
    const cmd = item.command;
    const cmdStr = Array.isArray(cmd)
      ? cmd.join(" ")
      : cmd != null
        ? String(cmd)
        : "(no command)";
    const ec = item.exitCode;
    const ecKnown = ec != null && ec !== undefined;
    return (
      <li className="chat-event chat-event-exec">
        <span className="chat-event-label">▶ exec</span>
        <code className="chat-event-cmd">{cmdStr}</code>
        {ecKnown && (
          <span className={ec === 0 ? "chat-event-ok" : "chat-event-fail"}>
            exit {String(ec)}
          </span>
        )}
      </li>
    );
  }
  if (item.kind === "error") {
    return (
      <li className="chat-event chat-event-err">
        <pre>{item.text}</pre>
      </li>
    );
  }
  return null;
}
