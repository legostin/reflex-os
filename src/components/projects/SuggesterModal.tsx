import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./suggester.css";

interface ExistingSuggestion {
  app_id: string;
  reason: string;
}

interface NewSuggestion {
  name: string;
  description: string;
  reason: string;
}

interface SuggestionPlan {
  use_existing: ExistingSuggestion[];
  create_new: NewSuggestion[];
}

interface SuggestionResult {
  plan: SuggestionPlan;
  project_description: string | null;
  raw: string | null;
}

interface AppLite {
  id: string;
  name: string;
  icon?: string | null;
}

interface Props {
  projectId: string;
  installedApps: AppLite[];
  onClose: () => void;
  onApplied: () => void;
}

export function SuggesterModal({
  projectId,
  installedApps,
  onClose,
  onApplied,
}: Props) {
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [plan, setPlan] = useState<SuggestionPlan | null>(null);
  const [pickedExisting, setPickedExisting] = useState<Set<string>>(new Set());
  const [pickedNew, setPickedNew] = useState<Set<number>>(new Set());
  const [applying, setApplying] = useState(false);
  const [applyError, setApplyError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    setLoading(true);
    setError(null);
    invoke<SuggestionResult>("suggest_apps_for_project", { projectId })
      .then((res) => {
        if (!alive) return;
        setPlan(res.plan);
        setPickedExisting(new Set(res.plan.use_existing.map((e) => e.app_id)));
        setPickedNew(new Set(res.plan.create_new.map((_, i) => i)));
      })
      .catch((e) => alive && setError(String(e)))
      .finally(() => alive && setLoading(false));
    return () => {
      alive = false;
    };
  }, [projectId]);

  function toggleExisting(id: string) {
    setPickedExisting((s) => {
      const n = new Set(s);
      n.has(id) ? n.delete(id) : n.add(id);
      return n;
    });
  }

  function toggleNew(idx: number) {
    setPickedNew((s) => {
      const n = new Set(s);
      n.has(idx) ? n.delete(idx) : n.add(idx);
      return n;
    });
  }

  async function applyPlan() {
    if (!plan || applying) return;
    setApplying(true);
    setApplyError(null);
    try {
      for (const e of plan.use_existing) {
        if (!pickedExisting.has(e.app_id)) continue;
        await invoke("link_app_to_project", {
          projectId,
          appId: e.app_id,
        });
      }
      for (let i = 0; i < plan.create_new.length; i++) {
        if (!pickedNew.has(i)) continue;
        const item = plan.create_new[i];
        const description = `${item.name}\n\n${item.description}`.trim();
        const res = await invoke<{ app_id: string; thread_id: string }>(
          "create_app",
          { description, template: "blank" },
        );
        try {
          await invoke("link_app_to_project", {
            projectId,
            appId: res.app_id,
          });
        } catch (e) {
          console.warn("[reflex] link new app failed", e);
        }
      }
      onApplied();
      onClose();
    } catch (e) {
      setApplyError(String(e));
    } finally {
      setApplying(false);
    }
  }

  function nameOf(appId: string): string {
    return installedApps.find((a) => a.id === appId)?.name ?? appId;
  }
  function iconOf(appId: string): string {
    return installedApps.find((a) => a.id === appId)?.icon ?? "🧩";
  }

  const canApply = pickedExisting.size > 0 || pickedNew.size > 0;

  return (
    <div className="modal-backdrop" onClick={() => !applying && onClose()}>
      <div
        className="modal suggester-modal"
        onClick={(e) => e.stopPropagation()}
      >
        <h2 className="modal-title">Предложения по проекту</h2>
        {loading && (
          <div className="suggester-loading">
            <div className="suggester-spinner" />
            Думаю над каталогом утилит…
          </div>
        )}
        {error && (
          <div className="suggester-error">Ошибка анализа: {error}</div>
        )}
        {plan && !loading && (
          <>
            {plan.use_existing.length === 0 && plan.create_new.length === 0 ? (
              <p className="modal-hint">
                Ничего предложить не удалось. Можешь привязать утилиты вручную
                позже.
              </p>
            ) : (
              <p className="modal-hint">
                Сними галочки если что-то не нужно. Создание новых утилит запустится в фоне через Codex.
              </p>
            )}

            {plan.use_existing.length > 0 && (
              <section className="suggester-section">
                <h3 className="suggester-section-title">
                  Привязать существующие
                </h3>
                <ul className="suggester-list">
                  {plan.use_existing.map((e) => (
                    <li
                      key={e.app_id}
                      className="suggester-row"
                      onClick={() => toggleExisting(e.app_id)}
                    >
                      <input
                        type="checkbox"
                        checked={pickedExisting.has(e.app_id)}
                        onChange={() => toggleExisting(e.app_id)}
                        onClick={(ev) => ev.stopPropagation()}
                      />
                      <span className="suggester-icon">{iconOf(e.app_id)}</span>
                      <div className="suggester-info">
                        <div className="suggester-name">{nameOf(e.app_id)}</div>
                        <div className="suggester-reason">{e.reason}</div>
                      </div>
                    </li>
                  ))}
                </ul>
              </section>
            )}

            {plan.create_new.length > 0 && (
              <section className="suggester-section">
                <h3 className="suggester-section-title">
                  Создать новые утилиты
                </h3>
                <ul className="suggester-list">
                  {plan.create_new.map((n, i) => (
                    <li
                      key={i}
                      className="suggester-row"
                      onClick={() => toggleNew(i)}
                    >
                      <input
                        type="checkbox"
                        checked={pickedNew.has(i)}
                        onChange={() => toggleNew(i)}
                        onClick={(ev) => ev.stopPropagation()}
                      />
                      <span className="suggester-icon">✨</span>
                      <div className="suggester-info">
                        <div className="suggester-name">{n.name}</div>
                        <div className="suggester-desc">{n.description}</div>
                        <div className="suggester-reason">{n.reason}</div>
                      </div>
                    </li>
                  ))}
                </ul>
              </section>
            )}
          </>
        )}

        {applyError && (
          <div className="suggester-error">{applyError}</div>
        )}

        <div className="modal-actions">
          <button
            className="modal-btn"
            disabled={applying}
            onClick={onClose}
          >
            Пропустить
          </button>
          <button
            className="modal-btn modal-btn-primary"
            disabled={applying || loading || !canApply}
            onClick={() => void applyPlan()}
          >
            {applying
              ? "Применяю…"
              : `Применить (${pickedExisting.size + pickedNew.size})`}
          </button>
        </div>
      </div>
    </div>
  );
}
