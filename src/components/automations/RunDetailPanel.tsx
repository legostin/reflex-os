import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { RunRecord } from "./types";
import { callerLabel, runStatusLabel } from "./labels";

export function RunDetailPanel({
  runId,
  onClose,
}: {
  runId: string;
  onClose: () => void;
}) {
  const [record, setRecord] = useState<RunRecord | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    invoke<RunRecord | null>("scheduler_run_detail", { runId })
      .then((r) => alive && setRecord(r))
      .catch((e) => alive && setError(String(e)));
    return () => {
      alive = false;
    };
  }, [runId]);

  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div className="run-detail-backdrop" onClick={onClose}>
      <aside
        className="run-detail-panel"
        onClick={(e) => e.stopPropagation()}
      >
        <header>
          <h3>Запуск {runId}</h3>
          <button className="icon-btn" onClick={onClose} title="Закрыть">
            ✕
          </button>
        </header>
        {error && <div className="automations-error">{error}</div>}
        {record === null && !error && (
          <div className="automations-empty">Загружаю…</div>
        )}
        {record && (
          <div className="run-body">
            <dl className="run-meta">
              <dt>Утилита</dt>
              <dd>{record.app_id}</dd>
              <dt>Расписание</dt>
              <dd>{record.schedule_id ?? "—"}</dd>
              <dt>Действие</dt>
              <dd>{record.action_id ?? "—"}</dd>
              <dt>Инициатор</dt>
              <dd>{callerLabel(record.caller)}</dd>
              <dt>Статус</dt>
              <dd>
                <span className={`pill pill-${record.status}`}>
                  {runStatusLabel(record.status)}
                </span>
              </dd>
              <dt>Длительность</dt>
              <dd>
                {record.ended_ms
                  ? `${record.ended_ms - record.started_ms} мс`
                  : "не завершён"}
              </dd>
            </dl>
            {record.error && (
              <pre className="run-error-block">{record.error}</pre>
            )}
            <h4>Шаги</h4>
            {record.steps.length === 0 ? (
              <div className="automations-empty">— нет —</div>
            ) : (
              <ol className="run-steps">
                {record.steps.map((s, i) => (
                  <li key={i} className={`step step-${s.status}`}>
                    <div className="step-head">
                      <span className={`pill pill-${s.status}`}>
                        {runStatusLabel(s.status)}
                      </span>
                      <code>{s.method}</code>
                      <span className="step-name">→ {s.name}</span>
                      <span className="step-time">
                        {s.ended_ms - s.started_ms} мс
                      </span>
                    </div>
                    {s.error && <pre className="run-error-block">{s.error}</pre>}
                    {s.output_preview && (
                      <details>
                        <summary>вывод ({s.output_size} байт)</summary>
                        <pre className="step-output">{s.output_preview}</pre>
                      </details>
                    )}
                  </li>
                ))}
              </ol>
            )}
          </div>
        )}
      </aside>
    </div>
  );
}
