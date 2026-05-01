import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
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

type Tab = "general" | "logs";

export function SettingsScreen() {
  const [tab, setTab] = useState<Tab>("logs");
  return (
    <div className="settings-root">
      <header className="settings-header">
        <h1>Настройки</h1>
        <div className="settings-tabs">
          <button
            className={tab === "general" ? "tab-on" : ""}
            onClick={() => setTab("general")}
          >
            Общие
          </button>
          <button
            className={tab === "logs" ? "tab-on" : ""}
            onClick={() => setTab("logs")}
          >
            Логи и события
          </button>
        </div>
      </header>
      {tab === "general" ? <GeneralPane /> : <LogsPane />}
    </div>
  );
}

function GeneralPane() {
  return (
    <div className="settings-pane">
      <section className="settings-section">
        <h2>О приложении</h2>
        <p>
          Reflex — macOS-агент-надстройка с локальным Codex CLI, встроенным
          браузером и системой памяти.
        </p>
      </section>
      <section className="settings-section">
        <h2>Действия</h2>
        <p className="settings-hint">
          Заглушка. Скоро добавим переключатели для Codex CLI, Ollama,
          параметров браузера и т.д.
        </p>
      </section>
    </div>
  );
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
