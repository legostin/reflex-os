export type QuickContext = {
  frontmost_app: string | null;
  finder_target: string | null;
};

export type Project = {
  id: string;
  name: string;
  root: string;
  created_at_ms: number;
  folder_path?: string | null;
  sandbox?: string;
  mcp_servers?: Record<string, any> | null;
  description?: string | null;
  agent_instructions?: string | null;
  skills?: string[];
  apps?: string[];
};

export type ProjectFolder = {
  path: string;
  name: string;
  parent_path?: string | null;
  project_count?: number;
  created_at_ms?: number;
};

export type BrowserTabSnapshot = { url: string; title: string };

export type ThreadEvent = {
  seq: number;
  stream: "stdout" | "stderr" | "error" | "user";
  raw: string;
  parsed: any | null;
};

export type ThreadQuestion = {
  question_id: string;
  method: string;
  params: any;
  thread_id: string | null;
};

export type Thread = {
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

export type StoredEvent = { seq: number; stream: string; ts_ms: number; raw: string };

export type StoredThreadMeta = {
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

export type ProjectThread = {
  project: Project;
  thread: { meta: StoredThreadMeta; events: StoredEvent[] };
};

export type Route =
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

export type AppWidget = {
  id: string;
  name: string;
  entry: string;
  size?: string;
  description?: string | null;
};

export type AppNetworkPolicy = {
  allowed_hosts?: string[];
};

export type AppPermissionRequest = {
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

export type AppStep = {
  method: string;
  params?: any;
  save_as?: string | null;
};

export type AppSchedule = {
  id: string;
  name: string;
  cron: string;
  enabled?: boolean;
  catch_up?: string;
  steps?: AppStep[];
};

export type AppAction = {
  id: string;
  name: string;
  description?: string | null;
  params_schema?: any;
  paramsSchema?: any;
  public?: boolean;
  steps?: AppStep[];
};

export type AppSelfTestCheck = {
  name: string;
  status: string;
  message?: string | null;
};

export type AppSelfTestStatus = {
  status: string;
  message?: string | null;
  started_at_ms?: number | null;
  finished_at_ms?: number | null;
  checks?: AppSelfTestCheck[];
};

export type AppManifest = {
  id: string;
  name: string;
  icon?: string | null;
  description?: string | null;
  entry: string;
  permissions: string[];
  kind: string;
  created_at_ms: number;
  folder_path?: string | null;
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
  self_test?: AppSelfTestStatus | null;
};

export type AppFolder = {
  path: string;
  name: string;
  parent_path?: string | null;
  created_at_ms?: number;
};
