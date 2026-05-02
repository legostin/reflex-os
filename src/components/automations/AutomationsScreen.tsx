import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { ScheduleListItem, SchedulerStats } from "./types";
import { RunHistoryView } from "./RunHistoryView";
import { RunDetailPanel } from "./RunDetailPanel";
import { useI18n, type Translate } from "../../i18n";
import "./automations.css";

type Tab = "schedules" | "history";

export function AutomationsScreen({
  onCreateAutomation,
}: {
  onCreateAutomation?: () => void;
}) {
  const { t } = useI18n();
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
        <h1>{t("nav.automations")}</h1>
        <div className="automations-header-actions">
          {onCreateAutomation && (
            <button
              className="automations-primary-btn"
              onClick={onCreateAutomation}
            >
              {t("automations.create")}
            </button>
          )}
          <div className="automations-tabs">
            <button
              className={tab === "schedules" ? "tab-on" : ""}
              onClick={() => setTab("schedules")}
            >
              {t("automations.schedules")}
            </button>
            <button
              className={tab === "history" ? "tab-on" : ""}
              onClick={() => setTab("history")}
            >
              {t("automations.history")}
            </button>
          </div>
        </div>
      </header>

      {error && <div className="automations-error">{error}</div>}

      <section
        className="automations-summary"
        aria-label={t("automations.summaryAria")}
      >
        <SummaryCard
          label={t("automations.total")}
          value={stats.total}
          detail={t("automations.enabledDetail", { count: stats.enabled })}
        />
        <SummaryCard label={t("automations.active")} value={stats.active} tone="ok" />
        <SummaryCard label={t("automations.running")} value={stats.running} tone="run" />
        <SummaryCard label={t("automations.paused")} value={stats.paused} />
        <SummaryCard label={t("automations.cronErrors")} value={stats.invalid} tone="bad" />
        <SummaryCard
          label={t("automations.nextRun")}
          value={formatCompactDateTime(stats.nextFireMs)}
          detail={formatFullDateTime(stats.nextFireMs, t)}
        />
        <SummaryCard
          label={t("automations.runErrors")}
          value={stats.recentErrors}
          detail={
            lastError
              ? t("automations.lastError", { target: lastErrorTarget ?? "?" })
              : t("automations.successRatio", {
                  ok: stats.recentOk,
                  sample: stats.recentSample,
                })
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
                {t("automations.noSchedules")}
              </div>
              {onCreateAutomation && (
                <button
                  className="automations-primary-btn"
                  onClick={onCreateAutomation}
                >
                  {t("automations.createAutomation")}
                </button>
              )}
            </div>
          ) : (
            <table className="automations-table">
              <thead>
                <tr>
                  <th>{t("automations.state")}</th>
                  <th>{t("automations.utilitySchedule")}</th>
                  <th>cron</th>
                  <th>{t("automations.nextRun")}</th>
                  <th>{t("automations.lastRun")}</th>
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

function formatFullDateTime(
  ms: number | null | undefined,
  t: Translate,
) {
  if (!ms) return t("automations.noActiveSchedules");
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
  const { t } = useI18n();
  const stateLabel = !s.valid
    ? t("automations.stateCronError")
    : s.paused
      ? t("automations.statePaused")
      : isRunning
        ? t("automations.stateRunning")
        : t("automations.stateActive");
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
          title={s.paused ? t("automations.resume") : t("automations.pause")}
        >
          {s.paused ? "▶" : "⏸"}
        </button>
        <button
          className="icon-btn"
          disabled={!s.valid || s.paused || isRunning}
          onClick={() => onRunNow(s.schedule_id)}
          title={t("automations.runNow")}
        >
          ⚡
        </button>
      </td>
    </tr>
  );
}

export default AutomationsScreen;
