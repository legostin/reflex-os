import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { RunSummary } from "./types";

export function RunHistoryView({
  onSelect,
}: {
  onSelect: (runId: string) => void;
}) {
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
          aria-label="Фильтр статуса запуска"
        >
          <option value="all">Все статусы</option>
          {statusOptions.map((status) => (
            <option key={status} value={status}>
              {status}
            </option>
          ))}
        </select>
        <select
          className="automations-select"
          value={appFilter}
          onChange={(e) => setAppFilter(e.currentTarget.value)}
          aria-label="Фильтр app"
        >
          <option value="all">Все apps</option>
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
          placeholder="run, app, schedule, error..."
          onChange={(e) => setQuery(e.currentTarget.value)}
        />
        <button
          className="automations-secondary-btn"
          type="button"
          onClick={() => setTick((n) => n + 1)}
        >
          Refresh
        </button>
        <span className="automations-count">
          {filteredRuns.length} / {runs.length}
        </span>
      </div>
      {error && <div className="automations-error">{error}</div>}
      {runs.length === 0 ? (
        <div className="automations-empty">Запусков ещё не было.</div>
      ) : filteredRuns.length === 0 ? (
        <div className="automations-empty">
          {hasFilters
            ? "Нет запусков под текущие фильтры."
            : "Запусков ещё не было."}
        </div>
      ) : (
        <table className="automations-table">
          <thead>
            <tr>
              <th>Время</th>
              <th>App</th>
              <th>Schedule / Action</th>
              <th>Caller</th>
              <th>Статус</th>
              <th>Длительность</th>
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
                <td>{r.caller}</td>
                <td>
                  <span className={`pill pill-${r.status}`}>{r.status}</span>
                  {r.error_preview && (
                    <div className="run-err">{r.error_preview}</div>
                  )}
                </td>
                <td>
                  {r.ended_ms ? `${r.ended_ms - r.started_ms} ms` : "…"}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}
