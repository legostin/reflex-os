import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { ScheduleListItem, SchedulerStats } from "./types";
import { RunHistoryView } from "./RunHistoryView";
import { RunDetailPanel } from "./RunDetailPanel";
import "./automations.css";

type Tab = "schedules" | "history";

export function AutomationsScreen({
  onCreateAutomation,
}: {
  onCreateAutomation?: () => void;
}) {
  const [tab, setTab] = useState<Tab>("schedules");
  const [items, setItems] = useState<ScheduleListItem[]>([]);
  const [schedulerStats, setSchedulerStats] = useState<SchedulerStats | null>(
    null,
  );
  const [running, setRunning] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const [tick, setTick] = useState(0);
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    Promise.all([
      invoke<ScheduleListItem[]>("scheduler_list"),
      invoke<SchedulerStats>("scheduler_stats", { recentLimit: 200 }),
    ])
      .then(([arr, nextStats]) => {
        if (!alive) return;
        setItems(arr);
        setSchedulerStats(nextStats);
        setError(null);
      })
      .catch((e) => alive && setError(String(e)));
    return () => {
      alive = false;
    };
  }, [tick]);

  useEffect(() => {
    const subs: Array<Promise<() => void>> = [];
    subs.push(
      listen<{ schedule_id: string; run_id: string }>(
        "reflex://scheduler-fire-started",
        (ev) => {
          if (!ev.payload?.schedule_id) return;
          setRunning((s) => new Set(s).add(ev.payload.schedule_id));
        },
      ),
    );
    subs.push(
      listen<{ schedule_id: string }>(
        "reflex://scheduler-fire-finished",
        (ev) => {
          if (ev.payload?.schedule_id) {
            setRunning((s) => {
              const n = new Set(s);
              n.delete(ev.payload.schedule_id!);
              return n;
            });
          }
          setTick((n) => n + 1);
        },
      ),
    );
    subs.push(
      listen("reflex://scheduler-state-changed", () => setTick((n) => n + 1)),
    );
    return () => {
      subs.forEach((p) => p.then((u) => u()));
    };
  }, []);

  async function setPaused(id: string, paused: boolean) {
    setError(null);
    try {
      await invoke("scheduler_set_paused", { scheduleId: id, paused });
      setTick((n) => n + 1);
    } catch (e) {
      setError(String(e));
    }
  }

  async function runNow(id: string) {
    setError(null);
    try {
      await invoke("scheduler_run_now", { scheduleId: id });
    } catch (e) {
      setError(String(e));
    }
  }

  const sortedItems = useMemo(
    () =>
      [...items].sort((a, b) => {
        const an = a.next_fire_ms ?? Number.POSITIVE_INFINITY;
        const bn = b.next_fire_ms ?? Number.POSITIVE_INFINITY;
        return an - bn;
      }),
    [items],
  );
  const stats = useMemo(() => {
    let enabled = 0;
    let active = 0;
    let paused = 0;
    let invalid = 0;
    let nextFireMs: number | null = null;
    for (const item of items) {
      if (item.enabled) enabled += 1;
      if (!item.valid) {
        invalid += 1;
      } else if (item.paused) {
        paused += 1;
      } else if (item.enabled) {
        active += 1;
        if (
          item.next_fire_ms != null &&
          (nextFireMs == null || item.next_fire_ms < nextFireMs)
        ) {
          nextFireMs = item.next_fire_ms;
        }
      }
    }
    const scheduleStats = schedulerStats?.schedules;
    const runStats = schedulerStats?.recent_runs;
    return {
      total: scheduleStats?.total ?? items.length,
      enabled: scheduleStats?.enabled ?? enabled,
      active: scheduleStats?.active ?? active,
      paused: scheduleStats?.paused ?? paused,
      invalid: scheduleStats?.invalid ?? invalid,
      nextFireMs: scheduleStats?.next_fire_ms ?? nextFireMs,
      running: running.size,
      recentSample: runStats?.sample ?? 0,
      recentOk: runStats?.ok ?? 0,
      recentErrors: runStats?.error ?? 0,
      lastError: runStats?.last_error ?? null,
    };
  }, [items, running, schedulerStats]);

  const lastError = stats.lastError;
  const lastErrorTarget = lastError
    ? lastError.schedule_id ?? lastError.action_id ?? lastError.app_id
    : null;

  return (
    <div className="automations-root">
      <header className="automations-header">
        <h1>Автоматизации</h1>
        <div className="automations-header-actions">
          {onCreateAutomation && (
            <button
              className="automations-primary-btn"
              onClick={onCreateAutomation}
            >
              + Автоматизация
            </button>
          )}
          <div className="automations-tabs">
            <button
              className={tab === "schedules" ? "tab-on" : ""}
              onClick={() => setTab("schedules")}
            >
              Расписания
            </button>
            <button
              className={tab === "history" ? "tab-on" : ""}
              onClick={() => setTab("history")}
            >
              История запусков
            </button>
          </div>
        </div>
      </header>

      {error && <div className="automations-error">{error}</div>}

      <section className="automations-summary" aria-label="Сводка автоматизаций">
        <SummaryCard
          label="Всего"
          value={stats.total}
          detail={`${stats.enabled} включено`}
        />
        <SummaryCard label="Активные" value={stats.active} tone="ok" />
        <SummaryCard label="Запущены" value={stats.running} tone="run" />
        <SummaryCard label="На паузе" value={stats.paused} />
        <SummaryCard label="Ошибки cron" value={stats.invalid} tone="bad" />
        <SummaryCard
          label="Следующий запуск"
          value={formatCompactDateTime(stats.nextFireMs)}
          detail={formatFullDateTime(stats.nextFireMs)}
        />
        <SummaryCard
          label="Ошибки запусков"
          value={stats.recentErrors}
          detail={
            lastError
              ? `Последняя: ${lastErrorTarget}`
              : `${stats.recentOk}/${stats.recentSample} успешно`
          }
          tone={stats.recentErrors > 0 ? "bad" : "ok"}
          onClick={
            lastError ? () => setSelectedRunId(lastError.run_id) : undefined
          }
          title={lastError?.error_preview ?? undefined}
        />
      </section>

      {tab === "schedules" && (
        <section className="automations-list">
          {sortedItems.length === 0 ? (
            <div className="automations-empty automations-empty-panel">
              <div>
                Расписаний нет. Создай утилиту из шаблона «Автоматизация», и
                Reflex сам добавит <code>schedules</code> в её manifest.
              </div>
              {onCreateAutomation && (
                <button
                  className="automations-primary-btn"
                  onClick={onCreateAutomation}
                >
                  Создать автоматизацию
                </button>
              )}
            </div>
          ) : (
            <table className="automations-table">
              <thead>
                <tr>
                  <th>Состояние</th>
                  <th>Утилита / расписание</th>
                  <th>cron</th>
                  <th>Следующий запуск</th>
                  <th>Последний</th>
                  <th></th>
                </tr>
              </thead>
              <tbody>
                {sortedItems.map((s) => (
                  <ScheduleRow
                    key={s.schedule_id}
                    s={s}
                    isRunning={running.has(s.schedule_id)}
                    onSetPaused={setPaused}
                    onRunNow={runNow}
                    onOpenLastRun={() =>
                      s.last_run_id && setSelectedRunId(s.last_run_id)
                    }
                  />
                ))}
              </tbody>
            </table>
          )}
        </section>
      )}

      {tab === "history" && (
        <RunHistoryView onSelect={(id) => setSelectedRunId(id)} />
      )}

      {selectedRunId && (
        <RunDetailPanel
          runId={selectedRunId}
          onClose={() => setSelectedRunId(null)}
        />
      )}
    </div>
  );
}

function SummaryCard({
  label,
  value,
  detail,
  tone,
  onClick,
  title,
}: {
  label: string;
  value: number | string;
  detail?: string;
  tone?: "ok" | "run" | "bad";
  onClick?: () => void;
  title?: string;
}) {
  const className = `automations-summary-card ${tone ? `tone-${tone}` : ""} ${
    onClick ? "is-action" : ""
  }`;
  const content = (
    <>
      <span>{label}</span>
      <strong>{value}</strong>
      {detail && <small>{detail}</small>}
    </>
  );

  if (onClick) {
    return (
      <button
        className={className}
        onClick={onClick}
        title={title}
        type="button"
      >
        {content}
      </button>
    );
  }

  return (
    <div className={className} title={title}>
      {content}
    </div>
  );
}

function formatCompactDateTime(ms: number | null | undefined) {
  if (!ms) return "—";
  return new Date(ms).toLocaleString(undefined, {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function formatFullDateTime(ms: number | null | undefined) {
  if (!ms) return "нет активных расписаний";
  return new Date(ms).toLocaleString();
}

function ScheduleRow({
  s,
  isRunning,
  onSetPaused,
  onRunNow,
  onOpenLastRun,
}: {
  s: ScheduleListItem;
  isRunning: boolean;
  onSetPaused: (id: string, paused: boolean) => void;
  onRunNow: (id: string) => void;
  onOpenLastRun: () => void;
}) {
  const stateLabel = !s.valid
    ? "ошибка cron"
    : s.paused
      ? "пауза"
      : isRunning
        ? "идёт запуск"
        : "активно";
  const stateClass = !s.valid
    ? "row-invalid"
    : s.paused
      ? "row-paused"
      : isRunning
        ? "row-running"
        : "row-active";
  return (
    <tr className={stateClass}>
      <td>
        <span className={`dot ${stateClass}`} /> {stateLabel}
      </td>
      <td>
        <div className="row-app">{s.app_name}</div>
        <div className="row-schedule">
          {s.name} <span className="row-id">({s.schedule_id})</span>
        </div>
      </td>
      <td>
        <code>{s.cron}</code>
      </td>
      <td>
        {s.valid && s.next_fire_ms
          ? new Date(s.next_fire_ms).toLocaleString()
          : "—"}
      </td>
      <td>
        {s.last_fire_at_ms ? (
          <button
            className="link-btn"
            onClick={onOpenLastRun}
            disabled={!s.last_run_id}
            title={s.last_run_id ?? ""}
          >
            {new Date(s.last_fire_at_ms).toLocaleString()}
          </button>
        ) : (
          "—"
        )}
      </td>
      <td className="row-actions">
        <button
          className="icon-btn"
          onClick={() => onSetPaused(s.schedule_id, !s.paused)}
          title={s.paused ? "Возобновить" : "Поставить на паузу"}
        >
          {s.paused ? "▶" : "⏸"}
        </button>
        <button
          className="icon-btn"
          disabled={!s.valid || s.paused || isRunning}
          onClick={() => onRunNow(s.schedule_id)}
          title="Запустить сейчас"
        >
          ⚡
        </button>
      </td>
    </tr>
  );
}

export default AutomationsScreen;
