import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import "./QuickPanel.css";

type QuickContext = {
  frontmost_app: string | null;
  finder_target: string | null;
};

type Project = {
  id: string;
  name: string;
  root: string;
  created_at_ms: number;
};

type QuickOpenPayload = {
  ctx: QuickContext;
  project: Project | null;
  candidate_root: string | null;
  nearest: Project[];
};

const EMPTY_PAYLOAD: QuickOpenPayload = {
  ctx: { frontmost_app: null, finder_target: null },
  project: null,
  candidate_root: null,
  nearest: [],
};

function basename(p: string): string {
  if (!p) return "";
  const trimmed = p.replace(/\/+$/, "");
  const idx = trimmed.lastIndexOf("/");
  return idx === -1 ? trimmed : trimmed.slice(idx + 1) || trimmed;
}

export default function QuickPanel() {
  const [payload, setPayload] = useState<QuickOpenPayload>(EMPTY_PAYLOAD);
  const [project, setProject] = useState<Project | null>(null);
  const [prompt, setPrompt] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const projectRef = useRef<Project | null>(null);
  projectRef.current = project;

  useEffect(() => {
    const focusInput = () => {
      requestAnimationFrame(() => inputRef.current?.focus());
    };

    const unlistenPromise = listen<QuickOpenPayload>(
      "reflex://quick-open",
      (e) => {
        const p = e.payload ?? EMPTY_PAYLOAD;
        setPayload(p);
        setProject(p.project);
        setPrompt("");
        setError(null);
        focusInput();
      },
    );

    focusInput();

    const onKey = async (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        await getCurrentWindow().hide();
        return;
      }
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        e.stopPropagation();
        void submit();
      }
    };
    window.addEventListener("keydown", onKey, true);

    return () => {
      window.removeEventListener("keydown", onKey, true);
      unlistenPromise.then((u) => u());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function submit() {
    const text = inputRef.current?.value.trim() ?? "";
    if (!text || busy) return;
    if (!projectRef.current) {
      setError("Выбери или создай проект");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await invoke("submit_quick", {
        prompt: text,
        ctx: payload.ctx,
        projectId: projectRef.current.id,
      });
    } catch (e) {
      console.error("[reflex] submit_quick failed", e);
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function createHere() {
    if (!payload.candidate_root) return;
    setBusy(true);
    setError(null);
    try {
      const created = await invoke<Project>("create_project", {
        root: payload.candidate_root,
        name: null,
      });
      setProject(created);
    } catch (e) {
      console.error("[reflex] create_project failed", e);
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="quick-root">
      <div className="quick-card">
        <div className="quick-input-row">
          <input
            ref={inputRef}
            className="quick-input"
            type="text"
            placeholder={
              project
                ? `Спросить ${project.name}…`
                : "Сначала выбери или создай проект"
            }
            value={prompt}
            onChange={(e) => setPrompt(e.currentTarget.value)}
            autoFocus
            spellCheck={false}
            disabled={busy}
          />
          <span className="quick-hint">
            <kbd>↵</kbd> submit · <kbd>esc</kbd> cancel
          </span>
        </div>

        <div className="quick-project-row">
          {project ? (
            <span className="quick-chip quick-chip-project">
              📁 {project.name}
              <span className="quick-chip-path-inline" title={project.root}>
                {project.root}
              </span>
            </span>
          ) : payload.candidate_root ? (
            <button
              className="quick-action"
              onClick={() => void createHere()}
              disabled={busy}
              title={payload.candidate_root}
            >
              + Создать проект «{basename(payload.candidate_root)}»
            </button>
          ) : (
            <span className="quick-muted">Нет открытой папки в Finder</span>
          )}

          {payload.nearest.length > 0 && (
            <select
              className="quick-select"
              value={project?.id ?? ""}
              onChange={(e) => {
                const id = e.currentTarget.value;
                const found = payload.nearest.find((p) => p.id === id);
                if (found) setProject(found);
              }}
            >
              <option value="" disabled>
                выбрать существующий…
              </option>
              {payload.nearest.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name} — {p.root}
                </option>
              ))}
            </select>
          )}

          {payload.ctx.frontmost_app && (
            <span className="quick-chip quick-chip-app">
              {payload.ctx.frontmost_app}
            </span>
          )}
        </div>

        {error && <div className="quick-error">{error}</div>}
      </div>
    </div>
  );
}
