import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  BRIDGE_API_GROUPS,
  BRIDGE_HELPER_GROUPS,
  BRIDGE_RECIPE_CARDS,
} from "../../appBridgeCatalog";
import "./settings.css";

type LogLevel = "trace" | "debug" | "info" | "warn" | "error";

interface LogEntry {
  seq: number;
  ts_ms: number;
  level: LogLevel;
  source: string;
  message: string;
}

const LEVEL_ORDER: Record<LogLevel, number> = {
  trace: 0,
  debug: 1,
  info: 2,
  warn: 3,
  error: 4,
};

type Tab = "capabilities" | "logs";

const CAPABILITY_GROUPS = [
  {
    title: "Проекты",
    body: "Папки с sandbox, browser MCP, MCP servers, agent profile, preferred skills, linked apps, widgets и indexed files.",
  },
  {
    title: "Топики",
    body: "Codex threads с project profile, memory recall и продолжением рабочей сессии.",
  },
  {
    title: "Генерируемые утилиты",
    body: "Static или local server apps с manifest, storage, actions, widgets и Reflex bridge APIs.",
  },
  {
    title: "Память",
    body: "Global, project и topic notes, плюс RAG по индексированным файлам и сохранённым фактам.",
  },
  {
    title: "Автоматизации",
    body: "Manifest schedules и actions, которые исполняются теми же bridge methods, что доступны apps.",
  },
  {
    title: "MCP и skills",
    body: "Project-scoped MCP JSON и preferred skills внедряются в новые, продолженные и auto-resumed topics.",
  },
] as const;

const PERMISSION_EXAMPLES = [
  "agent.project:<project>",
  "agent.project:*",
  "agent.cwd:*",
  "memory.global.read",
  "memory.global.write",
  "memory.project:*",
  "projects.read:*",
  "projects.write:<project>",
  "projects.write:*",
  "topics.read:<project>",
  "topics.read:*",
  "skills.read:<project>",
  "skills.read:*",
  "skills.write:<project>",
  "skills.write:*",
  "mcp.read:<project>",
  "mcp.read:*",
  "mcp.write:<project>",
  "mcp.write:*",
  "project.files.read:<project>",
  "project.files.read:*",
  "project.files.write:<project>",
  "project.files.write:*",
  "browser.read",
  "browser.control",
  "browser.project:<project>",
  "apps.create",
  "apps.manage",
  "apps.invoke:*",
  "apps.invoke:<app>",
  "scheduler.read:*",
  "scheduler.run:<app>",
  "scheduler.write:<app>::<schedule>",
  "net.fetch requires manifest.network.allowed_hosts",
] as const;

const BRIDGE_API_COUNT = BRIDGE_API_GROUPS.reduce(
  (sum, group) => sum + group.methods.length,
  0,
);

const BRIDGE_HELPER_COUNT = BRIDGE_HELPER_GROUPS.reduce(
  (sum, group) => sum + group.helpers.length,
  0,
);

const PERMISSION_COUNT = PERMISSION_EXAMPLES.length;

const SYSTEM_STATS = [
  {
    label: "Bridge methods",
    value: BRIDGE_API_COUNT,
    detail: "dispatch API",
  },
  {
    label: "Overlay helpers",
    value: BRIDGE_HELPER_COUNT,
    detail: "window.reflex*",
  },
  {
    label: "Patterns",
    value: BRIDGE_RECIPE_CARDS.length,
    detail: "рабочие связки",
  },
  {
    label: "Permission forms",
    value: PERMISSION_COUNT,
    detail: "manifest grants",
  },
] as const;

export function SettingsScreen() {
  const [tab, setTab] = useState<Tab>("capabilities");
  return (
    <div className="settings-root">
      <header className="settings-header">
        <h1>Настройки</h1>
        <div className="settings-tabs">
          <button
            className={tab === "capabilities" ? "tab-on" : ""}
            onClick={() => setTab("capabilities")}
          >
            Возможности
          </button>
          <button
            className={tab === "logs" ? "tab-on" : ""}
            onClick={() => setTab("logs")}
          >
            Логи и события
          </button>
        </div>
      </header>
      {tab === "capabilities" ? <CapabilitiesPane /> : <LogsPane />}
    </div>
  );
}

function CapabilitiesPane() {
  const [bridgeQuery, setBridgeQuery] = useState("");
  const [copiedToken, setCopiedToken] = useState<string | null>(null);
  const normalizedBridgeQuery = bridgeQuery.trim().toLowerCase();

  const visibleApiGroups = useMemo(() => {
    if (!normalizedBridgeQuery) return BRIDGE_API_GROUPS;
    return BRIDGE_API_GROUPS.map((group) => ({
      ...group,
      methods: group.methods.filter((method) =>
        method.toLowerCase().includes(normalizedBridgeQuery),
      ),
    })).filter((group) => group.methods.length > 0);
  }, [normalizedBridgeQuery]);

  const visibleHelperGroups = useMemo(() => {
    if (!normalizedBridgeQuery) return BRIDGE_HELPER_GROUPS;
    return BRIDGE_HELPER_GROUPS.map((group) => ({
      ...group,
      helpers: group.helpers.filter((helper) =>
        helper.toLowerCase().includes(normalizedBridgeQuery),
      ),
    })).filter((group) => group.helpers.length > 0);
  }, [normalizedBridgeQuery]);

  const visibleRecipeCards = useMemo(() => {
    if (!normalizedBridgeQuery) return BRIDGE_RECIPE_CARDS;
    return BRIDGE_RECIPE_CARDS.filter((recipe) =>
      [recipe.title, recipe.body, recipe.example, ...recipe.calls]
        .join(" ")
        .toLowerCase()
        .includes(normalizedBridgeQuery),
    );
  }, [normalizedBridgeQuery]);

  const visiblePermissionExamples = useMemo(() => {
    if (!normalizedBridgeQuery) return PERMISSION_EXAMPLES;
    return PERMISSION_EXAMPLES.filter((permission) =>
      permission.toLowerCase().includes(normalizedBridgeQuery),
    );
  }, [normalizedBridgeQuery]);

  const visibleApiCount = visibleApiGroups.reduce(
    (sum, group) => sum + group.methods.length,
    0,
  );
  const visibleHelperCount = visibleHelperGroups.reduce(
    (sum, group) => sum + group.helpers.length,
    0,
  );

  async function copyToken(value: string) {
    try {
      await copyTextToClipboard(value);
      setCopiedToken(value);
      window.setTimeout(() => {
        setCopiedToken((current) => (current === value ? null : current));
      }, 1200);
    } catch (e) {
      console.warn("[reflex] settings copy failed", e);
    }
  }

  return (
    <div className="settings-pane capabilities-pane">
      <section className="settings-section">
        <h2>Слой Reflex OS</h2>
        <p>
          Reflex — локальная macOS-надстройка над Codex CLI: проекты, темы,
          browser/MCP bridge, generated apps, widgets, memory, RAG и scheduled
          automations живут в одном workspace.
        </p>
      </section>

      <div className="settings-stat-grid" aria-label="Reflex OS summary">
        {SYSTEM_STATS.map((stat) => (
          <article className="settings-stat-card" key={stat.label}>
            <span>{stat.label}</span>
            <strong>{stat.value}</strong>
            <small>{stat.detail}</small>
          </article>
        ))}
      </div>

      <section className="settings-section settings-section-open">
        <h2>Карта системы</h2>
        <div className="settings-cap-grid">
          {CAPABILITY_GROUPS.map((group) => (
            <article className="settings-cap-card" key={group.title}>
              <h3>{group.title}</h3>
              <p>{group.body}</p>
            </article>
          ))}
        </div>
      </section>

      <section className="settings-section settings-section-open">
        <div className="settings-section-title-row">
          <h2>Bridge generated apps</h2>
          <div className="settings-section-controls">
            <input
              className="settings-bridge-search"
              placeholder="Поиск API, helpers, permissions…"
              value={bridgeQuery}
              onChange={(e) => setBridgeQuery(e.currentTarget.value)}
            />
            <span className="settings-section-meta">
              {visibleApiCount}/{BRIDGE_API_COUNT} methods
            </span>
          </div>
        </div>
        {visibleApiGroups.length === 0 ? (
          <div className="settings-empty-inline">Нет совпадений.</div>
        ) : (
          <div className="settings-api-grid">
            {visibleApiGroups.map((group) => (
              <article className="settings-api-group" key={group.title}>
                <h3>{group.title}</h3>
                <div className="settings-method-list">
                  {group.methods.map((method) => (
                    <CopyToken
                      key={method}
                      copied={copiedToken === method}
                      value={method}
                      onCopy={copyToken}
                    />
                  ))}
                </div>
              </article>
            ))}
          </div>
        )}
      </section>

      <section className="settings-section settings-section-open">
        <div className="settings-section-title-row">
          <h2>Рабочие связки bridge</h2>
          <span className="settings-section-meta">
            {visibleRecipeCards.length}/{BRIDGE_RECIPE_CARDS.length} patterns
          </span>
        </div>
        {visibleRecipeCards.length === 0 ? (
          <div className="settings-empty-inline">Нет совпадений.</div>
        ) : (
          <div className="settings-recipe-grid">
            {visibleRecipeCards.map((recipe) => (
              <article className="settings-recipe-card" key={recipe.title}>
                <h3>{recipe.title}</h3>
                <p>{recipe.body}</p>
                <div className="settings-method-list">
                  {recipe.calls.map((call) => (
                    <CopyToken
                      key={call}
                      copied={copiedToken === call}
                      value={call}
                      onCopy={copyToken}
                    />
                  ))}
                </div>
                <CopyToken
                  copied={copiedToken === recipe.example}
                  value={recipe.example}
                  onCopy={copyToken}
                  variant="example"
                />
              </article>
            ))}
          </div>
        )}
      </section>

      <section className="settings-section settings-section-open">
        <div className="settings-section-title-row">
          <h2>Runtime overlay helpers</h2>
          <span className="settings-section-meta">
            {visibleHelperCount}/{BRIDGE_HELPER_COUNT} helpers
          </span>
        </div>
        {visibleHelperGroups.length === 0 ? (
          <div className="settings-empty-inline">Нет совпадений.</div>
        ) : (
          <div className="settings-helper-grid">
            {visibleHelperGroups.map((group) => (
              <article className="settings-api-group" key={group.title}>
                <h3>{group.title}</h3>
                <div className="settings-method-list">
                  {group.helpers.map((helper) => (
                    <CopyToken
                      key={helper}
                      copied={copiedToken === helper}
                      value={helper}
                      onCopy={copyToken}
                    />
                  ))}
                </div>
              </article>
            ))}
          </div>
        )}
        <p className="settings-hint">
          Generated apps should prefer these helpers over manual postMessage;
          permissions and manifest.network rules still apply to the underlying
          bridge method.
        </p>
      </section>

      <section className="settings-section">
        <div className="settings-section-title-row">
          <h2>Разрешения</h2>
          <span className="settings-section-meta">
            {visiblePermissionExamples.length}/{PERMISSION_COUNT} manifest grants
          </span>
        </div>
        {visiblePermissionExamples.length === 0 ? (
          <div className="settings-empty-inline">Нет совпадений.</div>
        ) : (
          <div className="settings-token-list">
            {visiblePermissionExamples.map((permission) => (
              <CopyToken
                key={permission}
                copied={copiedToken === permission}
                value={permission}
                onCopy={copyToken}
                variant="permission"
              />
            ))}
          </div>
        )}
      </section>

      <section className="settings-section">
        <h2>Поток автоматизации</h2>
        <div className="settings-flow">
          <span>manifest.schedules</span>
          <span>scheduler runner</span>
          <span>bridge steps</span>
          <span>run history</span>
        </div>
        <p className="settings-hint">
          Generated apps могут обновлять собственный manifest, добавлять
          schedules/actions, смотреть runs и отдавать widgets или public
          actions другим apps.
        </p>
      </section>
    </div>
  );
}

function CopyToken({
  value,
  copied,
  onCopy,
  variant = "token",
}: {
  value: string;
  copied: boolean;
  onCopy: (value: string) => void | Promise<void>;
  variant?: "token" | "permission" | "example";
}) {
  return (
    <button
      className={`settings-copy-token settings-copy-${variant} ${copied ? "copied" : ""}`}
      onClick={() => void onCopy(value)}
      title={copied ? "Copied" : "Copy"}
      type="button"
    >
      {value}
    </button>
  );
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

function LogsPane() {
  const [entries, setEntries] = useState<LogEntry[]>([]);
  const [paused, setPaused] = useState(false);
  const [filterText, setFilterText] = useState("");
  const [minLevel, setMinLevel] = useState<LogLevel>("info");
  const [filterSource, setFilterSource] = useState<string>("all");
  const lastSeqRef = useRef<number>(0);
  const pausedRef = useRef(false);
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    pausedRef.current = paused;
  }, [paused]);

  useEffect(() => {
    let alive = true;
    invoke<LogEntry[]>("logs_get", { limit: 500 })
      .then((arr) => {
        if (!alive) return;
        setEntries(arr);
        if (arr.length > 0) lastSeqRef.current = arr[arr.length - 1].seq;
      })
      .catch(() => {});
    const u = listen<LogEntry>("reflex://logs/append", (ev) => {
      if (!ev.payload) return;
      lastSeqRef.current = ev.payload.seq;
      if (pausedRef.current) return;
      setEntries((prev) => {
        const next = [...prev, ev.payload];
        if (next.length > 2000) next.splice(0, next.length - 2000);
        return next;
      });
    });
    return () => {
      alive = false;
      u.then((un) => un());
    };
  }, []);

  useEffect(() => {
    if (paused) return;
    const el = listRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [entries, paused]);

  const sources = useMemo(() => {
    const set = new Set<string>();
    entries.forEach((e) => set.add(e.source));
    return Array.from(set).sort();
  }, [entries]);

  const visible = useMemo(() => {
    const minOrder = LEVEL_ORDER[minLevel];
    const text = filterText.trim().toLowerCase();
    return entries.filter((e) => {
      if (LEVEL_ORDER[e.level] < minOrder) return false;
      if (filterSource !== "all" && e.source !== filterSource) return false;
      if (text && !e.message.toLowerCase().includes(text)) return false;
      return true;
    });
  }, [entries, filterText, minLevel, filterSource]);

  return (
    <div className="settings-pane logs-pane">
      <div className="logs-toolbar">
        <select
          value={minLevel}
          onChange={(e) => setMinLevel(e.currentTarget.value as LogLevel)}
        >
          <option value="trace">trace+</option>
          <option value="debug">debug+</option>
          <option value="info">info+</option>
          <option value="warn">warn+</option>
          <option value="error">error</option>
        </select>
        <select
          value={filterSource}
          onChange={(e) => setFilterSource(e.currentTarget.value)}
        >
          <option value="all">все источники</option>
          {sources.map((s) => (
            <option key={s} value={s}>
              {s}
            </option>
          ))}
        </select>
        <input
          className="logs-search"
          placeholder="Поиск по тексту…"
          value={filterText}
          onChange={(e) => setFilterText(e.currentTarget.value)}
        />
        <button onClick={() => setPaused((p) => !p)}>
          {paused ? "▶ Возобновить" : "⏸ Пауза"}
        </button>
        <button
          onClick={() => setEntries([])}
          title="Очистить вид (буфер бэка не трогается)"
        >
          Очистить
        </button>
        <span className="logs-count">{visible.length} строк</span>
      </div>
      <div className="logs-list" ref={listRef}>
        {visible.length === 0 ? (
          <div className="logs-empty">Логов нет.</div>
        ) : (
          visible.map((e) => (
            <div key={e.seq} className={`log-row log-${e.level}`}>
              <span className="log-time">
                {new Date(e.ts_ms).toLocaleTimeString()}
              </span>
              <span className={`log-level log-level-${e.level}`}>
                {e.level}
              </span>
              <span className="log-source">{e.source}</span>
              <span className="log-msg">{e.message}</span>
            </div>
          ))
        )}
      </div>
    </div>
  );
}

export default SettingsScreen;
