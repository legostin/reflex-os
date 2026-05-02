import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { RunRecord } from "./types";
import { callerLabel, runStatusLabel } from "./labels";
import { useI18n } from "../../i18n";

export function RunDetailPanel({
  runId,
  onClose,
}: {
  runId: string;
  onClose: () => void;
}) {
  const { t } = useI18n();
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
          <h3>{t("automations.runTitle", { id: runId })}</h3>
          <button
            className="icon-btn"
            onClick={onClose}
            title={t("automations.close")}
          >
            ✕
          </button>
        </header>
        {error && <div className="automations-error">{error}</div>}
        {record === null && !error && (
          <div className="automations-empty">{t("automations.loading")}</div>
        )}
        {record && (
          <div className="run-body">
            <dl className="run-meta">
              <dt>{t("automations.utility")}</dt>
              <dd>{record.app_id}</dd>
              <dt>{t("automations.schedule")}</dt>
              <dd>{record.schedule_id ?? "—"}</dd>
              <dt>{t("automations.action")}</dt>
              <dd>{record.action_id ?? "—"}</dd>
              <dt>{t("automations.caller")}</dt>
              <dd>{callerLabel(record.caller, t)}</dd>
              <dt>{t("automations.status")}</dt>
              <dd>
                <span className={`pill pill-${record.status}`}>
                  {runStatusLabel(record.status, t)}
                </span>
              </dd>
              <dt>{t("automations.duration")}</dt>
              <dd>
                {record.ended_ms
                  ? t("automations.ms", {
                      count: record.ended_ms - record.started_ms,
                    })
                  : t("automations.unfinished")}
              </dd>
            </dl>
            {record.error && (
              <pre className="run-error-block">{record.error}</pre>
            )}
            <h4>{t("automations.steps")}</h4>
            {record.steps.length === 0 ? (
              <div className="automations-empty">
                — {t("automations.none")} —
              </div>
            ) : (
              <ol className="run-steps">
                {record.steps.map((s, i) => (
                  <li key={i} className={`step step-${s.status}`}>
                    <div className="step-head">
                      <span className={`pill pill-${s.status}`}>
                        {runStatusLabel(s.status, t)}
                      </span>
                      <code>{s.method}</code>
                      <span className="step-name">→ {s.name}</span>
                      <span className="step-time">
                        {t("automations.ms", {
                          count: s.ended_ms - s.started_ms,
                        })}
                      </span>
                    </div>
                    {s.error && <pre className="run-error-block">{s.error}</pre>}
                    {s.output_preview && (
                      <details>
                        <summary>
                          {t("automations.outputBytes", {
                            count: s.output_size,
                          })}
                        </summary>
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
