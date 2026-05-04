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
import { TopicComposer, type TopicComposerSendMeta } from "./TopicComposer";
import {
  BRIDGE_API_GROUPS,
  BRIDGE_HELPER_GROUPS,
  BRIDGE_RECIPE_CARDS,
} from "../appBridgeCatalog";
import {
  bridgeCatalogTitle,
  bridgeRecipeBody,
  bridgeRecipeTitle,
} from "../appBridgeCatalogI18n";
import { useI18n, type Translate } from "../i18n";
import "./ChatThread.css";

const BRIDGE_API_COUNT = BRIDGE_API_GROUPS.reduce(
  (sum, group) => sum + group.methods.length,
  0,
);

const BRIDGE_HELPER_COUNT = BRIDGE_HELPER_GROUPS.reduce(
  (sum, group) => sum + group.helpers.length,
  0,
);

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

type ProjectMemoryStats = {
  docs: number;
  chunks: number;
  sources: number;
  stale: number;
  missing: number;
  last_indexed_at_ms?: number | null;
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
  goal?: string | null;
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

type AppOpenRequestPayload = {
  app_id?: string;
  panel?: string;
  project_id?: string;
  thread_id?: string;
  from_app?: string;
};

type ProjectOpenRequestPayload = {
  project_id: string;
  from_app?: string;
};

type TopicOpenRequestPayload = {
  project_id?: string;
  thread_id: string;
  from_app?: string;
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
      project_id?: string;
    }
  | { kind: "app"; app_id: string }
  | { kind: "memory"; project_id?: string; thread_id?: string }
  | { kind: "automations" }
  | { kind: "browser"; project_id?: string }
  | { kind: "settings" };

function routeKey(r: Route): string {
  switch (r.kind) {
    case "home":
      return "home";
    case "apps":
      return r.project_id ? `apps:${r.project_id}` : "apps";
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
      return r.project_id ? `browser:${r.project_id}` : "browser";
    case "settings":
      return "settings";
  }
}

function routeForSystemPanel(payload: AppOpenRequestPayload): Route | null {
  switch (payload.panel?.trim()) {
    case "apps":
      return { kind: "apps" };
    case "memory":
      return {
        kind: "memory",
        project_id: payload.project_id,
        thread_id: payload.thread_id,
      };
    case "automations":
      return { kind: "automations" };
    case "browser":
      return { kind: "browser", project_id: payload.project_id };
    case "settings":
      return { kind: "settings" };
    default:
      return null;
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
  t: Translate,
): string {
  switch (r.kind) {
    case "home":
      return t("nav.home");
    case "apps":
      if (!r.project_id) return t("nav.apps");
      {
        const p = projects.find((x) => x.id === r.project_id);
        return `${t("nav.apps")} · ${p?.name ?? r.project_id}`;
      }
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
        const thread = threads.find((x) => x.id === r.thread_id);
        return t("nav.memoryWithName", {
          name: thread?.title ?? thread?.prompt?.slice(0, 32) ?? r.thread_id,
        });
      }
      if (!r.project_id) return t("nav.memory");
      const p = projects.find((x) => x.id === r.project_id);
      return t("nav.memoryWithName", { name: p?.name ?? r.project_id });
    }
    case "automations":
      return t("nav.automations");
    case "browser": {
      if (!r.project_id) return t("nav.browser");
      const p = projects.find((x) => x.id === r.project_id);
      return `${t("nav.browser")} · ${p?.name ?? r.project_id}`;
    }
    case "settings":
      return t("nav.settings");
  }
}

function projectIdFromRoute(
  route: Route,
  threads: Thread[],
): string | undefined {
  switch (route.kind) {
    case "project":
      return route.project_id;
    case "topic":
      return threads.find((thread) => thread.id === route.thread_id)
        ?.project_id;
    case "apps":
    case "browser":
      return route.project_id;
    case "memory":
      return (
        route.project_id ??
        (route.thread_id
          ? threads.find((thread) => thread.id === route.thread_id)
              ?.project_id
          : undefined)
      );
    default:
      return undefined;
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

type AppPermissionRequest = {
  id: string;
  status?: string;
  reason?: string | null;
  permissions?: string[];
  network_hosts?: string[];
  server_listen?: boolean;
  created_at_ms?: number;
  resolved_at_ms?: number | null;
  resolved_note?: string | null;
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
  runtime?: "static" | "server" | "external" | string | null;
  server?: { command: string[]; ready_timeout_ms?: number | null } | null;
  external?: {
    url?: string | null;
    title?: string | null;
    open_url?: string | null;
  } | null;
  integration?: {
    provider?: string | null;
    display_name?: string | null;
    capabilities?: string[];
    data_model?: any;
    auth?: any;
    mcp?: any;
    notes?: string | null;
  } | null;
  network?: AppNetworkPolicy | null;
  permission_requests?: AppPermissionRequest[];
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

async function copyTextToClipboard(text: string): Promise<void> {
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(text);
    return;
  }
  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.setAttribute("readonly", "");
  textarea.style.position = "fixed";
  textarea.style.opacity = "0";
  document.body.appendChild(textarea);
  textarea.select();
  const ok = document.execCommand("copy");
  document.body.removeChild(textarea);
  if (!ok) throw new Error("copy failed");
}

function buildAppCapabilityFacts(
  manifest: AppManifest | null,
  serverPort: number | null,
): AppCapabilityFact[] {
  if (!manifest) return [];

  const runtime =
    manifest.runtime === "server"
      ? "server"
      : manifest.runtime === "external"
        ? "external"
        : "static";
  const permissions = manifest.permissions ?? [];
  const allowedHosts = manifest.network?.allowed_hosts ?? [];
  const actions = manifest.actions ?? [];
  const schedules = manifest.schedules ?? [];
  const widgets = manifest.widgets ?? [];
  const pendingRequests = (manifest.permission_requests ?? []).filter(
    (request) => (request.status ?? "pending") === "pending",
  );
  const enabledSchedules = schedules.filter((s) => s.enabled !== false).length;
  const serverCommand = manifest.server?.command?.join(" ");
  const integrationProvider = manifest.integration?.provider?.trim();
  const externalUrl = manifest.external?.url?.trim();
  const integrationCapabilities = manifest.integration?.capabilities ?? [];

  return [
    {
      key: "runtime",
      label: "runtime",
      value:
        runtime === "server" && serverPort ? `server :${serverPort}` : runtime,
      title:
        runtime === "server"
          ? serverCommand
            ? `server command: ${serverCommand}`
            : "server runtime"
          : runtime === "external"
            ? externalUrl || "external web app"
            : `entry: ${manifest.entry}`,
    },
    ...(runtime === "external" || integrationProvider
      ? [
          {
            key: "integration",
            label: "integration",
            value: integrationProvider || "external",
            title:
              integrationCapabilities.length > 0
                ? integrationCapabilities.join(", ")
                : manifest.integration?.notes || "connected app profile",
          },
        ]
      : []),
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
      key: "permission_requests",
      label: "requests",
      value:
        pendingRequests.length === 0
          ? "none"
          : `${pendingRequests.length} pending`,
      title: pendingRequests.length
        ? pendingRequests
            .map((request) => request.reason || request.id)
            .join(", ")
        : "no pending permission requests",
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

function connectedAppMcpConfigured(manifest: AppManifest): boolean {
  const mcp = manifest.integration?.mcp;
  return (
    (isJsonObject(mcp) && (mcp.configured === true || !!mcp.saved_at_ms)) ||
    (manifest.integration?.capabilities ?? []).includes("mcp.configured")
  );
}

function connectedAppMcpChecked(manifest: AppManifest): boolean {
  const mcp = manifest.integration?.mcp;
  return isJsonObject(mcp) && !!mcp.last_query_at_ms;
}

function connectedAppLearned(manifest: AppManifest): boolean {
  const dataModel = manifest.integration?.data_model;
  return isJsonObject(dataModel) && !!dataModel.learned_profile;
}

function connectedAppServiceUrl(manifest: AppManifest): string {
  return (
    manifest.external?.url?.trim() ||
    manifest.external?.open_url?.trim() ||
    manifest.integration?.provider ||
    manifest.id
  );
}

function connectedAppPublicActionCount(manifest: AppManifest): number {
  return (manifest.actions ?? []).filter((action) => action.public).length;
}

function AppCapabilityDetails({ manifest }: { manifest: AppManifest | null }) {
  const { t } = useI18n();
  const permissions = manifest?.permissions ?? [];
  const allowedHosts = manifest?.network?.allowed_hosts ?? [];
  if (permissions.length === 0 && allowedHosts.length === 0) return null;

  return (
    <div
      className="appviewer-capability-details"
      aria-label={t("appViewer.manifestPermissions")}
    >
      {permissions.length > 0 && (
        <section className="appviewer-capability-detail-group">
          <div className="appviewer-capability-detail-title">
            {t("appViewer.permissions")}
          </div>
          <div className="appviewer-capability-chip-list">
            {permissions.map((permission) => (
              <code key={permission}>{permission}</code>
            ))}
          </div>
        </section>
      )}
      {allowedHosts.length > 0 && (
        <section className="appviewer-capability-detail-group">
          <div className="appviewer-capability-detail-title">
            {t("appViewer.networkHosts")}
          </div>
          <div className="appviewer-capability-chip-list">
            {allowedHosts.map((host) => (
              <code key={host}>{host}</code>
            ))}
          </div>
        </section>
      )}
    </div>
  );
}

function AppPermissionRequestsPanel({
  requests,
  resolvingId,
  onResolve,
}: {
  requests: AppPermissionRequest[];
  resolvingId: string | null;
  onResolve: (requestId: string, approve: boolean) => void;
}) {
  const { t } = useI18n();
  const pending = requests.filter(
    (request) => (request.status ?? "pending") === "pending",
  );
  if (pending.length === 0) return null;

  return (
    <div className="appviewer-permission-requests">
      <header>
        <strong>{t("appViewer.permissionRequestsTitle")}</strong>
        <span>{t("appViewer.permissionRequestsHint")}</span>
      </header>
      {pending.map((request) => {
        const permissions = request.permissions ?? [];
        const hosts = request.network_hosts ?? [];
        return (
          <section key={request.id} className="appviewer-permission-request">
            <div className="appviewer-permission-request-main">
              <div className="appviewer-permission-request-reason">
                {request.reason || t("appViewer.permissionRequestNoReason")}
              </div>
              <div className="appviewer-permission-request-chips">
                {permissions.map((permission) => (
                  <code key={`p:${permission}`}>{permission}</code>
                ))}
                {hosts.map((host) => (
                  <code key={`h:${host}`}>host:{host}</code>
                ))}
                {request.server_listen && (
                  <code>{t("appViewer.permissionRequestServerListen")}</code>
                )}
              </div>
            </div>
            <div className="appviewer-banner-actions">
              <button
                className="appviewer-btn appviewer-btn-primary"
                onClick={() => onResolve(request.id, true)}
                disabled={resolvingId === request.id}
              >
                {resolvingId === request.id
                  ? "..."
                  : t("appViewer.approvePermission")}
              </button>
              <button
                className="appviewer-btn"
                onClick={() => onResolve(request.id, false)}
                disabled={resolvingId === request.id}
              >
                {t("appViewer.denyPermission")}
              </button>
            </div>
          </section>
        );
      })}
    </div>
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
  const containerRef = useRef<HTMLDivElement>(null);
  const { t } = useI18n();

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

  // Navigate within the requested pane. If route already lives in another pane, just focus it.
  const navigateRoute = (r: Route, preferredPaneId?: PaneId) => {
    const k = routeKey(r);
    setLayout((prev) => {
      const preferred =
        prev.panes.find((p) => p.id === (preferredPaneId ?? prev.focusedPaneId)) ??
        prev.panes[0];
      if (preferred?.tabs.some((t) => routeKey(t) === k)) {
        return {
          ...prev,
          panes: prev.panes.map((p) =>
            p.id === preferred.id
              ? {
                  ...p,
                  activeKey: k,
                  tabs: p.tabs.map((t) => (routeKey(t) === k ? r : t)),
                }
              : p,
          ),
          focusedPaneId: preferred.id,
        };
      }
      const other = prev.panes.find((p) => p.tabs.some((t) => routeKey(t) === k));
      if (other) {
        return {
          ...prev,
          panes: prev.panes.map((p) =>
            p.id === other.id
              ? {
                  ...p,
                  activeKey: k,
                  tabs: p.tabs.map((t) => (routeKey(t) === k ? r : t)),
                }
              : p,
          ),
          focusedPaneId: other.id,
        };
      }
      if (!preferred) return prev;
      return {
        ...prev,
        panes: prev.panes.map((p) =>
          p.id === preferred.id
            ? { ...p, tabs: [...p.tabs, r], activeKey: k }
            : p,
        ),
        focusedPaneId: preferred.id,
      };
    });
  };

  const navigate = (r: Route) => navigateRoute(r);

  const openProjectRoute = (projectId: string) => {
    navigate({ kind: "project", project_id: projectId });
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
        title: t("project.pickFolderTitle"),
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

  const openInSidePane = (url: string, projectId?: string) => {
    const browserRoute: Route = { kind: "browser", project_id: projectId };
    const browserKey = routeKey(browserRoute);
    setLayout((prev) => {
      const browserPane = prev.panes.find((p) =>
        p.tabs.some((t) => routeKey(t) === browserKey),
      );
      if (browserPane) {
        return {
          ...prev,
          panes: prev.panes.map((p) =>
            p.id === browserPane.id
              ? { ...p, activeKey: browserKey }
              : p,
          ),
          focusedPaneId: browserPane.id,
        };
      }
      const reusablePane = prev.panes.find((p) =>
        p.tabs.some((t) => t.kind === "browser"),
      );
      if (reusablePane) {
        return {
          ...prev,
          panes: prev.panes.map((p) =>
            p.id === reusablePane.id
              ? {
                  ...p,
                  tabs: [...p.tabs, browserRoute],
                  activeKey: browserKey,
                }
              : p,
          ),
          focusedPaneId: reusablePane.id,
        };
      }
      const id = nextPaneId();
      return {
        panes: [
          ...prev.panes,
          { id, tabs: [browserRoute], activeKey: browserKey },
        ],
        paneSizes: { ...prev.paneSizes, [id]: 1 },
        focusedPaneId: id,
      };
    });
    void invoke("browser_tab_open", { url }).catch((e) =>
      console.error("[reflex] browser_tab_open from link failed", e),
    );
  };

  const createNewTopic = async (
    projectId: string,
    prompt: string,
    planMode: boolean,
    options?: {
      source?: string;
      browserTabs?: BrowserTabSnapshot[];
      imagePaths?: string[];
      goal?: string | null;
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
      imagePaths: options?.imagePaths,
      goal: options?.goal,
    });
    // backend emits reflex://thread-created which our listener will route into the focused pane.
  };

  const focusedPane =
    layout.panes.find((p) => p.id === layout.focusedPaneId) ?? layout.panes[0];
  const currentRoute: Route =
    focusedPane.tabs.find((r) => routeKey(r) === focusedPane.activeKey) ??
    focusedPane.tabs[0] ?? { kind: "home" };
  const activeRouteProjectId =
    projectIdFromRoute(currentRoute, threads) ?? null;

  useEffect(() => {
    void invoke("set_active_project", {
      projectId: activeRouteProjectId,
    }).catch((e) => console.warn("[reflex] set_active_project failed", e));
  }, [activeRouteProjectId]);

  const openLinkFromThread = (
    threadId: string,
    url: string,
    _ev: React.MouseEvent<HTMLAnchorElement>,
  ) => {
    const thread = threads.find((t) => t.id === threadId);
    const routeProjectId = projectIdFromRoute(currentRoute, threads);
    if (!thread || !routeProjectId || thread.project_id !== routeProjectId) {
      window.open(url, "_blank", "noopener,noreferrer");
      return;
    }
    openInSidePane(url, routeProjectId);
  };

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
        goal: e.payload.goal ?? null,
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
                  goal: Object.prototype.hasOwnProperty.call(
                    e.payload,
                    "goal",
                  )
                    ? e.payload.goal ?? null
                    : t.goal,
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
    const appOpen = listen<AppOpenRequestPayload>(
      "reflex://app-open-request",
      (e) => {
        const panelRoute = routeForSystemPanel(e.payload);
        if (panelRoute) {
          navigate(panelRoute);
          return;
        }
        if (e.payload.app_id) {
          navigate({ kind: "app", app_id: e.payload.app_id });
        }
      },
    );
    const projectOpen = listen<ProjectOpenRequestPayload>(
      "reflex://project-open-request",
      (e) => {
        if (e.payload.project_id) {
          openProjectRoute(e.payload.project_id);
        }
      },
    );
    const topicOpen = listen<TopicOpenRequestPayload>(
      "reflex://topic-open-request",
      (e) => {
        if (e.payload.thread_id) {
          navigate({ kind: "topic", thread_id: e.payload.thread_id });
        }
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
      appOpen.then((u) => u());
      projectOpen.then((u) => u());
      topicOpen.then((u) => u());
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

  const renderRoute = (r: Route, paneId: PaneId) => {
    switch (r.kind) {
      case "home":
        return (
          <HomeScreen
            projects={projects}
            threads={threads}
            openAppIds={openAppIds}
            onSelectProject={(id) =>
              navigateRoute({ kind: "project", project_id: id }, paneId)
            }
            onSelectTopic={(id) =>
              navigateRoute({ kind: "topic", thread_id: id }, paneId)
            }
            onSelectApp={(id) =>
              navigateRoute({ kind: "app", app_id: id }, paneId)
            }
            onOpenApps={() => navigateRoute({ kind: "apps" }, paneId)}
            onOpenMemory={() => navigateRoute({ kind: "memory" }, paneId)}
            onCreateTopic={(projectId, prompt, planMode, imagePaths, goal) =>
              createNewTopic(projectId, prompt, planMode, { imagePaths, goal })
            }
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
            onCreateApp={() =>
              navigate({
                kind: "apps",
                initialTemplate: "automation",
                openCreate: true,
                createRequestId: Date.now(),
                project_id: r.project_id,
              })
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
            onOpenApp={(id) => navigate({ kind: "app", app_id: id })}
          />
        );
      case "apps":
        return (
          <AppsScreen
            initialTemplate={r.initialTemplate}
            openCreate={r.openCreate}
            createRequestId={r.createRequestId}
            targetProject={projects.find((p) => p.id === r.project_id)}
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
      case "browser": {
        const browserProjectId = r.project_id ?? null;
        return (
          <BrowserScreen
            key={browserProjectId ?? "_global"}
            projectId={browserProjectId}
            projectName={
              browserProjectId
                ? projects.find((p) => p.id === browserProjectId)?.name ?? null
                : null
            }
            onStartChat={async (prompt, browserTabs) => {
              if (!browserProjectId) return;
              await createNewTopic(browserProjectId, prompt, false, {
                source: "browser",
                browserTabs,
              });
            }}
          />
        );
      }
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
              renderRoute={(r) => renderRoute(r, pane.id)}
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
            <h2 className="modal-title">{t("project.newTitle")}</h2>
            <p className="modal-hint">
              <code>{newProjectPath}</code>
              <br />
              {t("project.newHint")}
            </p>
            <textarea
              className="modal-input"
              placeholder={t("project.descriptionPlaceholder")}
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
                {t("project.skip")}
              </button>
              <button
                className="modal-btn modal-btn-primary"
                disabled={creatingProject || !newProjectDescription.trim()}
                onClick={() => void submitNewProject(true)}
              >
                {creatingProject
                  ? t("project.creating")
                  : t("project.createShortcut")}
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
  onNavigate,
  onAddPane,
  onCreateProject,
}: {
  route: Route;
  threads: Thread[];
  projects: Project[];
  onNavigate: (r: Route) => void;
  onAddPane: () => void;
  onCreateProject: () => void;
}) {
  const { t } = useI18n();
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
    if (route.project_id) {
      const p = projects.find((x) => x.id === route.project_id);
      crumbs.push({
        label: p?.name ?? route.project_id,
        route: { kind: "project", project_id: route.project_id },
      });
    }
    crumbs.push({ label: t("nav.apps"), route: null });
  } else if (route.kind === "app") {
    crumbs.push({ label: t("nav.apps"), route: { kind: "apps" } });
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
    crumbs.push({ label: t("nav.memory"), route: null });
  } else if (route.kind === "automations") {
    crumbs.push({ label: t("nav.automations"), route: null });
  } else if (route.kind === "browser") {
    crumbs.push({ label: t("nav.browser"), route: null });
  } else if (route.kind === "settings") {
    crumbs.push({ label: t("nav.settings"), route: null });
  }

  const openMemoryRoute = () => {
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
  };
  const openBrowserRoute = () => {
    onNavigate({
      kind: "browser",
      project_id: projectIdFromRoute(route, threads),
    });
  };

  return (
    <header className="chat-header">
      <div className="chat-header-top">
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
              {i < crumbs.length - 1 && (
                <span className="chat-crumb-sep">›</span>
              )}
            </span>
          ))}
        </nav>
        <span className="chat-subtitle">
          {threads.length} {t("header.threadLabel")} · {projects.length}{" "}
          {t("header.projectLabel")}
        </span>
      </div>
      <nav className="chat-header-actions" aria-label={t("nav.primary")}>
        <div className="header-action-group">
          <span className="header-action-label">{t("nav.groupStart")}</span>
          <button
            className={`header-tab ${route.kind === "home" ? "active" : ""}`}
            onClick={() => onNavigate({ kind: "home" })}
          >
            {t("nav.home")}
          </button>
          <button
            className="header-tab header-tab-primary"
            onClick={onCreateProject}
            title={t("nav.newProjectTitle")}
          >
            {t("nav.newProject")}
          </button>
        </div>

        <div className="header-action-group">
          <span className="header-action-label">{t("nav.groupTools")}</span>
          <button
            className={`header-tab ${route.kind === "memory" ? "active" : ""}`}
            onClick={openMemoryRoute}
            title={t("nav.memory")}
          >
            {t("nav.memory")}
          </button>
          <button
            className={`header-tab ${route.kind === "apps" || route.kind === "app" ? "active" : ""}`}
            onClick={() => onNavigate({ kind: "apps" })}
          >
            {t("nav.apps")}
          </button>
          <button
            className={`header-tab ${route.kind === "automations" ? "active" : ""}`}
            onClick={() => onNavigate({ kind: "automations" })}
            title={t("nav.automations")}
          >
            {t("nav.automations")}
          </button>
          <button
            className={`header-tab ${route.kind === "browser" ? "active" : ""}`}
            onClick={openBrowserRoute}
            title={t("nav.browser")}
          >
            {t("nav.browser")}
          </button>
        </div>

        <div className="header-action-group header-action-group-compact">
          <span className="header-action-label">{t("nav.groupView")}</span>
          <button
            className="header-tab"
            onClick={onAddPane}
            title={t("nav.newPaneTitle")}
          >
            {t("nav.newPane")}
          </button>
          <button
            className={`header-tab ${route.kind === "settings" ? "active" : ""}`}
            onClick={() => onNavigate({ kind: "settings" })}
            title={t("nav.settings")}
          >
            {t("nav.settings")}
          </button>
        </div>
      </nav>
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
  const { t } = useI18n();
  return (
    <nav className="tabs-row">
      {tabs.map((r) => {
        const k = routeKey(r);
        const active = k === activeKey;
        const label = tabLabel(r, projects, threads, t);
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
              title={t("nav.closeTab")}
              aria-label={t("nav.closeTab")}
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
          title={t("nav.closePane")}
          aria-label={t("nav.closePane")}
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
  nameKey: string;
  descriptionKey: string;
  placeholderKey: string;
  badges: string[];
}[] = [
  {
    id: "blank",
    icon: "📄",
    nameKey: "template.blank.name",
    descriptionKey: "template.blank.description",
    placeholderKey: "template.blank.placeholder",
    badges: ["static", "custom"],
  },
  {
    id: "chat",
    icon: "💬",
    nameKey: "template.chat.name",
    descriptionKey: "template.chat.description",
    placeholderKey: "template.chat.placeholder",
    badges: ["agent.stream", "storage"],
  },
  {
    id: "dashboard",
    icon: "📊",
    nameKey: "template.dashboard.name",
    descriptionKey: "template.dashboard.description",
    placeholderKey: "template.dashboard.placeholder",
    badges: ["agent.task", "cards/table"],
  },
  {
    id: "health-dashboard",
    icon: "🩺",
    nameKey: "template.healthDashboard.name",
    descriptionKey: "template.healthDashboard.description",
    placeholderKey: "template.healthDashboard.placeholder",
    badges: ["scheduler.stats", "memory.stats", "widgets"],
  },
  {
    id: "form",
    icon: "📝",
    nameKey: "template.form.name",
    descriptionKey: "template.form.description",
    placeholderKey: "template.form.placeholder",
    badges: ["form", "agent.task"],
  },
  {
    id: "api-client",
    icon: "🌐",
    nameKey: "template.apiClient.name",
    descriptionKey: "template.apiClient.description",
    placeholderKey: "template.apiClient.placeholder",
    badges: ["net.fetch", "network"],
  },
  {
    id: "connected-app",
    icon: "🔌",
    nameKey: "template.connectedApp.name",
    descriptionKey: "template.connectedApp.description",
    placeholderKey: "template.connectedApp.placeholder",
    badges: ["external", "bridge", "mcp"],
  },
  {
    id: "repo-wrapper",
    icon: "OSS",
    nameKey: "template.repoWrapper.name",
    descriptionKey: "template.repoWrapper.description",
    placeholderKey: "template.repoWrapper.placeholder",
    badges: ["repo", "wrapper", "mcp"],
  },
  {
    id: "automation",
    icon: "⏱",
    nameKey: "template.automation.name",
    descriptionKey: "template.automation.description",
    placeholderKey: "template.automation.placeholder",
    badges: ["schedules", "actions", "widgets"],
  },
  {
    id: "node-server",
    icon: "🚀",
    nameKey: "template.nodeServer.name",
    descriptionKey: "template.nodeServer.description",
    placeholderKey: "template.nodeServer.placeholder",
    badges: ["server", "stdlib"],
  },
];

const SKILL_PRESETS = [
  {
    id: "build-web-apps:frontend-app-builder",
    labelKey: "skill.webApps",
  },
  {
    id: "build-web-apps:react-best-practices",
    labelKey: "React",
  },
  {
    id: "playwright",
    labelKey: "skill.browserQa",
  },
  {
    id: "openai-docs",
    labelKey: "skill.openaiDocs",
  },
  {
    id: "github:gh-fix-ci",
    labelKey: "GitHub CI",
  },
  {
    id: "build-ios-apps:ios-debugger-agent",
    labelKey: "skill.iosDebug",
  },
  {
    id: "build-macos-apps:build-run-debug",
    labelKey: "skill.macosDebug",
  },
  {
    id: "game-studio:game-playtest",
    labelKey: "skill.gameQa",
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

function formatAgo(ms: number, t: Translate): string {
  if (ms < 60_000) return t("apps.justNow");
  const min = Math.floor(ms / 60_000);
  if (min < 60) return t("apps.minutesAgo", { count: min });
  const h = Math.floor(min / 60);
  if (h < 24) return t("apps.hoursAgo", { count: h });
  const d = Math.floor(h / 24);
  return t("apps.daysAgo", { count: d });
}

function AppsScreen({
  initialTemplate,
  openCreate,
  createRequestId,
  targetProject,
  onOpenApp,
  onOpenTopic,
}: {
  initialTemplate?: string;
  openCreate?: boolean;
  createRequestId?: number;
  targetProject?: Project;
  onOpenApp: (id: string) => void;
  onOpenTopic: (id: string) => void;
}) {
  const { t } = useI18n();
  const [items, setItems] = useState<AppManifest[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [importing, setImporting] = useState(false);
  const [installingConnected, setInstallingConnected] = useState(false);
  const [showModal, setShowModal] = useState(false);
  const [step, setStep] = useState<"template" | "describe">("template");
  const [template, setTemplate] = useState<string>("blank");
  const [description, setDescription] = useState("");
  const [connectedUrl, setConnectedUrl] = useState("");
  const [connectedName, setConnectedName] = useState("");
  const [sourceRepoUrl, setSourceRepoUrl] = useState("");
  const [trash, setTrash] = useState<TrashEntry[]>([]);
  const [showTrash, setShowTrash] = useState(false);
  const [busyId, setBusyId] = useState<string | null>(null);
  const selectedTemplate =
    TEMPLATES.find((item) => item.id === template) ?? TEMPLATES[0];
  const connectedApps = useMemo(
    () => items.filter((app) => !!app.integration?.provider),
    [items],
  );
  const connectedMcpReadyCount = connectedApps.filter(
    connectedAppMcpConfigured,
  ).length;

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
        title: t("apps.importTitle"),
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
      const [list, trashed] = await Promise.all([
        invoke<AppManifest[]>("list_apps"),
        invoke<TrashEntry[]>("list_trashed_apps"),
      ]);
      setItems(list);
      setTrash(trashed);
    } catch (e) {
      setError(String(e));
    }
  }

  async function deleteApp(appId: string, appName: string) {
    if (busyId) return;
    if (!window.confirm(t("apps.deleteConfirm", { name: appName }))) return;
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
    if (!window.confirm(t("apps.purgeConfirm", { name }))) return;
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
        .then(([list, trashed]) => {
          if (!alive) return;
          setItems(list);
          setTrash(trashed);
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
    const repoUrl = sourceRepoUrl.trim();
    if (creating) return;
    if (template === "repo-wrapper" && !repoUrl) return;
    if (template !== "repo-wrapper" && !text) return;
    setCreating(true);
    setError(null);
    try {
      const promptText =
        template === "repo-wrapper"
          ? [
              `Open-source repository URL: ${repoUrl}`,
              "",
              "Wrapper requirements from the user:",
              text ||
                "Create a Reflex wrapper around this open-source app, add a bridge layer, and derive an MCP/data adapter plan from the code.",
            ].join("\n")
          : text;
      const res = await invoke<{ app_id: string; thread_id: string }>(
        "create_app",
        {
          description: promptText,
          template,
          projectId: targetProject?.id ?? null,
          sourceRepoUrl: template === "repo-wrapper" ? repoUrl : null,
        },
      );
      setShowModal(false);
      setDescription("");
      setSourceRepoUrl("");
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

  async function installConnected(
    provider: string,
    options?: { url?: string | null; displayName?: string | null },
  ) {
    if (installingConnected) return;
    setInstallingConnected(true);
    setError(null);
    try {
      const manifest = await invoke<AppManifest>("install_connected_app", {
        provider,
        url: options?.url ?? null,
        displayName: options?.displayName ?? null,
        projectId: targetProject?.id ?? null,
      });
      setShowModal(false);
      setDescription("");
      setStep("template");
      setTemplate("blank");
      setConnectedUrl("");
      setConnectedName("");
      setSourceRepoUrl("");
      await refresh();
      onOpenApp(manifest.id);
    } catch (e) {
      console.error("[reflex] install_connected_app failed", e);
      setError(String(e));
    } finally {
      setInstallingConnected(false);
    }
  }

  return (
    <div className="apps-root">
      <header className="apps-header">
        <div className="apps-header-row">
          <h1 className="section-title">{t("nav.apps")}</h1>
          <div className="apps-header-buttons">
            <button
              className="apps-create-btn"
              onClick={() => setShowModal(true)}
            >
              {t("apps.newUtility")}
            </button>
            <button
              className="apps-trash-btn"
              onClick={() => setShowTrash((v) => !v)}
              title={t("apps.deletedAppsTitle")}
            >
              🗑 {t("apps.trash")}{trash.length > 0 ? ` (${trash.length})` : ""}
            </button>
          </div>
        </div>
        <p className="apps-hint">{t("apps.headerHint")}</p>
      </header>
      {error && <div className="apps-error">{error}</div>}
      {connectedApps.length > 0 && (
        <section className="connected-apps-panel">
          <div className="connected-apps-head">
            <div>
              <h2 className="section-title">{t("apps.connectedAdapters")}</h2>
              <p className="apps-hint">
                {t("apps.connectedAdaptersHint")}
              </p>
            </div>
            <span className="connected-apps-summary">
              {t("apps.connectedAdaptersSummary", {
                ready: connectedMcpReadyCount,
                total: connectedApps.length,
              })}
            </span>
          </div>
          <div className="connected-apps-list">
            {connectedApps.map((app) => {
              const mcpReady = connectedAppMcpConfigured(app);
              const mcpChecked = connectedAppMcpChecked(app);
              const learned = connectedAppLearned(app);
              const actionCount = connectedAppPublicActionCount(app);
              const provider =
                app.integration?.display_name ||
                app.integration?.provider ||
                app.name;
              return (
                <button
                  key={app.id}
                  className="connected-app-row"
                  onClick={() => onOpenApp(app.id)}
                  title={t("apps.connectedOpenTitle")}
                >
                  <span className="connected-app-icon">
                    {app.icon ?? "APP"}
                  </span>
                  <span className="connected-app-main">
                    <span className="connected-app-name">{app.name}</span>
                    <span className="connected-app-meta">
                      {provider} · {connectedAppServiceUrl(app)}
                    </span>
                  </span>
                  <span className="connected-app-badges">
                    <span
                      className={`connected-app-badge ${mcpReady ? "ok" : "warn"}`}
                    >
                      {mcpReady
                        ? t("apps.connectedMcpReady")
                        : t("apps.connectedMcpMissing")}
                    </span>
                    <span
                      className={`connected-app-badge ${learned ? "ok" : ""}`}
                    >
                      {learned
                        ? t("apps.connectedLearned")
                        : t("apps.connectedLearningNeeded")}
                    </span>
                    <span
                      className={`connected-app-badge ${mcpChecked ? "ok" : ""}`}
                    >
                      {mcpChecked
                        ? t("apps.connectedMcpChecked")
                        : t("apps.connectedMcpUnchecked")}
                    </span>
                    <span className="connected-app-badge">
                      {t("apps.connectedActions", { count: actionCount })}
                    </span>
                  </span>
                </button>
              );
            })}
          </div>
        </section>
      )}
      {showTrash && (
        <section className="apps-trash">
          <h3 className="apps-trash-title">{t("apps.trashTitle")}</h3>
          {trash.length === 0 ? (
            <div className="apps-trash-empty">{t("apps.trashEmpty")}</div>
          ) : (
            <ul className="apps-trash-list">
              {trash.map((trashEntry) => {
                const ageMs = Date.now() - trashEntry.deleted_at_ms;
                const ageStr = formatAgo(ageMs, t);
                return (
                  <li key={trashEntry.trash_id} className="apps-trash-row">
                    <span className="apps-trash-icon">
                      {trashEntry.original_icon ?? "🧩"}
                    </span>
                    <div className="apps-trash-info">
                      <div className="apps-trash-name">
                        {trashEntry.original_name}
                      </div>
                      <div className="apps-trash-meta">
                        {t("apps.deletedAt", { age: ageStr })} ·{" "}
                        <code>{trashEntry.original_id}</code>
                      </div>
                    </div>
                    <div className="apps-trash-actions">
                      <button
                        className="apps-trash-action"
                        disabled={busyId === trashEntry.trash_id}
                        onClick={() => void restoreApp(trashEntry.trash_id)}
                        title={t("apps.restore")}
                      >
                        ↩ {t("apps.restore")}
                      </button>
                      <button
                        className="apps-trash-action apps-trash-purge"
                        disabled={busyId === trashEntry.trash_id}
                        onClick={() =>
                          void purgeApp(
                            trashEntry.trash_id,
                            trashEntry.original_name,
                          )
                        }
                        title={t("apps.deleteForever")}
                      >
                        ✕ {t("apps.deleteForever")}
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
          <p>{t("apps.empty")}</p>
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
                title={
                  isReady ? t("apps.open") : t("apps.writingFiles")
                }
              >
                <button
                  className="apps-card-delete"
                  onClick={(ev) => {
                    ev.stopPropagation();
                    void deleteApp(a.id, a.name);
                  }}
                  disabled={busyId === a.id}
                  title={t("apps.moveToTrash")}
                >
                  ✕
                </button>
                <div className="apps-card-icon">{a.icon ?? "🧩"}</div>
                <div className="apps-card-name">
                  {a.name}
                  {!isReady && (
                    <span className="apps-card-badge">
                      {t("apps.creatingBadge")}
                    </span>
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
                <h2 className="modal-title">{t("apps.newUtilityTitle")}</h2>
                <p className="modal-hint">{t("apps.chooseTemplate")}</p>
                {targetProject && (
                  <div className="modal-context-chip">
                    {t("apps.linkedToProject", { name: targetProject.name })}
                  </div>
                )}
                <div className="template-grid">
                  {TEMPLATES.map((templateItem) => (
                    <button
                      key={templateItem.id}
                      className={`template-card ${template === templateItem.id ? "active" : ""}`}
                      onClick={() => setTemplate(templateItem.id)}
                    >
                      <div className="template-icon">{templateItem.icon}</div>
                      <div className="template-name">
                        {t(templateItem.nameKey)}
                      </div>
                      <div className="template-desc">
                        {t(templateItem.descriptionKey)}
                      </div>
                      <div className="template-badges">
                        {templateItem.badges.map((badge) => (
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
                    {t("apps.cancel")}
                  </button>
                  <button
                    className="modal-btn"
                    onClick={() => void importBundle()}
                    disabled={importing}
                    title={t("apps.importBundleTitle")}
                  >
                    {importing ? "..." : `📥 ${t("apps.importBundle")}`}
                  </button>
                  <button
                    className="modal-btn modal-btn-primary"
                    onClick={() => setStep("describe")}
                  >
                    {t("apps.next")}
                  </button>
                </div>
              </>
            ) : (
              <>
                <h2 className="modal-title">
                  {selectedTemplate.icon} {t(selectedTemplate.nameKey)}
                </h2>
                <p className="modal-hint">{t("apps.describeHint")}</p>
                {targetProject && (
                  <div className="modal-context-chip">
                    {t("apps.linkedToProject", { name: targetProject.name })}
                  </div>
                )}
                {template === "connected-app" && (
                  <div className="connected-install-stack">
                    <div className="connected-custom-panel">
                      <strong>{t("apps.installCustomTitle")}</strong>
                      <div className="connected-custom-grid">
                        <input
                          className="modal-input"
                          value={connectedName}
                          onChange={(e) =>
                            setConnectedName(e.currentTarget.value)
                          }
                          placeholder={t("apps.installCustomNamePlaceholder")}
                        />
                        <input
                          className="modal-input"
                          value={connectedUrl}
                          onChange={(e) =>
                            setConnectedUrl(e.currentTarget.value)
                          }
                          placeholder={t("apps.installCustomUrlPlaceholder")}
                        />
                      </div>
                      <button
                        className="modal-btn"
                        disabled={
                          creating ||
                          installingConnected ||
                          !connectedUrl.trim()
                        }
                        onClick={() =>
                          void installConnected("generic_web", {
                            url: connectedUrl.trim(),
                            displayName: connectedName.trim() || null,
                          })
                        }
                      >
                        {installingConnected
                          ? t("apps.installing")
                          : t("apps.installCustom")}
                      </button>
                    </div>
                  </div>
                )}
                {template === "repo-wrapper" && (
                  <div className="connected-custom-panel">
                    <strong>{t("apps.repoWrapperTitle")}</strong>
                    <span>{t("apps.repoWrapperHint")}</span>
                    <input
                      className="modal-input"
                      value={sourceRepoUrl}
                      onChange={(e) =>
                        setSourceRepoUrl(e.currentTarget.value)
                      }
                      placeholder={t("apps.repoWrapperUrlPlaceholder")}
                      autoFocus
                    />
                  </div>
                )}
                <textarea
                  className="modal-input"
                  placeholder={t(selectedTemplate.placeholderKey)}
                  value={description}
                  onChange={(e) => setDescription(e.currentTarget.value)}
                  autoFocus={template !== "repo-wrapper"}
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
                    disabled={creating || installingConnected}
                    onClick={() => setStep("template")}
                  >
                    {t("apps.back")}
                  </button>
                  <button
                    className="modal-btn"
                    disabled={creating || installingConnected}
                    onClick={() => setShowModal(false)}
                  >
                    {t("apps.cancel")}
                  </button>
                  <button
                    className="modal-btn modal-btn-primary"
                    disabled={
                      creating ||
                      installingConnected ||
                      (template === "repo-wrapper"
                        ? !sourceRepoUrl.trim()
                        : !description.trim())
                    }
                    onClick={() => void submitCreate()}
                  >
                    {creating ? t("apps.creating") : t("apps.createShortcut")}
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
  const { t } = useI18n();
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
  const [commitOpen, setCommitOpen] = useState(false);
  const [commitDraft, setCommitDraft] = useState("revision");
  const [exporting, setExporting] = useState(false);
  const [bridgeOpen, setBridgeOpen] = useState(false);
  const [bridgeQuery, setBridgeQuery] = useState("");
  const [copiedBridgeItem, setCopiedBridgeItem] = useState<string | null>(null);
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
  const [resolvingPermissionRequest, setResolvingPermissionRequest] = useState<
    string | null
  >(null);

  const isServerRuntime = manifest?.runtime === "server";
  const isExternalRuntime = manifest?.runtime === "external";
  const permissionRequests = manifest?.permission_requests ?? [];
  isServerRuntimeRef.current = isServerRuntime;
  const normalizedBridgeQuery = bridgeQuery.trim().toLowerCase();
  const visibleBridgeApiGroups = useMemo(() => {
    if (!normalizedBridgeQuery) return BRIDGE_API_GROUPS;
    return BRIDGE_API_GROUPS.map((group) => ({
      ...group,
      methods: group.methods.filter((method) =>
        method.toLowerCase().includes(normalizedBridgeQuery),
      ),
    })).filter((group) => group.methods.length > 0);
  }, [normalizedBridgeQuery]);
  const visibleBridgeHelperGroups = useMemo(() => {
    if (!normalizedBridgeQuery) return BRIDGE_HELPER_GROUPS;
    return BRIDGE_HELPER_GROUPS.map((group) => ({
      ...group,
      helpers: group.helpers.filter((helper) =>
        helper.toLowerCase().includes(normalizedBridgeQuery),
      ),
    })).filter((group) => group.helpers.length > 0);
  }, [normalizedBridgeQuery]);
  const visibleBridgeRecipes = useMemo(() => {
    if (!normalizedBridgeQuery) return BRIDGE_RECIPE_CARDS;
    return BRIDGE_RECIPE_CARDS.filter((recipe) => {
      const haystack = [
        recipe.title,
        recipe.body,
        bridgeRecipeTitle(recipe, t),
        bridgeRecipeBody(recipe, t),
        recipe.example,
        ...recipe.calls,
      ]
        .join(" ")
        .toLowerCase();
      return haystack.includes(normalizedBridgeQuery);
    });
  }, [normalizedBridgeQuery, t]);
  const visibleBridgeApiCount = visibleBridgeApiGroups.reduce(
    (sum, group) => sum + group.methods.length,
    0,
  );
  const visibleBridgeHelperCount = visibleBridgeHelperGroups.reduce(
    (sum, group) => sum + group.helpers.length,
    0,
  );
  const hasBridgeMatches =
    visibleBridgeApiGroups.length > 0 ||
    visibleBridgeHelperGroups.length > 0 ||
    visibleBridgeRecipes.length > 0;

  async function copyBridgeItem(text: string) {
    try {
      await copyTextToClipboard(text);
      setCopiedBridgeItem(text);
      window.setTimeout(() => {
        setCopiedBridgeItem((current) => (current === text ? null : current));
      }, 1200);
    } catch (e) {
      console.warn("[reflex] bridge copy failed", e);
    }
  }

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
    const summary = `Improve the element matching selector \`${pick.selector || pick.tagName}\`.\n\nContext:\n\`\`\`html\n${pick.outerHTML}\n\`\`\`\n\nRequested change, verbatim and possibly non-English:\n${text}`;
    await dispatchRevise(summary);
    setPick(null);
    setPickInstruction("");
  };

  const submitErrorFix = async () => {
    if (!lastError) return;
    const summary = `The app crashed with an error:\n\nMessage: ${lastError.message}\nLocation: ${lastError.filename}:${lastError.lineno}:${lastError.colno}\nStack:\n\`\`\`\n${lastError.stack || "(no stack)"}\n\`\`\`\n\nFix this bug.`;
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

  useEffect(() => {
    let alive = true;
    const refresh = () => {
      void invoke<AppManifest>("read_app_manifest", { appId })
        .then((m) => {
          if (alive) setManifest(m);
        })
        .catch((e) => {
          if (alive) setError(String(e));
        });
    };
    const requested = listen<{ app_id: string }>(
      "reflex://app-permission-requested",
      (event) => {
        if (event.payload.app_id === appId) refresh();
      },
    );
    const resolved = listen<{ app_id: string }>(
      "reflex://app-permission-resolved",
      (event) => {
        if (event.payload.app_id === appId) refresh();
      },
    );
    return () => {
      alive = false;
      requested.then((unlisten) => unlisten());
      resolved.then((unlisten) => unlisten());
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

  async function resolvePermissionRequest(requestId: string, approve: boolean) {
    if (resolvingPermissionRequest) return;
    setResolvingPermissionRequest(requestId);
    setError(null);
    try {
      const updated = await invoke<AppManifest>("resolve_app_permission_request", {
        appId,
        requestId,
        approve,
        note: approve ? "Approved from app viewer" : "Denied from app viewer",
      });
      setManifest(updated);
    } catch (e) {
      setError(String(e));
    } finally {
      setResolvingPermissionRequest(null);
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
        title: t("appViewer.exportDialogTitle"),
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
    const message = commitDraft.trim() || "revision";
    setBusy("save");
    setError(null);
    try {
      await invoke("app_save", { appId, message });
      setCommitOpen(false);
      setCommitDraft("revision");
      await refreshStatus();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  }

  async function revert() {
    if (busy) return;
    if (!window.confirm(t("appViewer.revertConfirm"))) return;
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
  const externalUrl = manifest?.external?.url?.trim() || null;
  const src = isExternalRuntime
    ? externalUrl
    : isServerRuntime
      ? serverPort
        ? `reflexserver://${encodeURIComponent(appId)}/`
        : null
      : `reflexapp://localhost/${encodeURIComponent(appId)}/${entry}`;
  const sandbox = isExternalRuntime
    ? "allow-scripts allow-forms allow-same-origin allow-popups allow-downloads"
    : isServerRuntime
      ? "allow-scripts allow-forms allow-same-origin"
      : "allow-scripts allow-forms allow-same-origin";
  const manifestFacts = useMemo(
    () => buildAppCapabilityFacts(manifest, serverPort),
    [manifest, serverPort],
  );

  useEffect(() => {
    if (!status?.has_changes) setCommitOpen(false);
  }, [status?.has_changes]);

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
              title={t("appViewer.restartServerTitle")}
            >
              {busy === "restart" ? "..." : `↻ ${t("appViewer.restartServer")}`}
            </button>
          )}
          {isServerRuntime && (
            <button
              className="appviewer-btn"
              onClick={() => setLogsOpen((v) => !v)}
              title={t("appViewer.serverLogsTitle")}
            >
              {logsOpen
                ? `▾ ${t("appViewer.logs")}`
                : `▸ ${t("appViewer.logs")}`}
            </button>
          )}
          <button
            className={`appviewer-btn ${bridgeOpen ? "appviewer-btn-primary" : ""}`}
            onClick={() => setBridgeOpen((v) => !v)}
            title={t("appViewer.runtimeHelpersTitle")}
          >
            {bridgeOpen ? "▾ Bridge" : "▸ Bridge"}
          </button>
          {isExternalRuntime && externalUrl && (
            <button
              className="appviewer-btn"
              onClick={() =>
                window.open(
                  manifest?.external?.open_url ?? externalUrl,
                  "_blank",
                  "noopener,noreferrer",
                )
              }
              title={t("appViewer.openExternalTitle")}
            >
              ↗ {t("appViewer.openExternal")}
            </button>
          )}
          {!isServerRuntime && !isExternalRuntime && (
            <button
              className={`appviewer-btn ${inspecting ? "appviewer-btn-primary" : ""}`}
              onClick={toggleInspecting}
              disabled={busy !== null || reviseBusy}
              title={t("appViewer.inspectorTitle")}
            >
              {inspecting
                ? `✕ ${t("appViewer.inspector")}`
                : `🎯 ${t("appViewer.inspector")}`}
            </button>
          )}
          <button
            className="appviewer-btn"
            onClick={() => void handleEditClick()}
            disabled={busy !== null || openingNested !== null}
            title={t("appViewer.editExistingThreadTitle")}
          >
            {openingNested === "edit" ? "..." : `✏️ ${t("appViewer.edit")}`}
          </button>
          <button
            className="appviewer-btn"
            onClick={() => void handleNewThreadClick()}
            disabled={busy !== null || openingNested !== null}
            title={t("appViewer.newThreadTitle")}
          >
            {openingNested === "new"
              ? "..."
              : `🆕 ${t("appViewer.newThread")}`}
          </button>
          <button
            className="appviewer-btn"
            onClick={() => void exportApp()}
            disabled={busy !== null || exporting}
            title={t("appViewer.exportTitle")}
          >
            {exporting ? "..." : `📤 ${t("appViewer.export")}`}
          </button>
        </div>
      </header>

      {manifestFacts.length > 0 && (
        <>
          <div
            className="appviewer-capabilities"
            aria-label={t("appViewer.manifestCapabilities")}
          >
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
          <AppCapabilityDetails manifest={manifest} />
        </>
      )}

      {bridgeOpen && (
        <div
          className="appviewer-bridge-panel"
          aria-label={t("appViewer.bridgeCatalog")}
        >
          <div className="appviewer-bridge-head">
            <span>Runtime bridge</span>
            <input
              value={bridgeQuery}
              onChange={(e) => setBridgeQuery(e.currentTarget.value)}
              placeholder={t("appViewer.bridgeSearch")}
            />
            <div className="appviewer-bridge-counts">
              <span>
                {t("appViewer.methodsCount", {
                  visible: visibleBridgeApiCount,
                  total: BRIDGE_API_COUNT,
                })}
              </span>
              <span>
                {t("appViewer.helpersCount", {
                  visible: visibleBridgeHelperCount,
                  total: BRIDGE_HELPER_COUNT,
                })}
              </span>
            </div>
          </div>
          {!hasBridgeMatches ? (
            <div className="appviewer-bridge-empty">
              {t("appViewer.noBridgeMatches")}
            </div>
          ) : (
            <>
              {visibleBridgeRecipes.length > 0 && (
                <div className="appviewer-bridge-recipes">
                  {visibleBridgeRecipes.map((recipe) => (
                    <div className="appviewer-bridge-recipe" key={recipe.title}>
                      <div className="appviewer-bridge-title">
                        {bridgeRecipeTitle(recipe, t)}
                      </div>
                      <p>{bridgeRecipeBody(recipe, t)}</p>
                      <button
                        className={`appviewer-bridge-code-button ${copiedBridgeItem === recipe.example ? "copied" : ""}`}
                        onClick={() => void copyBridgeItem(recipe.example)}
                        title={t("appViewer.copy")}
                      >
                        <code>{recipe.example}</code>
                      </button>
                    </div>
                  ))}
                </div>
              )}
              {visibleBridgeApiGroups.length > 0 && (
                <div className="appviewer-bridge-section">
                  <div className="appviewer-bridge-section-label">
                    {t("appViewer.methods")}
                  </div>
                  <div className="appviewer-bridge-grid">
                    {visibleBridgeApiGroups.map((group) => (
                      <div className="appviewer-bridge-group" key={group.title}>
                        <div className="appviewer-bridge-title">
                          {bridgeCatalogTitle(group.title, t)}
                        </div>
                        <div className="appviewer-bridge-list">
                          {group.methods.map((method) => (
                            <button
                              key={method}
                              className={`appviewer-bridge-chip ${copiedBridgeItem === method ? "copied" : ""}`}
                              onClick={() => void copyBridgeItem(method)}
                              title={t("appViewer.copy")}
                            >
                              <code>{method}</code>
                            </button>
                          ))}
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              )}
              {visibleBridgeHelperGroups.length > 0 && (
                <div className="appviewer-bridge-section">
                  <div className="appviewer-bridge-section-label">
                    {t("appViewer.helpers")}
                  </div>
                  <div className="appviewer-bridge-grid">
                    {visibleBridgeHelperGroups.map((group) => (
                      <div className="appviewer-bridge-group" key={group.title}>
                        <div className="appviewer-bridge-title">
                          {bridgeCatalogTitle(group.title, t)}
                        </div>
                        <div className="appviewer-bridge-list">
                          {group.helpers.map((helper) => (
                            <button
                              key={helper}
                              className={`appviewer-bridge-chip ${copiedBridgeItem === helper ? "copied" : ""}`}
                              onClick={() => void copyBridgeItem(helper)}
                              title={t("appViewer.copy")}
                            >
                              <code>{helper}</code>
                            </button>
                          ))}
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              )}
            </>
          )}
        </div>
      )}

      {(manifest?.actions?.length ?? 0) > 0 && (
        <div
          className="appviewer-action-strip"
          aria-label={t("appViewer.actions")}
        >
          <div className="appviewer-action-strip-title">
            {t("appViewer.actions")}
          </div>
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
                  <span className="appviewer-action-public">
                    {t("appViewer.public")}
                  </span>
                )}
                {!!actionParamsSchema(action) && (
                  <span className="appviewer-action-public">
                    {t("appViewer.params")}
                  </span>
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
        <div
          className="appviewer-action-editor"
          aria-label={t("appViewer.actionParamsEditor")}
        >
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
              {t("apps.cancel")}
            </button>
            <button
              className="appviewer-btn appviewer-btn-primary"
              onClick={submitActionDraft}
              disabled={actionBusy !== null}
            >
              {t("appViewer.run")}
            </button>
          </div>
        </div>
      )}

      {status?.has_changes && (
        <div className="appviewer-banner appviewer-banner-warn">
          <span>{t("appViewer.unsavedChanges")}</span>
          <div className="appviewer-banner-actions">
            <button
              className="appviewer-btn"
              onClick={() => setShowDiff(true)}
              disabled={busy !== null}
              title={t("appViewer.diffTitle")}
            >
              🔍 Diff
            </button>
            <button
              className="appviewer-btn appviewer-btn-primary"
              onClick={() => {
                if (commitOpen) void save();
                else setCommitOpen(true);
              }}
              disabled={busy !== null}
            >
              {busy === "save"
                ? "..."
                : commitOpen
                  ? t("appViewer.commit")
                  : t("appViewer.save")}
            </button>
            <button
              className="appviewer-btn appviewer-btn-danger"
              onClick={() => void revert()}
              disabled={busy !== null}
            >
              {t("appViewer.revert")}
            </button>
            <button
              className="appviewer-btn"
              onClick={() => setReloadKey((k) => k + 1)}
              disabled={busy !== null}
            >
              {t("appViewer.reload")}
            </button>
          </div>
        </div>
      )}

      {status?.has_changes && commitOpen && (
        <div
          className="appviewer-commit-editor"
          aria-label={t("appViewer.saveRevision")}
        >
          <input
            className="appviewer-commit-input"
            value={commitDraft}
            onChange={(e) => setCommitDraft(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                void save();
              }
              if (e.key === "Escape") {
                e.preventDefault();
                setCommitOpen(false);
              }
            }}
            autoFocus
          />
          <div className="appviewer-commit-actions">
            <button
              className="appviewer-btn"
              onClick={() => setCommitOpen(false)}
              disabled={busy !== null}
            >
              {t("apps.cancel")}
            </button>
            <button
              className="appviewer-btn appviewer-btn-primary"
              onClick={() => void save()}
              disabled={busy !== null}
            >
              {busy === "save" ? "..." : t("appViewer.commit")}
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
            <strong>{t("appViewer.appCrashed")}</strong> {lastError.message}
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
              title={t("appViewer.errorFixTitle")}
            >
              {reviseBusy ? "..." : "✨ Fix"}
            </button>
            <button
              className="appviewer-btn"
              onClick={() => setLastError(null)}
              disabled={reviseBusy}
            >
              {t("appViewer.dismiss")}
            </button>
          </div>
        </div>
      )}

      {pick && (
        <div className="inspector-card">
          <header className="inspector-card-header">
            <span className="inspector-card-tag">
              🎯 {t("appViewer.selected")}
            </span>
            <code className="inspector-card-selector">
              {pick.selector || pick.tagName}
            </code>
            <button
              className="inspector-card-close"
              onClick={() => setPick(null)}
              aria-label={t("appViewer.close")}
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
            placeholder={t("appViewer.inspectorPlaceholder")}
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
              {reviseBusy ? t("appViewer.applying") : "Apply (⌘↵)"}
            </button>
          </div>
        </div>
      )}

      {error && <div className="apps-error">{error}</div>}

      <AppPermissionRequestsPanel
        requests={permissionRequests}
        resolvingId={resolvingPermissionRequest}
        onResolve={(requestId, approve) =>
          void resolvePermissionRequest(requestId, approve)
        }
      />

      {isServerRuntime && serverState !== "running" && (
        <div
          className={`appviewer-banner ${serverState === "failed" || serverState === "crashed" ? "appviewer-banner-warn" : "appviewer-banner-info"}`}
        >
          {serverState === "starting" && (
            <span>{t("appViewer.startingServer")}</span>
          )}
          {serverState === "failed" && (
            <span>
              {t("appViewer.serverStartFailed", {
                error: serverError ?? "",
              })}
            </span>
          )}
          {serverState === "crashed" && (
            <span>
              {t("appViewer.serverCrashed", {
                error: serverError ?? "process exited",
              })}
            </span>
          )}
          {(serverState === "failed" || serverState === "crashed") && (
            <div className="appviewer-banner-actions">
              <button
                className="appviewer-btn"
                onClick={() => void restartServer()}
                disabled={busy !== null}
              >
                {t("appViewer.restartServer")}
              </button>
            </div>
          )}
        </div>
      )}

      {!isServerRuntime && !isExternalRuntime && status && !status.entry_exists ? (
        <div className="appviewer-stuck">
          <h3>{t("appViewer.generationIncomplete")}</h3>
          <p>
            {t("appViewer.generationIncompleteBefore")} <code>{entry}</code>{" "}
            {t("appViewer.generationIncompleteAfter")}
          </p>
          <div className="appviewer-stuck-actions">
            <button
              className="appviewer-btn"
              onClick={() => setReloadKey((n) => n + 1)}
            >
              {t("appViewer.checkAgain")}
            </button>
            <button
              className="appviewer-btn appviewer-btn-danger"
              disabled={busy !== null}
              onClick={async () => {
                if (
                  !window.confirm(
                    t("apps.deleteConfirm", {
                      name: manifest?.name ?? appId,
                    }),
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
              {t("apps.moveToTrash")}
            </button>
          </div>
        </div>
      ) : src ? (
        <iframe
          ref={iframeRef}
          key={`${reloadKey}:${serverPort ?? externalUrl ?? "static"}`}
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
              {t("appViewer.serverLogs")}
              {serverPort != null && (
                <span className="server-logs-port"> · :{serverPort}</span>
              )}
            </span>
            <button
              className="server-logs-clear"
              onClick={() => setLogs([])}
              title={t("appViewer.clearLogsTitle")}
            >
              {t("appViewer.clear")}
            </button>
          </div>
          <div className="server-logs-body">
            {logs.length === 0 ? (
              <div className="server-logs-empty">{t("appViewer.empty")}</div>
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
                const thread = threads.find((x) => x.id === tid);
                const label =
                  thread?.title ?? thread?.prompt?.slice(0, 32) ?? tid;
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
                      aria-label={t("appViewer.close")}
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
                  <p>{t("appViewer.selectThread")}</p>
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
  onOpenMemory,
  onCreateTopic,
  onCreateProject,
}: {
  projects: Project[];
  threads: Thread[];
  openAppIds: Set<string>;
  onSelectProject: (id: string) => void;
  onSelectTopic: (id: string) => void;
  onSelectApp: (id: string) => void;
  onOpenApps: () => void;
  onOpenMemory: () => void;
  onCreateTopic: (
    projectId: string,
    prompt: string,
    planMode: boolean,
    imagePaths?: string[],
    goal?: string | null,
  ) => Promise<void>;
  onCreateProject: () => void;
}) {
  const { t } = useI18n();
  const projectsRef = useRef<HTMLElement>(null);
  const [showStartDialog, setShowStartDialog] = useState(false);
  const [dialogProjectId, setDialogProjectId] = useState("");
  const [dialogSubmitting, setDialogSubmitting] = useState(false);
  const [dialogError, setDialogError] = useState<string | null>(null);
  const [dialogApps, setDialogApps] = useState<AppManifest[]>([]);
  const hasProjects = projects.length > 0;
  const dialogProject =
    projects.find((project) => project.id === dialogProjectId) ?? null;
  const chooseProject = () => {
    if (!hasProjects) {
      onCreateProject();
      return;
    }
    projectsRef.current?.scrollIntoView({ block: "start", behavior: "smooth" });
  };
  const openStartDialog = () => {
    if (!hasProjects) {
      onCreateProject();
      return;
    }
    setDialogError(null);
    setShowStartDialog(true);
  };

  const submitStartDialog = async (
    prompt: string,
    imagePaths: string[],
    meta?: TopicComposerSendMeta,
  ) => {
    if (!prompt.trim() || !dialogProjectId || dialogSubmitting) return;
    setDialogSubmitting(true);
    setDialogError(null);
    try {
      await onCreateTopic(
        dialogProjectId,
        prompt,
        meta?.planMode ?? false,
        imagePaths,
        meta?.goal ?? null,
      );
      setShowStartDialog(false);
    } catch (e) {
      setDialogError(String(e));
    } finally {
      setDialogSubmitting(false);
    }
  };
  const recent = threads
    .slice()
    .sort((a, b) => b.created_at_ms - a.created_at_ms)
    .slice(0, 5);

  useEffect(() => {
    if (projects.length === 0) {
      if (dialogProjectId) setDialogProjectId("");
      return;
    }
    if (!projects.some((project) => project.id === dialogProjectId)) {
      setDialogProjectId("");
    }
  }, [projects, dialogProjectId]);

  useEffect(() => {
    let alive = true;

    const refreshApps = () => {
      invoke<AppManifest[]>("list_apps")
        .then((list) => {
          if (alive) setDialogApps(list);
        })
        .catch((e) =>
          console.warn("[reflex] list_apps for home composer failed", e),
        );
    };

    refreshApps();
    let unlisten: (() => void) | null = null;
    listen("reflex://apps-changed", refreshApps)
      .then((u) => {
        unlisten = u;
      })
      .catch((e) => console.warn("[reflex] listen apps-changed home", e));
    return () => {
      alive = false;
      unlisten?.();
    };
  }, []);

  return (
    <div className="home-root">
      <section className="home-start">
        <div className="home-start-copy">
          <p className="home-eyebrow">{t("home.startEyebrow")}</p>
          <h1>{t("home.startTitle")}</h1>
          <p>{t("home.startBody")}</p>
        </div>
        <div className="home-start-actions">
          <button
            className="home-primary-action"
            onClick={chooseProject}
          >
            {hasProjects
              ? t("home.chooseProject")
              : t("home.createFirstProject")}
          </button>
          <button className="home-secondary-action" onClick={onOpenApps}>
            {t("home.openUtilities")}
          </button>
        </div>
        <div className="home-guide-grid">
          <button
            className="home-guide-card"
            onClick={openStartDialog}
          >
            <span className="home-guide-icon">💬</span>
            <span className="home-guide-title">{t("home.askInProject")}</span>
            <span className="home-guide-hint">
              {t("home.askInProjectHint")}
            </span>
          </button>
          <button className="home-guide-card" onClick={onOpenApps}>
            <span className="home-guide-icon">🧩</span>
            <span className="home-guide-title">{t("home.buildUtility")}</span>
            <span className="home-guide-hint">
              {t("home.buildUtilityHint")}
            </span>
          </button>
          <button className="home-guide-card" onClick={onOpenMemory}>
            <span className="home-guide-icon">M</span>
            <span className="home-guide-title">{t("home.reviewMemory")}</span>
            <span className="home-guide-hint">
              {t("home.reviewMemoryHint")}
            </span>
          </button>
        </div>
      </section>
      <HomeAppsSection
        openAppIds={openAppIds}
        onSelectApp={onSelectApp}
        onOpenApps={onOpenApps}
      />
      <section ref={projectsRef}>
        <div className="section-head">
          <h2 className="section-title">{t("home.projects")}</h2>
          <button className="apps-create-btn" onClick={onCreateProject}>
            {t("home.newProject")}
          </button>
        </div>
        {projects.length === 0 ? (
          <div className="home-empty-panel">
            <p>{t("home.noProjectsHint")}</p>
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
                    {t("home.topicsCount", { count })}
                  </div>
                </button>
              );
            })}
          </div>
        )}
      </section>
      {recent.length > 0 && (
        <section>
          <h2 className="section-title">{t("home.recent")}</h2>
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
      {showStartDialog && (
        <div
          className="modal-backdrop"
          onClick={() => !dialogSubmitting && setShowStartDialog(false)}
        >
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <h2 className="modal-title">{t("home.startDialogTitle")}</h2>
            <p className="modal-hint">{t("home.startDialogHint")}</p>
            <label className="modal-field">
              <span>{t("home.projectSelectLabel")}</span>
              <select
                className="modal-input modal-select"
                value={dialogProjectId}
                onChange={(e) => setDialogProjectId(e.currentTarget.value)}
                autoFocus
              >
                <option value="" disabled>
                  {t("home.projectSelectPlaceholder")}
                </option>
                {projects.map((project) => (
                  <option key={project.id} value={project.id}>
                    {project.name}
                  </option>
                ))}
              </select>
            </label>
            {dialogProject && (
              <>
                <TopicComposer
                  threadId={null}
                  projectRoot={dialogProject.root}
                  running={false}
                  showPlanBanner={false}
                  submitting={dialogSubmitting}
                  stopping={false}
                  apps={dialogApps}
                  memoryScope="project"
                  onSend={submitStartDialog}
                  onStop={async () => {}}
                  onOpenApp={onSelectApp}
                />
              </>
            )}
            {dialogError && <div className="apps-error">{dialogError}</div>}
            <div className="modal-actions">
              <button
                className="modal-btn"
                disabled={dialogSubmitting}
                onClick={() => setShowStartDialog(false)}
              >
                {t("apps.cancel")}
              </button>
            </div>
          </div>
        </div>
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
  const { t } = useI18n();
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
          {t("nav.apps")}
          {runningCount > 0 && (
            <span className="section-badge running">
              {t("home.runningCount", { count: runningCount })}
            </span>
          )}
        </h2>
        <button className="apps-create-btn" onClick={onOpenApps}>
          {t("home.manageUtilities")}
        </button>
      </div>
      {error && <div className="apps-error">{error}</div>}
      {!loaded ? (
        <div className="home-empty-panel">
          <p>{t("home.loadingUtilities")}</p>
        </div>
      ) : items.length === 0 ? (
        <div className="home-empty-panel">
          <p>{t("apps.empty")}</p>
          <button className="home-inline-action" onClick={onOpenApps}>
            {t("home.openUtilities")}
          </button>
        </div>
      ) : (
        <div className="home-apps-grid">
          {items.map((app) => {
            const isReady = app.ready !== false;
            const isRunning = isHomeAppRunning(app, statuses, openAppIds);
            const statusLabel = !isReady
              ? t("home.appCreating")
              : isRunning
                ? t("home.appRunning")
                : t("home.appStopped");
            return (
              <button
                key={app.id}
                className={`home-app-card ${isReady ? "" : "home-app-card-disabled"}`}
                disabled={!isReady}
                onClick={() => isReady && onSelectApp(app.id)}
                title={
                  isReady ? t("home.openUtilityTitle") : t("apps.writingFiles")
                }
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
          {t("home.appsSummary", {
            ready: readyCount,
            running: runningCount,
          })}
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

type DashboardActionSource = {
  appId: string;
  appName: string;
  appIcon?: string | null;
  action: AppAction;
};

type DashboardRecord = {
  status: "loading" | "ok" | "error";
  preview: string;
  value?: unknown;
  updatedAtMs?: number;
  error?: string;
};

type DashboardViewLayout = "summary" | "list" | "table" | "metric";
type DashboardSortMode = "relevance" | "latest" | "oldest" | "largest" | "smallest";

type DashboardValueFilter = {
  id: string;
  label: string;
  tokens: string[];
  keyHints: string[];
};

type DashboardViewSpec = {
  version: 1;
  title: string;
  query: string;
  tokens: string[];
  includeTokens: string[];
  excludeKeys: string[];
  filters: DashboardValueFilter[];
  layout: DashboardViewLayout;
  sort: DashboardSortMode;
  maxItems: number;
  showMeta: boolean;
};

type CustomDashboardWidget = {
  id: string;
  title: string;
  prompt: string;
  createdAtMs: number;
  spec?: DashboardViewSpec;
};

const DASHBOARD_ACTION_PATTERN =
  /\b(dashboard|summary|snapshot|status|stats|health|overview|today)\b/i;
const DASHBOARD_REFRESH_MS = 60_000;
const DASHBOARD_CACHE_TTL_MS = 55_000;

function dashboardSourceKey(source: DashboardActionSource): string {
  return `${source.appId}::${source.action.id}`;
}

function dashboardCacheKey(projectId: string): string {
  return `reflex:project-dashboard:v1:${projectId}`;
}

function customDashboardWidgetsKey(projectId: string): string {
  return `reflex:project-dashboard-widgets:v1:${projectId}`;
}

function readDashboardCache(projectId: string): Record<string, DashboardRecord> {
  try {
    const raw = window.localStorage.getItem(dashboardCacheKey(projectId));
    if (!raw) return {};
    const parsed = JSON.parse(raw);
    return isJsonObject(parsed) ? (parsed as Record<string, DashboardRecord>) : {};
  } catch {
    return {};
  }
}

function writeDashboardCache(
  projectId: string,
  records: Record<string, DashboardRecord>,
) {
  try {
    window.localStorage.setItem(dashboardCacheKey(projectId), JSON.stringify(records));
  } catch {}
}

function readCustomDashboardWidgets(projectId: string): CustomDashboardWidget[] {
  try {
    const raw = window.localStorage.getItem(customDashboardWidgetsKey(projectId));
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    const widgets: CustomDashboardWidget[] = [];
    for (const item of parsed) {
      if (
        !isJsonObject(item) ||
        typeof item.id !== "string" ||
        typeof item.title !== "string" ||
        typeof item.prompt !== "string" ||
        typeof item.createdAtMs !== "number"
      ) {
        continue;
      }
      const spec = isDashboardViewSpec(item.spec)
        ? item.spec
        : buildDashboardViewSpec(item.prompt, item.title);
      widgets.push({
        id: item.id,
        title: item.title,
        prompt: item.prompt,
        createdAtMs: item.createdAtMs,
        spec,
      });
    }
    return widgets;
  } catch {
    return [];
  }
}

function writeCustomDashboardWidgets(
  projectId: string,
  widgets: CustomDashboardWidget[],
) {
  try {
    window.localStorage.setItem(
      customDashboardWidgetsKey(projectId),
      JSON.stringify(widgets),
    );
  } catch {}
}

function actionHasRequiredParams(action: AppAction): boolean {
  const schema = actionParamsSchema(action);
  return isJsonObject(schema) && Array.isArray(schema.required) && schema.required.length > 0;
}

function isDashboardActionCandidate(action: AppAction): boolean {
  if (action.public !== true || actionHasRequiredParams(action)) return false;
  const haystack = [action.id, action.name, action.description ?? ""].join(" ");
  return DASHBOARD_ACTION_PATTERN.test(haystack);
}

function unwrapActionResult(value: any): unknown {
  if (isJsonObject(value) && "result" in value) return value.result;
  return value;
}

function normalizeDashboardValue(value: unknown): unknown {
  let result = unwrapActionResult(value);
  if (isJsonObject(result)) {
    const keys = Object.keys(result);
    if (keys.length === 1 && keys[0] === "value") {
      result = result.value;
    }
  }
  return result;
}

function dashboardPreview(value: unknown): string {
  const result = normalizeDashboardValue(value);
  if (isJsonObject(result)) {
    if (result.setup_required === true) {
      return String(result.message ?? "Setup required");
    }
    if (typeof result.summary === "string") return result.summary;
    if (typeof result.text === "string") return result.text;
    if (typeof result.message === "string") return result.message;
  }
  if (Array.isArray(result)) {
    return result.length === 0
      ? "[]"
      : `${result.length} items\n${previewJsonValue(result.slice(0, 3))}`;
  }
  return previewJsonValue(result);
}

async function fetchDashboardAction(
  projectId: string,
  source: DashboardActionSource,
): Promise<DashboardRecord> {
  try {
    const result = await invoke<any>("app_invoke", {
      appId: source.appId,
      method: "apps.invoke",
      params: {
        app_id: source.appId,
        action_id: source.action.id,
        params: { projectId },
      },
    });
    return {
      status: "ok",
      preview: dashboardPreview(result),
      value: normalizeDashboardValue(result),
      updatedAtMs: Date.now(),
    };
  } catch (e) {
    return {
      status: "error",
      preview: "",
      error: String(e),
      updatedAtMs: Date.now(),
    };
  }
}

function mergeDashboardRecord(
  projectId: string,
  prev: Record<string, DashboardRecord>,
  key: string,
  record: DashboardRecord,
): Record<string, DashboardRecord> {
  const next = { ...prev, [key]: record };
  writeDashboardCache(projectId, next);
  return next;
}

function dashboardTokens(input: string): string[] {
  return Array.from(
    new Set(
      input
        .toLowerCase()
        .split(/[^\p{L}\p{N}_]+/u)
        .map((part) => part.trim())
        .filter((part) => part.length >= 2),
    ),
  );
}

const DASHBOARD_STOP_TOKENS = new Set([
  "a",
  "an",
  "and",
  "as",
  "for",
  "from",
  "in",
  "into",
  "me",
  "my",
  "of",
  "on",
  "please",
  "show",
  "that",
  "the",
  "this",
  "to",
  "what",
  "widget",
  "with",
  "виджет",
  "для",
  "как",
  "мне",
  "надо",
  "нужно",
  "покажи",
  "показать",
  "про",
  "что",
  "хочу",
]);

const DASHBOARD_TOKEN_SYNONYMS: Record<string, string[]> = {
  active: ["enabled", "running", "live"],
  blocked: ["stuck", "waiting"],
  closed: ["complete", "completed", "done", "resolved"],
  count: ["number", "total", "size", "length"],
  current: ["latest", "last", "selected", "active"],
  disabled: ["offline", "stopped"],
  error: ["errors", "failed", "failure", "last_error"],
  event: ["events", "log", "logs"],
  failed: ["error", "errors", "failure"],
  health: ["status", "state"],
  item: ["items", "list"],
  job: ["jobs", "task", "tasks"],
  latest: ["last", "current", "recent"],
  list: ["items", "rows"],
  login: ["auth", "authenticated"],
  source: ["provider", "origin"],
  status: ["state", "health"],
};

type DashboardFilterDefinition = {
  id: string;
  label: string;
  triggers: string[];
  values: string[];
  keyHints: string[];
};

const DASHBOARD_FILTER_DEFINITIONS: DashboardFilterDefinition[] = [
  {
    id: "failed",
    label: "Failed",
    triggers: ["failed", "failure", "error", "errors", "ошиб", "сбой", "неуда"],
    values: ["failed", "failure", "error", "errors", "errored", "crashed", "unhealthy", "ошибка"],
    keyHints: ["error", "health", "message", "result", "status", "state"],
  },
  {
    id: "open",
    label: "Open",
    triggers: ["open", "opened", "todo", "откры", "незакр"],
    values: ["open", "opened", "todo", "new", "active", "pending"],
    keyHints: ["closed", "open", "phase", "status", "state"],
  },
  {
    id: "closed",
    label: "Closed",
    triggers: ["closed", "done", "resolved", "complete", "completed", "закры", "готов", "заверш"],
    values: ["closed", "done", "resolved", "complete", "completed", "success", "succeeded"],
    keyHints: ["closed", "result", "status", "state"],
  },
  {
    id: "running",
    label: "Running",
    triggers: ["running", "live", "active", "started", "выполн", "запущ", "актив"],
    values: ["running", "live", "active", "started", "enabled", "true"],
    keyHints: ["active", "enabled", "live", "running", "status", "state"],
  },
  {
    id: "pending",
    label: "Pending",
    triggers: ["pending", "waiting", "queued", "ожидан", "очеред"],
    values: ["pending", "waiting", "queued", "scheduled"],
    keyHints: ["queue", "status", "state"],
  },
  {
    id: "blocked",
    label: "Blocked",
    triggers: ["blocked", "stuck", "заблок", "застр"],
    values: ["blocked", "stuck", "paused"],
    keyHints: ["blocked", "status", "state"],
  },
  {
    id: "disabled",
    label: "Disabled",
    triggers: ["disabled", "offline", "stopped", "выключ", "отключ"],
    values: ["disabled", "offline", "stopped", "false"],
    keyHints: ["active", "enabled", "ready", "status", "state"],
  },
  {
    id: "ready",
    label: "Ready",
    triggers: ["ready", "healthy"],
    values: ["ready", "available", "healthy", "ok", "true"],
    keyHints: ["available", "health", "ready", "status", "state"],
  },
  {
    id: "stale",
    label: "Stale",
    triggers: ["stale", "old", "устар"],
    values: ["stale", "old", "expired"],
    keyHints: ["fresh", "status", "state", "stale"],
  },
];

const DASHBOARD_RU_TOKEN_SYNONYMS: Array<[string, string[]]> = [
  ["авториз", ["auth", "authenticated", "login"]],
  ["доступ", ["available", "enabled", "ready"]],
  ["задач", ["task", "tasks", "job", "jobs"]],
  ["колич", ["count", "total", "number"]],
  ["кеш", ["cache", "cached"]],
  ["лог", ["log", "logs", "events"]],
  ["метрик", ["metric", "metrics", "stats"]],
  ["ошиб", ["error", "errors", "failed", "failure"]],
  ["послед", ["latest", "last", "current", "recent"]],
  ["разреш", ["permission", "permissions"]],
  ["свод", ["summary", "overview"]],
  ["сесс", ["session", "sessions"]],
  ["событ", ["event", "events"]],
  ["состоян", ["status", "state", "health"]],
  ["спис", ["list", "items"]],
  ["статус", ["status", "state", "health"]],
  ["табли", ["table", "rows", "columns"]],
  ["токен", ["token"]],
  ["числ", ["count", "total", "number"]],
];

const DASHBOARD_META_KEY_PATTERNS = [
  /^auth$/i,
  /^cache($|[_-])/i,
  /^credentials?$/i,
  /^debug$/i,
  /^expires($|[_-])/i,
  /^last[_-]?error$/i,
  /^metadata$/i,
  /^permission[_-]?requests?$/i,
  /^raw$/i,
  /^source$/i,
  /^trace$/i,
];

const DASHBOARD_SECRET_KEY_PATTERNS = [
  /api[_-]?key/i,
  /authorization/i,
  /cookie/i,
  /password/i,
  /refresh[_-]?token/i,
  /secret/i,
  /token/i,
];

type DashboardFlatField = {
  path: string;
  key: string;
  label: string;
  raw: unknown;
  value: string;
  score: number;
  priority: number;
};

type DashboardListSummary = {
  path: string;
  key: string;
  label: string;
  count: number;
  totalCount: number;
  items: string[];
  sample: unknown[];
  score: number;
  priority: number;
};

type DashboardProjectedMetric = {
  label: string;
  value: string;
};

type DashboardProjectedTable = {
  label: string;
  count: number;
  totalCount: number;
  columns: Array<{ key: string; label: string }>;
  rows: Array<Record<string, string>>;
};

type DashboardProjectedView = {
  layout: DashboardViewLayout;
  metrics: DashboardProjectedMetric[];
  rows: DashboardFlatField[];
  lists: DashboardListSummary[];
  table?: DashboardProjectedTable;
  text?: string;
};

function isDashboardViewLayout(value: unknown): value is DashboardViewLayout {
  return (
    value === "summary" ||
    value === "list" ||
    value === "table" ||
    value === "metric"
  );
}

function isDashboardSortMode(value: unknown): value is DashboardSortMode {
  return (
    value === "relevance" ||
    value === "latest" ||
    value === "oldest" ||
    value === "largest" ||
    value === "smallest"
  );
}

function isDashboardValueFilter(value: unknown): value is DashboardValueFilter {
  return (
    isJsonObject(value) &&
    typeof value.id === "string" &&
    typeof value.label === "string" &&
    Array.isArray(value.tokens) &&
    value.tokens.every((item) => typeof item === "string") &&
    Array.isArray(value.keyHints) &&
    value.keyHints.every((item) => typeof item === "string")
  );
}

function isDashboardViewSpec(value: unknown): value is DashboardViewSpec {
  return (
    isJsonObject(value) &&
    value.version === 1 &&
    typeof value.title === "string" &&
    typeof value.query === "string" &&
    Array.isArray(value.tokens) &&
    value.tokens.every((item) => typeof item === "string") &&
    Array.isArray(value.includeTokens) &&
    value.includeTokens.every((item) => typeof item === "string") &&
    Array.isArray(value.excludeKeys) &&
    value.excludeKeys.every((item) => typeof item === "string") &&
    Array.isArray(value.filters) &&
    value.filters.every(isDashboardValueFilter) &&
    isDashboardViewLayout(value.layout) &&
    isDashboardSortMode(value.sort) &&
    typeof value.maxItems === "number" &&
    typeof value.showMeta === "boolean"
  );
}

function expandDashboardTokens(tokens: string[]): string[] {
  const out = new Set<string>();
  for (const token of tokens) {
    out.add(token);
    for (const synonym of DASHBOARD_TOKEN_SYNONYMS[token] ?? []) out.add(synonym);
    for (const [prefix, synonyms] of DASHBOARD_RU_TOKEN_SYNONYMS) {
      if (token.startsWith(prefix)) {
        for (const synonym of synonyms) out.add(synonym);
      }
    }
  }
  return Array.from(out);
}

function inferDashboardLayout(prompt: string): DashboardViewLayout {
  const lower = prompt.toLowerCase();
  if (/\b(table|grid|columns?|rows?)\b|табли/u.test(lower)) return "table";
  if (/\b(count|how many|number|total|metric|kpi)\b|скольк|колич|числ|метрик/u.test(lower)) {
    return "metric";
  }
  if (/\b(list|items?|events?|logs?|sessions?|tasks?|issues?|jobs?)\b|спис|сесс|событ|лог|задач/u.test(lower)) {
    return "list";
  }
  return "summary";
}

function dashboardPromptHasTerm(prompt: string, tokens: string[], term: string): boolean {
  const lower = prompt.toLowerCase();
  if (/^[\p{L}\p{N}_]+$/u.test(term)) {
    return tokens.some((token) => token === term || token.startsWith(term));
  }
  return lower.includes(term);
}

function inferDashboardFilters(prompt: string, tokens: string[]): DashboardValueFilter[] {
  const filters: DashboardValueFilter[] = [];
  for (const definition of DASHBOARD_FILTER_DEFINITIONS) {
    if (
      definition.triggers.some((trigger) =>
        dashboardPromptHasTerm(prompt, tokens, trigger),
      )
    ) {
      filters.push({
        id: definition.id,
        label: definition.label,
        tokens: definition.values,
        keyHints: definition.keyHints,
      });
    }
  }
  return filters;
}

function inferDashboardSort(prompt: string, tokens: string[]): DashboardSortMode {
  if (
    /\b(oldest|earliest|first)\b/i.test(prompt) ||
    tokens.some((token) => ["стар", "перв"].some((prefix) => token.startsWith(prefix)))
  ) {
    return "oldest";
  }
  if (
    /\b(biggest|highest|largest|most|top)\b/i.test(prompt) ||
    tokens.some((token) => ["больш", "макс", "топ"].some((prefix) => token.startsWith(prefix)))
  ) {
    return "largest";
  }
  if (
    /\b(lowest|smallest|least)\b/i.test(prompt) ||
    tokens.some((token) => ["меньш", "миним"].some((prefix) => token.startsWith(prefix)))
  ) {
    return "smallest";
  }
  if (
    /\b(latest|last|newest|recent|current)\b/i.test(prompt) ||
    tokens.some((token) => ["послед", "свеж", "текущ"].some((prefix) => token.startsWith(prefix)))
  ) {
    return "latest";
  }
  return "relevance";
}

function shouldShowDashboardMeta(prompt: string, tokens: string[]): boolean {
  const lower = prompt.toLowerCase();
  return (
    /\b(auth|authentication|cache|credential|debug|expires|metadata|permission|source|token)\b/i.test(lower) ||
    tokens.some((token) =>
      ["авториз", "кеш", "разреш", "токен", "источник"].some((prefix) =>
        token.startsWith(prefix),
      ),
    )
  );
}

function buildDashboardViewSpec(
  prompt: string,
  fallbackTitle?: string,
): DashboardViewSpec {
  const tokens = dashboardTokens(prompt);
  const includeTokens = expandDashboardTokens(
    tokens.filter((token) => !DASHBOARD_STOP_TOKENS.has(token)),
  );
  return {
    version: 1,
    title: fallbackTitle || titleFromWidgetPrompt(prompt),
    query: prompt,
    tokens,
    includeTokens,
    excludeKeys: [],
    filters: inferDashboardFilters(prompt, tokens),
    layout: inferDashboardLayout(prompt),
    sort: inferDashboardSort(prompt, tokens),
    maxItems: 5,
    showMeta: shouldShowDashboardMeta(prompt, tokens),
  };
}

function dashboardSpecForSource(source: DashboardActionSource): DashboardViewSpec {
  return buildDashboardViewSpec(
    [
      source.appName,
      source.action.name || source.action.id,
      source.action.description ?? "",
      source.action.id,
    ].join(" "),
    source.action.name || source.action.id,
  );
}

function dashboardSourceSearchText(source: DashboardActionSource): string {
  return [
    source.appId,
    source.appName,
    source.action.id,
    source.action.name,
    source.action.description ?? "",
  ]
    .join(" ")
    .toLowerCase();
}

function dashboardRecordSearchText(record?: DashboardRecord): string {
  if (!record) return "";
  try {
    return JSON.stringify(record.value ?? record.preview ?? "").toLowerCase();
  } catch {
    return String(record.preview ?? "").toLowerCase();
  }
}

function customWidgetSourceScore(
  widget: CustomDashboardWidget,
  source: DashboardActionSource,
  record?: DashboardRecord,
): number {
  const tokens = widget.spec?.includeTokens ?? buildDashboardViewSpec(widget.prompt).includeTokens;
  if (tokens.length === 0) return 0;
  const haystack = `${dashboardSourceSearchText(source)} ${dashboardRecordSearchText(record)}`;
  return tokens.reduce((score, token) => score + (haystack.includes(token) ? 1 : 0), 0);
}

function uniqueDashboardSources(
  sources: DashboardActionSource[],
): DashboardActionSource[] {
  const seen = new Set<string>();
  const out: DashboardActionSource[] = [];
  for (const source of sources) {
    const key = dashboardSourceKey(source);
    if (seen.has(key)) continue;
    seen.add(key);
    out.push(source);
  }
  return out;
}

function titleFromWidgetPrompt(prompt: string): string {
  const normalized = prompt.replace(/\s+/g, " ").trim();
  if (!normalized) return "Widget";
  return normalized.length > 44 ? `${normalized.slice(0, 41)}...` : normalized;
}

function formatDashboardLabel(key: string): string {
  const spaced = key
    .replace(/\[\]/g, "")
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
    .replace(/[_-]+/g, " ")
    .trim();
  if (!spaced) return key;
  return spaced.charAt(0).toUpperCase() + spaced.slice(1);
}

function formatDashboardScalar(key: string, value: unknown): string {
  if (value == null) return "—";
  if (typeof value === "boolean") return value ? "true" : "false";
  if (typeof value === "number") {
    if (value > 1_000_000_000_000 && /(^|_)(at|time|expires|updated|created)|_ms$/i.test(key)) {
      return new Date(value).toLocaleString();
    }
    return Number.isInteger(value) ? value.toString() : value.toFixed(2);
  }
  return String(value);
}

function dashboardKeyFromPath(path: string): string {
  const parts = path.split(".");
  return parts[parts.length - 1]?.replace(/\[\]/g, "") || path;
}

function dashboardPathLabel(path: string): string {
  const key = dashboardKeyFromPath(path);
  if (!key || key === "items" || key === "data" || key === "value") {
    return formatDashboardLabel(path.replace(/\./g, " "));
  }
  return formatDashboardLabel(key);
}

function dashboardKeyMatches(key: string, patterns: RegExp[]): boolean {
  const normalized = key.replace(/\[\]/g, "");
  return patterns.some((pattern) => pattern.test(normalized));
}

function isDashboardPathExcluded(path: string, spec: DashboardViewSpec): boolean {
  const parts = path.split(".").map((part) => part.replace(/\[\]/g, ""));
  if (parts.some((part) => dashboardKeyMatches(part, DASHBOARD_SECRET_KEY_PATTERNS))) {
    return true;
  }
  if (spec.excludeKeys.some((key) => parts.includes(key))) return true;
  if (spec.showMeta) return false;
  return parts.some((part) => dashboardKeyMatches(part, DASHBOARD_META_KEY_PATTERNS));
}

function dashboardTextScore(text: string, tokens: string[]): number {
  if (tokens.length === 0) return 0;
  const lower = text.toLowerCase();
  return tokens.reduce((score, token) => score + (lower.includes(token) ? 1 : 0), 0);
}

function dashboardFieldPriority(path: string, value: unknown): number {
  const key = dashboardKeyFromPath(path);
  let priority = 0;
  if (/(^|_)(status|state|health|summary|message|title|name|label)$/i.test(key)) {
    priority += 7;
  }
  if (/(^|_)(active|available|count|current|enabled|failed|latest|live|open|ready|running|selected|success|total)$/i.test(key)) {
    priority += 5;
  }
  if (/(^|_)(created|updated|time|at|date)(_at|_ms)?$/i.test(key)) priority += 2;
  if (/(^|_)id$|_id$|key$/i.test(key)) priority -= 1;
  if (typeof value === "string" && value.trim()) priority += 1;
  if (typeof value === "number" || typeof value === "boolean") priority += 1;
  return priority;
}

function displayNameForDashboardItem(value: unknown): string {
  if (isJsonObject(value)) {
    const entries = Object.entries(value);
    const named = entries.find(([key, item]) => {
      const cleanKey = key.toLowerCase();
      return (
        typeof item === "string" &&
        item.trim().length > 0 &&
        (cleanKey === "name" ||
          cleanKey === "title" ||
          cleanKey === "label" ||
          cleanKey.endsWith("_name") ||
          cleanKey.endsWith("_title"))
      );
    });
    if (named) return named[1];

    const summary = entries.find(([key, item]) => {
      const cleanKey = key.toLowerCase();
      return (
        (typeof item === "string" || typeof item === "number") &&
        /summary|message|description|status|state|id|key/.test(cleanKey)
      );
    });
    if (summary) {
      return `${formatDashboardLabel(summary[0])}: ${formatDashboardScalar(summary[0], summary[1])}`;
    }

    const first = entries.find(([, item]) => item != null && typeof item !== "object");
    if (first) return `${formatDashboardLabel(first[0])}: ${formatDashboardScalar(first[0], first[1])}`;
  }
  return formatDashboardScalar("", value);
}

function summarizeDashboardArrayItems(items: unknown[], maxItems: number): string[] {
  const inspected = items.slice(0, 200);
  const counts = new Map<string, number>();
  for (const item of inspected) {
    const label = displayNameForDashboardItem(item).trim();
    if (!label) continue;
    counts.set(label, (counts.get(label) ?? 0) + 1);
  }
  const summarized = Array.from(counts.entries())
    .sort((a, b) => b[1] - a[1])
    .slice(0, maxItems)
    .map(([label, count]) => (count > 1 ? `${label} (${count})` : label));
  if (summarized.length === 0 && items.length > 0) {
    return items.slice(0, maxItems).map((item) => formatDashboardScalar("", item));
  }
  return summarized;
}

type DashboardPrimitiveEntry = {
  path: string;
  key: string;
  value: unknown;
  text: string;
  tokens: string[];
};

function collectDashboardPrimitiveEntries(
  value: unknown,
  spec: DashboardViewSpec,
  path: string,
  entries: DashboardPrimitiveEntry[],
  depth = 0,
) {
  if (depth > 4 || entries.length > 80 || value == null) return;
  if (Array.isArray(value)) {
    for (const [index, item] of value.slice(0, 10).entries()) {
      collectDashboardPrimitiveEntries(item, spec, `${path}[${index}]`, entries, depth + 1);
    }
    return;
  }
  if (!isJsonObject(value)) {
    const effectivePath = path || "value";
    if (isDashboardPathExcluded(effectivePath, spec)) return;
    const text = formatDashboardScalar(effectivePath, value).toLowerCase();
    entries.push({
      path: effectivePath,
      key: dashboardKeyFromPath(effectivePath),
      value,
      text,
      tokens: dashboardTokens(text),
    });
    return;
  }
  for (const [key, item] of Object.entries(value).slice(0, 40)) {
    const nextPath = path ? `${path}.${key}` : key;
    if (isDashboardPathExcluded(nextPath, spec)) continue;
    collectDashboardPrimitiveEntries(item, spec, nextPath, entries, depth + 1);
  }
}

function dashboardTokenMatches(tokens: string[], expected: string): boolean {
  return tokens.some((token) => token === expected || token.startsWith(expected));
}

function dashboardEntryMatchesFilter(
  entry: DashboardPrimitiveEntry,
  filter: DashboardValueFilter,
): boolean {
  const key = entry.key.toLowerCase();
  const path = entry.path.toLowerCase();
  const keyMatches = filter.keyHints.some(
    (hint) => key.includes(hint) || path.includes(hint),
  );
  const valueMatches = filter.tokens.some((token) =>
    dashboardTokenMatches(entry.tokens, token),
  );
  if (keyMatches && valueMatches) return true;

  return filter.tokens.some((token) => {
    if (token.length < 4) return dashboardTokenMatches(entry.tokens, token);
    return entry.text.includes(token);
  });
}

function dashboardItemMatchesFilters(item: unknown, spec: DashboardViewSpec): boolean {
  if (spec.filters.length === 0) return true;
  const entries: DashboardPrimitiveEntry[] = [];
  collectDashboardPrimitiveEntries(item, spec, "", entries);
  if (entries.length === 0) return false;
  return spec.filters.every((filter) =>
    entries.some((entry) => dashboardEntryMatchesFilter(entry, filter)),
  );
}

function parseDashboardTimestamp(value: unknown): number | null {
  if (typeof value === "number") {
    if (value > 1_000_000_000_000) return value;
    if (value > 1_000_000_000) return value * 1000;
    return null;
  }
  if (typeof value !== "string" || !value.trim()) return null;
  const parsed = Date.parse(value);
  return Number.isNaN(parsed) ? null : parsed;
}

function dashboardItemTimestamp(item: unknown, spec: DashboardViewSpec): number | null {
  const entries: DashboardPrimitiveEntry[] = [];
  collectDashboardPrimitiveEntries(item, spec, "", entries);
  let best: number | null = null;
  let bestPriority = -Infinity;
  for (const entry of entries) {
    const timestamp = parseDashboardTimestamp(entry.value);
    if (timestamp == null) continue;
    const priority = /(updated|created|time|date|timestamp|at|ms)$/i.test(entry.key)
      ? 2
      : 0;
    if (priority > bestPriority || (priority === bestPriority && timestamp > (best ?? 0))) {
      best = timestamp;
      bestPriority = priority;
    }
  }
  return best;
}

function dashboardItemNumber(item: unknown, spec: DashboardViewSpec): number | null {
  const entries: DashboardPrimitiveEntry[] = [];
  collectDashboardPrimitiveEntries(item, spec, "", entries);
  let best: number | null = null;
  let bestPriority = -Infinity;
  for (const entry of entries) {
    if (typeof entry.value !== "number") continue;
    const priority =
      dashboardTextScore(`${entry.path} ${entry.key}`, spec.includeTokens) +
      (/(count|total|score|size|amount|value|duration|latency|age)/i.test(entry.key) ? 1 : 0);
    if (priority > bestPriority) {
      best = entry.value;
      bestPriority = priority;
    }
  }
  return best;
}

function dashboardItemRelevance(item: unknown, spec: DashboardViewSpec): number {
  const entries: DashboardPrimitiveEntry[] = [];
  collectDashboardPrimitiveEntries(item, spec, "", entries);
  return entries.reduce(
    (score, entry) =>
      score +
      dashboardTextScore(`${entry.path} ${entry.text}`, spec.includeTokens) +
      spec.filters.reduce(
        (filterScore, filter) =>
          filterScore + (dashboardEntryMatchesFilter(entry, filter) ? 4 : 0),
        0,
      ),
    0,
  );
}

function sortDashboardArrayItems(items: unknown[], spec: DashboardViewSpec): unknown[] {
  if (items.length <= 1) return items;
  const needsTimestamp = spec.sort === "latest" || spec.sort === "oldest";
  const needsNumber = spec.sort === "largest" || spec.sort === "smallest";
  const decorated = items.map((item, index) => ({
    item,
    index,
    timestamp: needsTimestamp ? dashboardItemTimestamp(item, spec) : null,
    number: needsNumber ? dashboardItemNumber(item, spec) : null,
    relevance: dashboardItemRelevance(item, spec),
  }));
  decorated.sort((a, b) => {
    if (spec.sort === "latest" || spec.sort === "oldest") {
      const aTime = a.timestamp ?? (spec.sort === "latest" ? -Infinity : Infinity);
      const bTime = b.timestamp ?? (spec.sort === "latest" ? -Infinity : Infinity);
      const delta = spec.sort === "latest" ? bTime - aTime : aTime - bTime;
      if (delta !== 0) return delta;
    }
    if (spec.sort === "largest" || spec.sort === "smallest") {
      const aNumber = a.number ?? (spec.sort === "largest" ? -Infinity : Infinity);
      const bNumber = b.number ?? (spec.sort === "largest" ? -Infinity : Infinity);
      const delta = spec.sort === "largest" ? bNumber - aNumber : aNumber - bNumber;
      if (delta !== 0) return delta;
    }
    const relevanceDelta = b.relevance - a.relevance;
    if (relevanceDelta !== 0) return relevanceDelta;
    return a.index - b.index;
  });
  return decorated.map((entry) => entry.item);
}

function prepareDashboardArrayItems(items: unknown[], spec: DashboardViewSpec): unknown[] {
  const filtered =
    spec.filters.length === 0
      ? items
      : items.filter((item) => dashboardItemMatchesFilters(item, spec));
  return sortDashboardArrayItems(filtered, spec);
}

function collectDashboardFields(
  value: unknown,
  spec: DashboardViewSpec,
  path: string,
  fields: DashboardFlatField[],
  depth = 0,
) {
  if (depth > 5 || value == null) return;
  if (Array.isArray(value)) return;
  if (!isJsonObject(value)) {
    if (!path || isDashboardPathExcluded(path, spec)) return;
    const label = dashboardPathLabel(path);
    const formatted = formatDashboardScalar(path, value);
    const score =
      dashboardTextScore(`${path} ${label}`, spec.includeTokens) * 3 +
      dashboardTextScore(formatted, spec.includeTokens);
    fields.push({
      path,
      key: dashboardKeyFromPath(path),
      label,
      raw: value,
      value: formatted,
      score,
      priority: dashboardFieldPriority(path, value),
    });
    return;
  }

  for (const [key, item] of Object.entries(value).slice(0, 80)) {
    const nextPath = path ? `${path}.${key}` : key;
    if (isDashboardPathExcluded(nextPath, spec)) continue;
    if (Array.isArray(item)) continue;
    collectDashboardFields(item, spec, nextPath, fields, depth + 1);
  }
}

function collectDashboardLists(
  value: unknown,
  spec: DashboardViewSpec,
  path: string,
  lists: DashboardListSummary[],
  depth = 0,
) {
  if (depth > 5 || value == null) return;
  if (Array.isArray(value)) {
    if (!path || isDashboardPathExcluded(path, spec)) return;
    const label = dashboardPathLabel(path);
    const preparedItems = prepareDashboardArrayItems(value, spec);
    const items = summarizeDashboardArrayItems(preparedItems, spec.maxItems);
    lists.push({
      path,
      key: dashboardKeyFromPath(path),
      label,
      count: preparedItems.length,
      totalCount: value.length,
      items,
      sample: preparedItems.slice(0, Math.max(spec.maxItems, 10)),
      score:
        dashboardTextScore(`${path} ${label} ${items.join(" ")}`, spec.includeTokens) +
        (preparedItems.length < value.length ? 2 : 0),
      priority: preparedItems.length > 0 ? 3 : 1,
    });
    return;
  }
  if (!isJsonObject(value)) return;
  for (const [key, item] of Object.entries(value).slice(0, 80)) {
    const nextPath = path ? `${path}.${key}` : key;
    if (isDashboardPathExcluded(nextPath, spec)) continue;
    collectDashboardLists(item, spec, nextPath, lists, depth + 1);
  }
}

function sortDashboardItems<T extends { score: number; priority: number; label: string }>(
  items: T[],
): T[] {
  return [...items].sort((a, b) => {
    const scoreDelta = b.score - a.score;
    if (scoreDelta !== 0) return scoreDelta;
    const priorityDelta = b.priority - a.priority;
    if (priorityDelta !== 0) return priorityDelta;
    return a.label.localeCompare(b.label);
  });
}

function buildDashboardTable(
  lists: DashboardListSummary[],
  spec: DashboardViewSpec,
): DashboardProjectedTable | undefined {
  const source = sortDashboardItems(lists).find((list) =>
    list.sample.some((item) => isJsonObject(item)),
  );
  if (!source) return undefined;

  const objectRows = source.sample.filter(isJsonObject);
  const columnScores = new Map<string, { label: string; score: number; priority: number }>();
  for (const row of objectRows) {
    for (const [key, item] of Object.entries(row)) {
      const path = `${source.path}[].${key}`;
      if (item == null || typeof item === "object" || isDashboardPathExcluded(path, spec)) {
        continue;
      }
      const label = formatDashboardLabel(key);
      const current = columnScores.get(key);
      const score =
        dashboardTextScore(`${path} ${label}`, spec.includeTokens) * 3 +
        dashboardFieldPriority(path, item);
      if (!current || score > current.score) {
        columnScores.set(key, {
          label,
          score,
          priority: dashboardFieldPriority(path, item),
        });
      }
    }
  }

  const columns = Array.from(columnScores.entries())
    .sort((a, b) => {
      const scoreDelta = b[1].score - a[1].score;
      if (scoreDelta !== 0) return scoreDelta;
      return b[1].priority - a[1].priority;
    })
    .slice(0, 4)
    .map(([key, meta]) => ({ key, label: meta.label }));

  if (columns.length === 0) return undefined;

  return {
    label: source.label,
    count: source.count,
    totalCount: source.totalCount,
    columns,
    rows: objectRows.slice(0, spec.maxItems).map((row) => {
      const out: Record<string, string> = {};
      for (const column of columns) {
        out[column.key] = formatDashboardScalar(column.key, row[column.key]);
      }
      return out;
    }),
  };
}

function projectDashboardValue(
  value: unknown,
  spec: DashboardViewSpec,
): DashboardProjectedView {
  const normalized = normalizeDashboardValue(value);
  if (normalized == null) {
    return { layout: spec.layout, metrics: [], rows: [], lists: [] };
  }
  if (typeof normalized !== "object") {
    return {
      layout: spec.layout,
      metrics: [],
      rows: [],
      lists: [],
      text: formatDashboardScalar("", normalized),
    };
  }

  const fields: DashboardFlatField[] = [];
  const lists: DashboardListSummary[] = [];
  if (Array.isArray(normalized)) {
    collectDashboardLists(normalized, spec, "items", lists);
  } else {
    collectDashboardFields(normalized, spec, "", fields);
    collectDashboardLists(normalized, spec, "", lists);
  }

  const rows = sortDashboardItems(fields).slice(0, spec.layout === "metric" ? 4 : 8);
  const visibleLists = sortDashboardItems(lists).slice(0, spec.layout === "summary" ? 3 : 5);
  const metrics: DashboardProjectedMetric[] = [];

  if (spec.layout === "metric") {
    for (const list of visibleLists.slice(0, 3)) {
      metrics.push({ label: list.label, value: list.count.toString() });
    }
    for (const field of rows) {
      if (metrics.length >= 4) break;
      if (
        typeof field.raw === "number" ||
        typeof field.raw === "boolean" ||
        /(status|state|health|count|total|active|enabled|ready|running)/i.test(field.key)
      ) {
        metrics.push({ label: field.label, value: field.value });
      }
    }
  }

  return {
    layout: spec.layout,
    metrics,
    rows,
    lists: visibleLists,
    table: spec.layout === "table" ? buildDashboardTable(visibleLists, spec) : undefined,
  };
}

function dashboardCountLabel(
  t: Translate,
  count: number,
  totalCount: number,
): string {
  return count === totalCount
    ? t("dashboard.itemsCount", { count })
    : t("dashboard.filteredItemsCount", { count, total: totalCount });
}

function dashboardMetricValue(count: number, totalCount: number): string {
  return count === totalCount ? count.toString() : `${count}/${totalCount}`;
}

function dashboardProjectedHasContent(projected: DashboardProjectedView): boolean {
  return Boolean(
    projected.text ||
      projected.metrics.length > 0 ||
      projected.table ||
      projected.lists.length > 0 ||
      projected.rows.length > 0,
  );
}

function DashboardMetricsView({ metrics }: { metrics: DashboardProjectedMetric[] }) {
  if (metrics.length === 0) return null;
  return (
    <div className="dashboard-value-metrics">
      {metrics.map((metric) => (
        <div key={`${metric.label}:${metric.value}`} className="dashboard-value-metric">
          <span className="dashboard-value-metric-value">{metric.value}</span>
          <span className="dashboard-value-label">{metric.label}</span>
        </div>
      ))}
    </div>
  );
}

function DashboardTableView({ table }: { table: DashboardProjectedTable }) {
  const { t } = useI18n();
  return (
    <div className="dashboard-value-table-wrap">
      <div className="dashboard-value-count">
        {table.label} · {dashboardCountLabel(t, table.count, table.totalCount)}
      </div>
      <table className="dashboard-value-table">
        <thead>
          <tr>
            {table.columns.map((column) => (
              <th key={column.key}>{column.label}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {table.rows.map((row, rowIndex) => (
            <tr key={rowIndex}>
              {table.columns.map((column) => (
                <td key={column.key}>{row[column.key]}</td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function DashboardListsView({ lists }: { lists: DashboardListSummary[] }) {
  const { t } = useI18n();
  if (lists.length === 0) return null;
  return (
    <div className="dashboard-value-lists">
      {lists.map((list) => (
        <div key={list.path} className="dashboard-value-list">
          <div className="dashboard-value-count">
            {list.label} · {dashboardCountLabel(t, list.count, list.totalCount)}
          </div>
          {list.items.length > 0 && (
            <ul>
              {list.items.map((item) => (
                <li key={item}>{item}</li>
              ))}
            </ul>
          )}
        </div>
      ))}
    </div>
  );
}

function DashboardRowsView({ rows }: { rows: DashboardFlatField[] }) {
  if (rows.length === 0) return null;
  return (
    <div className="dashboard-value-rows">
      {rows.map((row) => (
        <div key={row.path} className="dashboard-value-row">
          <span className="dashboard-value-label">{row.label}</span>
          <span className="dashboard-value-scalar">{row.value}</span>
        </div>
      ))}
    </div>
  );
}

function DashboardProjectedViewContent({
  projected,
  fallback,
}: {
  projected: DashboardProjectedView;
  fallback?: string;
}) {
  const { t } = useI18n();
  if (!dashboardProjectedHasContent(projected)) {
    return (
      <div className="dashboard-value-empty">
        {fallback ?? t("dashboard.noDisplayableData")}
      </div>
    );
  }
  if (projected.text) {
    return <div className="dashboard-value-text">{projected.text}</div>;
  }
  return (
    <div className={`dashboard-value-object dashboard-value-${projected.layout}`}>
      <DashboardMetricsView metrics={projected.metrics} />
      {projected.table && <DashboardTableView table={projected.table} />}
      {projected.layout !== "table" && <DashboardListsView lists={projected.lists} />}
      <DashboardRowsView rows={projected.rows} />
    </div>
  );
}

function DashboardValueView({
  value,
  fallback,
  spec,
}: {
  value: unknown;
  fallback?: string;
  spec?: DashboardViewSpec;
}) {
  const viewSpec = spec ?? buildDashboardViewSpec("summary status overview");
  return (
    <DashboardProjectedViewContent
      projected={projectDashboardValue(value, viewSpec)}
      fallback={fallback}
    />
  );
}

type DashboardCompositeEntry = {
  key: string;
  source: DashboardActionSource;
  record: DashboardRecord;
};

type DashboardProjectedSource = {
  key: string;
  label: string;
  status: DashboardRecord["status"];
  error?: string;
  projected?: DashboardProjectedView;
};

function dashboardCompositeSourceLabel(source: DashboardActionSource): string {
  return `${source.appIcon ?? "U"} ${source.appName} · ${source.action.name || source.action.id}`;
}

function projectDashboardCompositeEntries(
  entries: DashboardCompositeEntry[],
  spec: DashboardViewSpec,
): DashboardProjectedSource[] {
  return entries.map((entry) => ({
    key: entry.key,
    label: dashboardCompositeSourceLabel(entry.source),
    status: entry.record.status,
    error: entry.record.error,
    projected:
      entry.record.status === "ok"
        ? projectDashboardValue(entry.record.value ?? entry.record.preview, spec)
        : undefined,
  }));
}

function aggregateDashboardMetrics(
  sources: DashboardProjectedSource[],
): DashboardProjectedMetric[] {
  const listTotals = new Map<string, { count: number; totalCount: number }>();
  const projectedSources = sources.filter((source) => source.projected);
  for (const source of projectedSources) {
    for (const list of source.projected?.lists ?? []) {
      const current = listTotals.get(list.label) ?? { count: 0, totalCount: 0 };
      current.count += list.count;
      current.totalCount += list.totalCount;
      listTotals.set(list.label, current);
    }
  }
  const metrics = Array.from(listTotals.entries()).map(([label, value]) => ({
    label,
    value: dashboardMetricValue(value.count, value.totalCount),
  }));
  if (metrics.length > 0) return metrics.slice(0, 4);

  const multipleSources = projectedSources.length > 1;
  const seen = new Set<string>();
  for (const source of projectedSources) {
    for (const metric of source.projected?.metrics ?? []) {
      const label = multipleSources ? `${source.label} · ${metric.label}` : metric.label;
      const key = `${label}:${metric.value}`;
      if (seen.has(key)) continue;
      seen.add(key);
      metrics.push({ label, value: metric.value });
      if (metrics.length >= 4) return metrics;
    }
  }
  return metrics;
}

function aggregateDashboardTable(
  sources: DashboardProjectedSource[],
  sourceColumnLabel: string,
): DashboardProjectedTable | undefined {
  const tableSources = sources.flatMap((source) =>
    source.projected?.table
      ? [{ sourceLabel: source.label, table: source.projected.table }]
      : [],
  );
  if (tableSources.length === 0) return undefined;

  const columns = new Map<string, { key: string; label: string }>();
  if (tableSources.length > 1) {
    columns.set("__source", { key: "__source", label: sourceColumnLabel });
  }
  for (const { table } of tableSources) {
    for (const column of table.columns) {
      if (columns.size >= 5) break;
      if (!columns.has(column.key)) columns.set(column.key, column);
    }
  }

  const columnList = Array.from(columns.values());
  return {
    label: tableSources[0].table.label,
    count: tableSources.reduce((sum, item) => sum + item.table.count, 0),
    totalCount: tableSources.reduce((sum, item) => sum + item.table.totalCount, 0),
    columns: columnList,
    rows: tableSources.flatMap(({ sourceLabel, table }) =>
      table.rows.map((row) => {
        const next: Record<string, string> = {};
        for (const column of columnList) {
          next[column.key] =
            column.key === "__source" ? sourceLabel : row[column.key] || "—";
        }
        return next;
      }),
    ),
  };
}

function DashboardCompositeValueView({
  entries,
  spec,
}: {
  entries: DashboardCompositeEntry[];
  spec: DashboardViewSpec;
}) {
  const { t } = useI18n();
  const sources = projectDashboardCompositeEntries(entries, spec);
  const contentSources = sources.filter(
    (source) => source.projected && dashboardProjectedHasContent(source.projected),
  );
  if (contentSources.length === 0) {
    const loading = sources.some((source) => source.status === "loading");
    const error = sources.find((source) => source.status === "error")?.error;
    return (
      <div className="dashboard-value-empty">
        {loading ? t("dashboard.loading") : error || t("dashboard.emptyValue")}
      </div>
    );
  }

  const metrics = spec.layout === "metric" ? aggregateDashboardMetrics(contentSources) : [];
  const table =
    spec.layout === "table"
      ? aggregateDashboardTable(contentSources, t("dashboard.sourceColumn"))
      : undefined;
  const listBlocks = contentSources.flatMap((source) =>
    (source.projected?.lists ?? []).map((list) => ({
      list: {
        ...list,
        path: `${source.key}:${list.path}`,
        label: contentSources.length > 1 ? `${source.label} · ${list.label}` : list.label,
      },
    })),
  );
  const rowBlocks = contentSources
    .filter((source) => (source.projected?.rows.length ?? 0) > 0)
    .slice(0, 4);

  return (
    <div className="dashboard-composite-value">
      <DashboardMetricsView metrics={metrics} />
      {table && <DashboardTableView table={table} />}
      {spec.layout !== "table" && (
        <DashboardListsView lists={listBlocks.map((block) => block.list)} />
      )}
      {rowBlocks.map((source) => (
        <div key={source.key} className="dashboard-composite-source">
          {contentSources.length > 1 && (
            <div className="dashboard-custom-source-title">{source.label}</div>
          )}
          <DashboardRowsView rows={source.projected?.rows ?? []} />
        </div>
      ))}
    </div>
  );
}

function ProjectDashboard({
  project,
  apps,
  onOpenApp,
}: {
  project: Project;
  apps: AppManifest[];
  onOpenApp: (id: string) => void;
}) {
  const { t } = useI18n();
  const linkedIds = useMemo(() => new Set(project.apps ?? []), [project.apps]);
  const allActionSources = useMemo<DashboardActionSource[]>(() => {
    const out: DashboardActionSource[] = [];
    for (const app of apps) {
      if (!linkedIds.has(app.id) || app.ready === false) continue;
      for (const action of app.actions ?? []) {
        if (action.public !== true || actionHasRequiredParams(action)) continue;
        out.push({
          appId: app.id,
          appName: app.name,
          appIcon: app.icon ?? null,
          action,
        });
      }
    }
    return out;
  }, [apps, linkedIds]);
  const actionSources = useMemo(
    () => allActionSources.filter((source) => isDashboardActionCandidate(source.action)),
    [allActionSources],
  );
  const legacyWidgetSources = useMemo<WidgetSource[]>(() => {
    const out: WidgetSource[] = [];
    for (const app of apps) {
      if (!linkedIds.has(app.id) || app.ready === false) continue;
      for (const widget of app.widgets ?? []) {
        out.push({
          appId: app.id,
          appName: app.name,
          appIcon: app.icon ?? null,
          widget,
        });
      }
    }
    return out;
  }, [apps, linkedIds]);
  const [records, setRecords] = useState<Record<string, DashboardRecord>>(() =>
    readDashboardCache(project.id),
  );
  const [customWidgets, setCustomWidgets] = useState<CustomDashboardWidget[]>(() =>
    readCustomDashboardWidgets(project.id),
  );
  const [addingWidget, setAddingWidget] = useState(false);
  const [widgetPrompt, setWidgetPrompt] = useState("");
  const activeActionSources = useMemo(
    () =>
      uniqueDashboardSources([
        ...actionSources,
        ...(customWidgets.length > 0 ? allActionSources : []),
      ]),
    [actionSources, allActionSources, customWidgets.length],
  );

  useEffect(() => {
    setRecords(readDashboardCache(project.id));
    setCustomWidgets(readCustomDashboardWidgets(project.id));
    setAddingWidget(false);
    setWidgetPrompt("");
  }, [project.id]);

  useEffect(() => {
    if (activeActionSources.length === 0) return;
    let cancelled = false;

    const refresh = async () => {
      await Promise.all(
        activeActionSources.map(async (source) => {
          const key = dashboardSourceKey(source);
          const cached = readDashboardCache(project.id)[key];
          if (
            cached?.updatedAtMs &&
            Date.now() - cached.updatedAtMs < DASHBOARD_CACHE_TTL_MS
          ) {
            setRecords((prev) => ({ ...prev, [key]: cached }));
            return;
          }
          setRecords((prev) =>
            prev[key]
              ? prev
              : {
                  ...prev,
                  [key]: { status: "loading", preview: "" },
                },
          );
          const record = await fetchDashboardAction(project.id, source);
          if (cancelled) return;
          setRecords((prev) =>
            mergeDashboardRecord(project.id, prev, key, record),
          );
        }),
      );
    };

    void refresh();
    const timer = window.setInterval(refresh, DASHBOARD_REFRESH_MS);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [activeActionSources, project.id]);

  const refreshOne = async (source: DashboardActionSource) => {
    const key = dashboardSourceKey(source);
    setRecords((prev) => ({
      ...prev,
      [key]: { ...(prev[key] ?? { preview: "" }), status: "loading" },
    }));
    const record = await fetchDashboardAction(project.id, source);
    setRecords((prev) => mergeDashboardRecord(project.id, prev, key, record));
  };

  const addCustomWidget = () => {
    const prompt = widgetPrompt.trim();
    if (!prompt) return;
    const widget: CustomDashboardWidget = {
      id: `custom_${Date.now().toString(36)}`,
      title: titleFromWidgetPrompt(prompt),
      prompt,
      createdAtMs: Date.now(),
      spec: buildDashboardViewSpec(prompt),
    };
    setCustomWidgets((prev) => {
      const next = [widget, ...prev];
      writeCustomDashboardWidgets(project.id, next);
      return next;
    });
    setWidgetPrompt("");
    setAddingWidget(false);
  };

  const removeCustomWidget = (id: string) => {
    setCustomWidgets((prev) => {
      const next = prev.filter((widget) => widget.id !== id);
      writeCustomDashboardWidgets(project.id, next);
      return next;
    });
  };

  return (
    <section className="project-dashboard">
      <div className="section-head">
        <h2 className="section-title">{t("project.dashboard")}</h2>
        <button
          type="button"
          className="apps-create-btn"
          onClick={() => setAddingWidget((open) => !open)}
        >
          {addingWidget ? t("dashboard.cancelAdd") : t("dashboard.addWidget")}
        </button>
      </div>
      {addingWidget && (
        <div className="dashboard-widget-composer">
          <textarea
            className="modal-input"
            rows={3}
            value={widgetPrompt}
            onChange={(e) => setWidgetPrompt(e.currentTarget.value)}
            placeholder={t("dashboard.addPlaceholder")}
            autoFocus
          />
          <div className="dashboard-widget-composer-actions">
            <button
              type="button"
              className="modal-btn modal-btn-primary"
              disabled={!widgetPrompt.trim()}
              onClick={addCustomWidget}
            >
              {t("dashboard.saveWidget")}
            </button>
          </div>
        </div>
      )}
      {customWidgets.length > 0 && (
        <div className="dashboard-custom-grid">
          {customWidgets.map((widget) => {
            const spec = widget.spec ?? buildDashboardViewSpec(widget.prompt, widget.title);
            const scored = allActionSources
              .map((source) => ({
                source,
                key: dashboardSourceKey(source),
                score: customWidgetSourceScore(
                  widget,
                  source,
                  records[dashboardSourceKey(source)],
                ),
              }))
              .filter((item) => item.score > 0)
              .sort((a, b) => b.score - a.score)
              .slice(0, 3);
            const hasPendingSources = allActionSources.some((source) => {
              const record = records[dashboardSourceKey(source)];
              return !record || record.status === "loading";
            });
            return (
              <article key={widget.id} className="dashboard-custom-card">
                <header className="dashboard-custom-header">
                  <div>
                    <div className="dashboard-action-name">{widget.title}</div>
                    <div className="dashboard-widget-prompt">{widget.prompt}</div>
                  </div>
                  <button
                    type="button"
                    className="dashboard-action-refresh"
                    onClick={() => removeCustomWidget(widget.id)}
                    title={t("dashboard.removeWidget")}
                  >
                    ×
                  </button>
                </header>
                {scored.length === 0 ? (
                  <div className="dashboard-empty">
                    {hasPendingSources
                      ? t("dashboard.matchingData")
                      : t("dashboard.noSource")}
                  </div>
                ) : (
                  <DashboardCompositeValueView
                    spec={spec}
                    entries={scored.map(({ source, key }) => ({
                      key,
                      source,
                      record: records[key] ?? {
                        status: "loading",
                        preview: "",
                      },
                    }))}
                  />
                )}
              </article>
            );
          })}
        </div>
      )}
      {actionSources.length === 0 ? (
        customWidgets.length === 0 && (
        <div className="dashboard-empty">{t("dashboard.empty")}</div>
        )
      ) : (
        <div className="dashboard-action-grid">
          {actionSources.map((source) => {
            const key = dashboardSourceKey(source);
            const record = records[key] ?? { status: "loading", preview: "" };
            const spec = dashboardSpecForSource(source);
            return (
              <article
                key={key}
                className={`dashboard-action-card dashboard-action-${record.status}`}
              >
                <header className="dashboard-action-header">
                  <button
                    type="button"
                    className="dashboard-action-source"
                    onClick={() => onOpenApp(source.appId)}
                    title={t("widget.openTitle", { name: source.appName })}
                  >
                    <span>{source.appIcon ?? "U"}</span>
                    <span>{source.appName}</span>
                  </button>
                  <button
                    type="button"
                    className="dashboard-action-refresh"
                    onClick={() => void refreshOne(source)}
                    title={t("dashboard.refresh")}
                  >
                    ↻
                  </button>
                </header>
                <div className="dashboard-action-name">
                  {source.action.name || source.action.id}
                </div>
                <DashboardValueView
                  value={
                    record.status === "ok"
                      ? record.value ?? record.preview
                      : null
                  }
                  spec={spec}
                  fallback={
                    record.status === "loading"
                      ? t("dashboard.loading")
                      : record.status === "error"
                        ? record.error || t("dashboard.error")
                        : t("dashboard.emptyValue")
                  }
                />
                {record.updatedAtMs && (
                  <div className="dashboard-action-meta">
                    {t("dashboard.cachedAt", {
                      time: new Date(record.updatedAtMs).toLocaleTimeString(),
                    })}
                  </div>
                )}
              </article>
            );
          })}
        </div>
      )}
      {legacyWidgetSources.length > 0 && (
        <details className="dashboard-legacy">
          <summary>{t("dashboard.legacyWidgets")}</summary>
          <WidgetGrid sources={legacyWidgetSources} onOpenApp={onOpenApp} />
        </details>
      )}
    </section>
  );
}

function ProjectScreen({
  projectId,
  projects,
  threads,
  onSelectTopic,
  onProjectUpdated,
  onCreateTopic,
  onCreateApp,
  onOpenApp,
}: {
  projectId: string;
  projects: Project[];
  threads: Thread[];
  onSelectTopic: (id: string) => void;
  onProjectUpdated: (p: Project) => void;
  onCreateTopic: (prompt: string, planMode: boolean) => Promise<void>;
  onCreateApp: () => void;
  onOpenApp: (id: string) => void;
}) {
  const { t } = useI18n();
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
  const [projectMemoryStats, setProjectMemoryStats] =
    useState<ProjectMemoryStats | null>(null);

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

  const visibleEntries = useMemo(
    () =>
      entries.filter(
        (e) => (showHidden || !e.is_hidden) && e.name !== ".reflex",
      ),
    [entries, showHidden],
  );
  const visibleEntryPaths = useMemo(
    () => visibleEntries.map((entry) => entry.path),
    [visibleEntries],
  );
  const runningCount = topics.filter((t) => !t.done).length;

  useEffect(() => {
    if (!project || visibleEntryPaths.length === 0) {
      setStatuses({});
      return;
    }
    let alive = true;
    invoke<PathStatus[]>("memory_path_status_batch", {
      projectRoot: project.root,
      paths: visibleEntryPaths,
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
  }, [project?.root, visibleEntryPaths, statusTick]);

  useEffect(() => {
    if (!project) {
      setProjectMemoryStats(null);
      return;
    }
    let alive = true;
    invoke<ProjectMemoryStats>("memory_stats", { projectRoot: project.root })
      .then((nextStats) => {
        if (alive) setProjectMemoryStats(nextStats);
      })
      .catch((e) => {
        if (alive) {
          setProjectMemoryStats(null);
          console.error("[reflex] memory_stats", e);
        }
      });
    return () => {
      alive = false;
    };
  }, [project?.root, statusTick]);

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
  const fallbackFileIndexStats = useMemo(() => {
    let indexed = 0;
    let stale = 0;
    let ignored = 0;
    let indexable = 0;
    for (const entry of visibleEntries) {
      if (entry.kind !== "file") continue;
      const status = statuses[entry.path];
      if (!status) continue;
      const ignoredClass =
        status.class === "binary" ||
        status.class === "toolarge" ||
        status.class === "unsupported";
      if (ignoredClass) {
        ignored += 1;
        continue;
      }
      indexable += 1;
      if (status.indexed) indexed += 1;
      if (status.indexed && status.stale) stale += 1;
    }
    return {
      indexed,
      stale,
      ignored,
      missing: Math.max(indexable - indexed, 0),
    };
  }, [statuses, visibleEntries]);
  const ragDocs = projectMemoryStats?.docs ?? fallbackFileIndexStats.indexed;
  const ragChunks = projectMemoryStats?.chunks ?? null;
  const ragSources = projectMemoryStats?.sources ?? null;
  const ragStale = projectMemoryStats?.stale ?? fallbackFileIndexStats.stale;
  const ragMissing =
    projectMemoryStats?.missing ?? fallbackFileIndexStats.missing;
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
              title={t("project.openInFinder")}
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
              placeholder={t("project.descriptionEditPlaceholder")}
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
              title={t("project.descriptionEditTitle")}
            >
              {project.description ?? t("project.descriptionEmpty")}
            </div>
          )}
        </section>
      )}

      {project && (
        <section className="project-start-panel">
          <button
            className="project-start-action project-start-action-primary"
            onClick={() => setShowNewTopic(true)}
          >
            <span>{t("project.newTopic")}</span>
            <small>{t("project.startTopicHint")}</small>
          </button>
          <button className="project-start-action" onClick={onCreateApp}>
            <span>{t("project.createUtility")}</span>
            <small>{t("project.utilityHint")}</small>
          </button>
          <button
            className="project-start-action"
            onClick={() => setShowLinkPicker(true)}
          >
            <span>{t("project.linkUtility")}</span>
            <small>{t("project.connectUtilityHint")}</small>
          </button>
        </section>
      )}

      {project && (
        <section
          className="project-context-grid"
          aria-label={t("project.agentContext")}
        >
          <article className="project-context-item">
            <span className="project-context-label">
              {t("project.safetyMode")}
            </span>
            <strong>{sandbox}</strong>
          </article>
          <button
            className="project-context-item project-context-button"
            onClick={openAgentProfileEditor}
            type="button"
          >
            <span className="project-context-label">
              {t("project.agentBehavior")}
            </span>
            <strong>
              {hasAgentProfile ? t("project.configured") : t("project.default")}
            </strong>
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
          </button>
          <button
            className="project-context-item project-context-button"
            onClick={openMcpEditor}
            type="button"
          >
            <span className="project-context-label">
              {t("project.connections")}
            </span>
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
          </button>
          <article className="project-context-item">
            <span className="project-context-label">
              {t("project.knowledge")}
            </span>
            <strong>{t("project.docsCount", { count: ragDocs })}</strong>
            <div className="project-context-chips">
              {ragChunks != null && (
                <span>{t("project.chunksCount", { count: ragChunks })}</span>
              )}
              {ragSources != null && (
                <span>{t("project.sourcesCount", { count: ragSources })}</span>
              )}
              {ragStale > 0 && (
                <span>{t("project.staleCount", { count: ragStale })}</span>
              )}
              {ragMissing > 0 && (
                <span>{t("project.missingCount", { count: ragMissing })}</span>
              )}
              {!projectMemoryStats && fallbackFileIndexStats.ignored > 0 && (
                <span>
                  {t("project.ignoredCount", {
                    count: fallbackFileIndexStats.ignored,
                  })}
                </span>
              )}
              {ragDocs === 0 &&
                ragStale === 0 &&
                ragMissing === 0 &&
                !fallbackFileIndexStats.ignored && (
                  <span>{t("project.noIndex")}</span>
                )}
            </div>
          </article>
          <article className="project-context-item">
            <span className="project-context-label">
              {t("project.utilities")}
            </span>
            <strong>{linkedAppIds.length}</strong>
          </article>
          <article className="project-context-item">
            <span className="project-context-label">
              {t("project.topics")}
            </span>
            <strong>{topics.length}</strong>
            {runningCount > 0 && (
              <span className="project-context-note">
                {t("project.runningCount", { count: runningCount })}
              </span>
            )}
          </article>
        </section>
      )}

      {project && (
        <ProjectDashboard
          project={project}
          apps={installedApps}
          onOpenApp={onOpenApp}
        />
      )}

      {project && (
        <section className="project-linked">
          <div className="section-head">
            <h2 className="section-title">{t("project.linkedUtilities")}</h2>
            <div className="section-actions">
              <button className="apps-create-btn" onClick={onCreateApp}>
                {t("project.createUtility")}
              </button>
              <button
                className="apps-create-btn"
                onClick={() => setShowLinkPicker(true)}
              >
                {t("project.linkUtility")}
              </button>
            </div>
          </div>
          {(() => {
            const linked = linkedAppIds
              .map((id) => installedApps.find((a) => a.id === id))
              .filter((a): a is AppManifest => !!a);
            if (linked.length === 0) {
              return (
                <div className="project-linked-empty">
                  {t("project.linkedEmpty")}
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
                        {t("project.open")}
                      </button>
                      <button
                        className="apps-trash-action"
                        onClick={() => void unlinkApp(a.id)}
                      >
                        {t("project.unlink")}
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
        <details className="project-settings project-advanced">
          <summary className="project-advanced-summary">
            <span>
              <strong>{t("project.advancedControls")}</strong>
              <small>{t("project.advancedControlsHint")}</small>
            </span>
          </summary>
          <div className="project-advanced-body">
            <div className="setting-row">
              <label className="setting-label">{t("project.safetyMode")}</label>
              <select
                className="setting-select"
                value={sandbox}
                onChange={(e) => void setSandbox(e.currentTarget.value)}
              >
                <option value="read-only">{t("project.readOnlySafe")}</option>
                <option value="workspace-write">
                  {t("project.workspaceWriteDefault")}
                </option>
                <option value="danger-full-access">
                  danger-full-access ⚠️
                </option>
              </select>
              {sandbox === "danger-full-access" && (
                <span className="setting-hint setting-hint-warn">
                  {t("project.dangerFullAccessHint")}
                </span>
              )}
            </div>
            <div className="setting-row setting-row-block">
              <label className="setting-label">
                {t("project.agentBehavior")}
              </label>
              <div className="setting-mcp-summary">
                {hasAgentProfile ? (
                  <>
                    {project?.agent_instructions?.trim() && (
                      <span className="setting-chip setting-chip-muted">
                        {t("project.instructionsChip")}
                      </span>
                    )}
                    {projectSkills.map((skill) => (
                      <span key={skill} className="setting-chip">
                        {skill}
                      </span>
                    ))}
                  </>
                ) : (
                  <span className="setting-empty">
                    {t("project.codexDefaultBehavior")}
                  </span>
                )}
                <button
                  className="setting-action"
                  onClick={openAgentProfileEditor}
                >
                  {t("project.editProfile")}
                </button>
              </div>
              {profileEditing && (
                <div className="setting-editor">
                  <label className="setting-editor-label">
                    {t("project.instructionsLabel")}
                  </label>
                  <textarea
                    className="setting-textarea"
                    value={profileInstructionsDraft}
                    spellCheck={false}
                    onChange={(e) =>
                      setProfileInstructionsDraft(e.currentTarget.value)
                    }
                    rows={6}
                    placeholder={t("project.instructionsPlaceholder")}
                  />
                  <label className="setting-editor-label">
                    {t("project.preferredSkillsLabel")}
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
                          {t(skill.labelKey)}
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
                      {t("apps.cancel")}
                    </button>
                    <button
                      className="setting-action setting-action-primary"
                      onClick={() => void saveAgentProfile()}
                      disabled={profileSaving}
                    >
                      {profileSaving
                        ? t("project.saving")
                        : t("appViewer.save")}
                    </button>
                  </div>
                </div>
              )}
            </div>
            <div className="setting-row">
              <label className="setting-label">
                {t("project.browserBridge")}
              </label>
              <label className="setting-toggle">
                <input
                  type="checkbox"
                  checked={browserOn}
                  onChange={(e) => void setBrowser(e.currentTarget.checked)}
                />
                {browserOn
                  ? t("project.browserEnabled")
                  : t("project.browserDisabled")}
              </label>
              {browserOn && (
                <span className="setting-hint">
                  {t("project.browserHint")}
                </span>
              )}
            </div>
            <div className="setting-row setting-row-block">
              <label className="setting-label">{t("project.connections")}</label>
              <div className="setting-mcp-summary">
                {mcpServerNames.length === 0 ? (
                  <span className="setting-empty">{t("project.none")}</span>
                ) : (
                  mcpServerNames.map((name) => (
                    <span key={name} className="setting-chip">
                      {name}
                    </span>
                  ))
                )}
                <button className="setting-action" onClick={openMcpEditor}>
                  {t("project.editJson")}
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
                      {t("apps.cancel")}
                    </button>
                    <button
                      className="setting-action setting-action-primary"
                      onClick={() => void saveMcpServers()}
                      disabled={mcpSaving}
                    >
                      {mcpSaving ? t("project.saving") : t("appViewer.save")}
                    </button>
                  </div>
                </div>
              )}
            </div>
          </div>
        </details>
      )}

      <section className="project-topics-section">
        <div className="section-head">
          <h2 className="section-title">
            {t("project.topics")}
            {runningCount > 0 && (
              <span className="section-badge running">
                {t("project.runningCount", { count: runningCount })}
              </span>
            )}
          </h2>
          <button
            className="apps-create-btn"
            onClick={() => setShowNewTopic(true)}
          >
            {t("project.newTopic")}
          </button>
        </div>
        {topics.length === 0 ? (
          <div className="section-empty">
            {t("project.noTopics")}
          </div>
        ) : (
          <ul className="topic-list">
            {topics.map((topic) => (
              <li key={topic.id}>
                <button
                  className="topic-row topic-row-with-status"
                  onClick={() => onSelectTopic(topic.id)}
                >
                  <StatusDot
                    done={topic.done}
                    ok={topic.exit_code === 0}
                  />
                  <div className="topic-row-body">
                    <span className="topic-row-prompt">
                      {topic.title ?? topic.prompt}
                    </span>
                    <span className="topic-row-meta">
                      {topic.done
                        ? topic.exit_code === 0
                          ? t("project.done")
                          : `exit ${topic.exit_code ?? "?"}`
                        : t("project.running")}
                      {" · "}
                      {new Date(topic.created_at_ms).toLocaleString()}
                    </span>
                  </div>
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="project-files-section">
        <div className="section-head">
          <h2 className="section-title">{t("project.files")}</h2>
          <label className="section-toggle">
            <input
              type="checkbox"
              checked={showHidden}
              onChange={(e) => setShowHidden(e.currentTarget.checked)}
            />
            {t("project.showHidden")}
          </label>
        </div>
        {visibleEntries.length === 0 ? (
          <div className="section-empty">{t("project.emptyFolder")}</div>
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
                  ? t("project.ragStale")
                  : s.indexed
                    ? `${t("project.inRag")}${
                        s.indexed_under
                          ? ` (${t("project.docsCount", {
                              count: s.indexed_under,
                            })})`
                          : ""
                      }`
                    : ignored
                      ? t("project.notIndexed")
                      : t("project.canIndex");
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
                    title={`${e.path}\n${dotTitle}\n(${t("project.altOpenFinder")})`}
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
            <h2 className="modal-title">{t("project.linkUtilityTitle")}</h2>
            <p className="modal-hint">{t("project.linkUtilityHint")}</p>
            {(() => {
              const linkedIds = new Set(project.apps ?? []);
              const available = installedApps.filter(
                (a) => !linkedIds.has(a.id),
              );
              if (available.length === 0) {
                return (
                  <div className="project-linked-empty">
                    {t("project.allUtilitiesLinked")}
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
                            <span className="apps-card-badge">
                              {t("apps.creatingBadge")}
                            </span>
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
                {t("apps.cancel")}
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
            <h2 className="modal-title">{t("project.newTopicTitle")}</h2>
            <p className="modal-hint">{t("project.newTopicHint")}</p>
            <textarea
              className="modal-input"
              placeholder={t("project.newTopicPlaceholder")}
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
              <span>📋 {t("project.planFirst")}</span>
            </label>
            <div className="modal-actions">
              <button
                className="modal-btn"
                disabled={creatingTopic}
                onClick={() => setShowNewTopic(false)}
              >
                {t("apps.cancel")}
              </button>
              <button
                className="modal-btn modal-btn-primary"
                disabled={creatingTopic || !newTopicPrompt.trim()}
                onClick={() => void submitNewTopic()}
              >
                {creatingTopic
                  ? t("project.starting")
                  : t("project.createShortcut")}
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
  onOpenApp,
}: {
  thread_id: string;
  threads: Thread[];
  projects: Project[];
  onOpenLink?: (
    threadId: string,
    url: string,
    ev: React.MouseEvent<HTMLAnchorElement>,
  ) => void;
  onOpenApp?: (appId: string) => void;
}) {
  const { t } = useI18n();
  const thread = threads.find((t) => t.id === thread_id);
  const project = thread
    ? projects.find((p) => p.id === thread.project_id) ?? null
    : null;
  const projectRoot = project?.root ?? thread?.cwd ?? "";
  const bottomRef = useRef<HTMLDivElement>(null);
  const [showRecall, setShowRecall] = useState(false);
  const [apps, setApps] = useState<AppManifest[]>([]);

  useEffect(() => {
    let alive = true;

    const refreshApps = () => {
      invoke<AppManifest[]>("list_apps")
        .then((list) => {
          if (alive) setApps(list);
        })
        .catch((e) => console.warn("[reflex] list_apps for topic failed", e));
    };

    refreshApps();
    let unlisten: (() => void) | null = null;
    listen("reflex://apps-changed", refreshApps)
      .then((u) => {
        unlisten = u;
      })
      .catch((e) => console.warn("[reflex] listen apps-changed topic", e));
    return () => {
      alive = false;
      unlisten?.();
    };
  }, []);

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
        <p>{t("topic.notFound")}</p>
      </div>
    );
  }

  const recallQuery = mostRecentTopicPrompt(thread);

  return (
    <ol className="chat-list">
      <li className="chat-item-controls">
        <button
          type="button"
          className="header-tab"
          onClick={() => setShowRecall((v) => !v)}
          title={t("topic.memoryToggleTitle")}
        >
          {showRecall ? t("topic.hideMemory") : t("topic.memory")}
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
      <ThreadCard
        thread={thread}
        projectRoot={projectRoot || null}
        apps={apps}
        onOpenLink={onOpenLink}
        onOpenApp={onOpenApp}
      />
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
  projectRoot,
  apps,
  onOpenLink,
  onOpenApp,
}: {
  thread: Thread;
  projectRoot: string | null;
  apps: AppManifest[];
  onOpenLink?: (
    threadId: string,
    url: string,
    ev: React.MouseEvent<HTMLAnchorElement>,
  ) => void;
  onOpenApp?: (appId: string) => void;
}) {
  const { t } = useI18n();
  const [submitting, setSubmitting] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Show the banner only after the agent has produced a plan and the turn is done.
  // Empty or still-running turns keep it hidden.
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
      ? t("project.done")
      : `exit ${thread.exit_code ?? "?"}`
    : t("project.running");

  const running = !thread.done;

  async function sendFollowupText(
    prompt: string,
    imagePaths: string[] = [],
    meta?: TopicComposerSendMeta,
  ) {
    const text = prompt.trim();
    if (!text || submitting) return;
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
      if (meta?.goal) {
        await invoke("set_thread_goal", {
          projectId: thread.project_id,
          threadId: thread.id,
          goal: meta.goal,
        });
      }
      const confirmsPlan = showPlanBanner && isPlanApprovalText(text);
      const args: Record<string, unknown> = {
        projectId: thread.project_id,
        threadId: thread.id,
        prompt: confirmsPlan ? "go - execute the plan as described." : text,
      };
      if (confirmsPlan) args.planConfirmed = true;
      if (imagePaths.length > 0) args.imagePaths = imagePaths;
      await invoke("continue_thread", args);
    } catch (e) {
      console.error("[reflex] continue_thread failed", e);
      setError(String(e));
      throw e;
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
        prompt: "go - execute the plan as described.",
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
            📋 <strong>{t("thread.planMode")}</strong>{" "}
            {t("thread.planBanner")}
          </div>
          <button
            className="appviewer-btn appviewer-btn-primary"
            disabled={submitting}
            onClick={() => void confirmPlan()}
          >
            {submitting ? "..." : `✓ ${t("thread.confirmRun")}`}
          </button>
        </div>
      )}
      <TopicComposer
        threadId={thread.id}
        projectRoot={projectRoot}
        running={running}
        showPlanBanner={showPlanBanner}
        submitting={submitting}
        stopping={stopping}
        apps={apps}
        onSend={sendFollowupText}
        onStop={stopThread}
        onOpenApp={onOpenApp}
      />
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
  const { t } = useI18n();
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
          placeholder={t("thread.answerPlaceholder")}
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
