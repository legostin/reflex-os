import { useEffect, useState } from "react";
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

  return (
    <section className="automations-runs">
      {error && <div className="automations-error">{error}</div>}
      {runs.length === 0 ? (
        <div className="automations-empty">Запусков ещё не было.</div>
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
            {runs.map((r) => (
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
