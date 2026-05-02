import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { RunSummary } from "./types";
import { callerLabel, runStatusLabel } from "./labels";
import { useI18n } from "../../i18n";

export function RunHistoryView({
  onSelect,
}: {
  onSelect: (runId: string) => void;
}) {
  const { t } = useI18n();
  const [runs, setRuns] = useState<RunSummary[]>([]);
  const [tick, setTick] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [statusFilter, setStatusFilter] = useState<string>("all");
  const [appFilter, setAppFilter] = useState<string>("all");
  const [query, setQuery] = useState<string>("");

  useEffect(() => {
    let alive = true;
    invoke<RunSummary[]>("scheduler_runs", { limit: 200 })
      .then((arr) => alive && setRuns(arr))
      .catch((e) => alive && setError(String(e)));
    return () => {
      alive = false;
    };
  }, [tick]);

  useEffect(() => {
    const u = listen("reflex://scheduler-fire-finished", () =>
      setTick((n) => n + 1),
    );
    return () => {
      u.then((un) => un());
    };
  }, []);

  const appOptions = useMemo(() => {
    return Array.from(new Set(runs.map((r) => r.app_id))).sort((a, b) =>
      a.localeCompare(b),
    );
  }, [runs]);
  const statusOptions = useMemo(() => {
    return Array.from(new Set(runs.map((r) => r.status))).sort((a, b) =>
      a.localeCompare(b),
    );
  }, [runs]);
  const filteredRuns = useMemo(() => {
    const q = query.trim().toLowerCase();
    return runs.filter((r) => {
      if (statusFilter !== "all" && r.status !== statusFilter) return false;
      if (appFilter !== "all" && r.app_id !== appFilter) return false;
      if (!q) return true;
      const hay = [
        r.run_id,
        r.app_id,
        r.schedule_id ?? "",
        r.action_id ?? "",
        r.caller,
        r.status,
        r.error_preview ?? "",
      ]
        .join(" ")
        .toLowerCase();
      return hay.includes(q);
    });
  }, [appFilter, query, runs, statusFilter]);
  const hasFilters =
    statusFilter !== "all" || appFilter !== "all" || query.trim().length > 0;

  return (
    <section className="automations-runs">
      <div className="automations-toolbar">
        <select
          className="automations-select"
          value={statusFilter}
          onChange={(e) => setStatusFilter(e.currentTarget.value)}
          aria-label={t("automations.statusFilterAria")}
        >
          <option value="all">{t("automations.allStatuses")}</option>
          {statusOptions.map((status) => (
            <option key={status} value={status}>
              {runStatusLabel(status, t)}
            </option>
          ))}
        </select>
        <select
          className="automations-select"
          value={appFilter}
          onChange={(e) => setAppFilter(e.currentTarget.value)}
          aria-label={t("automations.appFilterAria")}
        >
          <option value="all">{t("automations.allUtilities")}</option>
          {appOptions.map((appId) => (
            <option key={appId} value={appId}>
              {appId}
            </option>
          ))}
        </select>
        <input
          className="automations-search"
          type="search"
          value={query}
          placeholder={t("automations.searchRuns")}
          onChange={(e) => setQuery(e.currentTarget.value)}
        />
        <button
          className="automations-secondary-btn"
          type="button"
          onClick={() => setTick((n) => n + 1)}
        >
          {t("automations.refresh")}
        </button>
        <span className="automations-count">
          {filteredRuns.length} / {runs.length}
        </span>
      </div>
      {error && <div className="automations-error">{error}</div>}
      {runs.length === 0 ? (
        <div className="automations-empty">{t("automations.noRuns")}</div>
      ) : filteredRuns.length === 0 ? (
        <div className="automations-empty">
          {hasFilters
            ? t("automations.noRunsForFilters")
            : t("automations.noRuns")}
        </div>
      ) : (
        <table className="automations-table">
          <thead>
            <tr>
              <th>{t("automations.time")}</th>
              <th>{t("automations.utility")}</th>
              <th>{t("automations.scheduleAction")}</th>
              <th>{t("automations.caller")}</th>
              <th>{t("automations.status")}</th>
              <th>{t("automations.duration")}</th>
            </tr>
          </thead>
          <tbody>
            {filteredRuns.map((r) => (
              <tr
                key={r.run_id}
                onClick={() => onSelect(r.run_id)}
                className={`run-row run-${r.status}`}
              >
                <td>{new Date(r.started_ms).toLocaleString()}</td>
                <td>{r.app_id}</td>
                <td>
                  <code>{r.schedule_id ?? r.action_id ?? "—"}</code>
                </td>
                <td>{callerLabel(r.caller, t)}</td>
                <td>
                  <span className={`pill pill-${r.status}`}>
                    {runStatusLabel(r.status, t)}
                  </span>
                  {r.error_preview && (
                    <div className="run-err">{r.error_preview}</div>
                  )}
                </td>
                <td>
                  {r.ended_ms
                    ? t("automations.ms", {
                        count: r.ended_ms - r.started_ms,
                      })
                    : "..."}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}
