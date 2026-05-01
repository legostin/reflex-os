import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import {
  MemoryNote,
  MemoryScope,
  MEMORY_SCOPES,
} from "../../types/memory";
import MemoryEditor from "./MemoryEditor";
import SearchBox from "./SearchBox";
import "./memory.css";

interface MemoryPanelProps {
  projectRoot?: string | null;
  threadId?: string | null;
  initialScope?: MemoryScope;
}

interface ListFilter {
  kind?: string | null;
  tags?: string[];
  q?: string | null;
}

type ListArgs = {
  scope: MemoryScope;
  projectRoot?: string | null;
  threadId?: string | null;
  filter?: ListFilter;
  [key: string]: unknown;
};

type DeleteArgs = {
  scope: MemoryScope;
  relPath: string;
  projectRoot?: string | null;
  threadId?: string | null;
  [key: string]: unknown;
};

function relativeTime(ms: number): string {
  const diff = Date.now() - ms;
  if (diff < 0) return "just now";
  const sec = Math.floor(diff / 1000);
  if (sec < 60) return `${sec}s ago`;
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m ago`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h ago`;
  const day = Math.floor(hr / 24);
  if (day < 30) return `${day}d ago`;
  const mon = Math.floor(day / 30);
  if (mon < 12) return `${mon}mo ago`;
  const yr = Math.floor(day / 365);
  return `${yr}y ago`;
}

type EditorState =
  | { mode: "closed" }
  | { mode: "create" }
  | { mode: "edit"; note: MemoryNote };

export default function MemoryPanel({
  projectRoot,
  threadId,
  initialScope,
}: MemoryPanelProps) {
  const defaultScope: MemoryScope =
    initialScope ?? (projectRoot ? "project" : "global");
  const [scope, setScope] = useState<MemoryScope>(defaultScope);
  const [notes, setNotes] = useState<MemoryNote[]>([]);
  const [loading, setLoading] = useState<boolean>(false);
  const [error, setError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  const [editor, setEditor] = useState<EditorState>({ mode: "closed" });

  const scopeRequiresProject = scope === "project" || scope === "topic";
  const scopeRequiresThread = scope === "topic";
  const missingProject = scopeRequiresProject && !projectRoot;
  const missingThread = scopeRequiresThread && !threadId;

  const refresh = useCallback(async () => {
    if (missingProject || missingThread) {
      setNotes([]);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const args: ListArgs = { scope };
      if (scopeRequiresProject) args.projectRoot = projectRoot ?? null;
      if (scopeRequiresThread) args.threadId = threadId ?? null;
      const list = await invoke<MemoryNote[]>("memory_list", args);
      const sorted = [...list].sort(
        (a, b) => b.front.updated_at_ms - a.front.updated_at_ms,
      );
      setNotes(sorted);
    } catch (e) {
      setError(String(e));
      setNotes([]);
    } finally {
      setLoading(false);
    }
  }, [
    scope,
    projectRoot,
    threadId,
    scopeRequiresProject,
    scopeRequiresThread,
    missingProject,
    missingThread,
  ]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  function toggleExpanded(relPath: string) {
    setExpanded((prev) => ({ ...prev, [relPath]: !prev[relPath] }));
  }

  async function handleDelete(note: MemoryNote) {
    const ok = window.confirm(
      `Delete memory "${note.front.name}"? This cannot be undone.`,
    );
    if (!ok) return;
    try {
      const args: DeleteArgs = {
        scope: note.scope,
        relPath: note.rel_path,
      };
      if (note.scope === "project" || note.scope === "topic") {
        args.projectRoot = projectRoot ?? null;
      }
      if (note.scope === "topic") {
        args.threadId = threadId ?? null;
      }
      await invoke<void>("memory_delete", args);
      setNotes((prev) => prev.filter((n) => n.rel_path !== note.rel_path));
    } catch (e) {
      setError(String(e));
    }
  }

  function handleSaved(saved: MemoryNote) {
    setEditor({ mode: "closed" });
    setNotes((prev) => {
      const idx = prev.findIndex((n) => n.rel_path === saved.rel_path);
      if (idx === -1) return [saved, ...prev];
      const copy = [...prev];
      copy[idx] = saved;
      return copy.sort(
        (a, b) => b.front.updated_at_ms - a.front.updated_at_ms,
      );
    });
    if (saved.scope !== scope) setScope(saved.scope);
  }

  const newDisabled = useMemo(() => {
    if (scope === "project" && !projectRoot) return true;
    if (scope === "topic" && (!projectRoot || !threadId)) return true;
    return false;
  }, [scope, projectRoot, threadId]);

  return (
    <div className="memory-root">
      <header className="memory-header">
        <div>
          <h1 className="memory-title">Memory</h1>
          <p className="memory-subtitle">
            Notes the agent remembers across sessions.
          </p>
        </div>
        <div className="memory-actions">
          <button
            type="button"
            className="memory-btn"
            onClick={() => void refresh()}
            disabled={loading}
            title="Refresh"
          >
            {loading ? "Loading..." : "Refresh"}
          </button>
          <button
            type="button"
            className="memory-btn memory-btn-primary"
            onClick={() => setEditor({ mode: "create" })}
            disabled={newDisabled}
            title={
              newDisabled
                ? "Open a project (and topic) to add a scoped memory"
                : "Create a new note"
            }
          >
            + New
          </button>
        </div>
      </header>

      <div className="memory-tabs">
        {MEMORY_SCOPES.map((s) => (
          <button
            key={s}
            type="button"
            className={`memory-tab ${scope === s ? "active" : ""}`}
            onClick={() => setScope(s)}
          >
            {s.charAt(0).toUpperCase() + s.slice(1)}
          </button>
        ))}
      </div>

      {editor.mode !== "closed" && (
        <MemoryEditor
          initialScope={scope}
          projectRoot={projectRoot}
          threadId={threadId}
          existing={editor.mode === "edit" ? editor.note : null}
          onSaved={handleSaved}
          onCancel={() => setEditor({ mode: "closed" })}
        />
      )}

      {error && <div className="memory-error">{error}</div>}

      {projectRoot && <SearchBox projectRoot={projectRoot} />}

      {missingProject && (
        <div className="memory-empty">
          Open a project to view {scope} memory.
        </div>
      )}
      {!missingProject && missingThread && (
        <div className="memory-empty">
          Open a topic to view topic memory.
        </div>
      )}

      {!missingProject && !missingThread && (
        <ul className="memory-list">
          {notes.length === 0 && !loading && (
            <li className="memory-empty">No notes in {scope} scope yet.</li>
          )}
          {notes.map((note) => {
            const isOpen = !!expanded[note.rel_path];
            return (
              <li key={note.rel_path} className="memory-item">
                <div
                  className="memory-item-row"
                  onClick={() => toggleExpanded(note.rel_path)}
                >
                  <div className="memory-item-main">
                    <div className="memory-item-title">
                      <span>{note.front.name || note.rel_path}</span>
                      <span className="memory-kind-badge">
                        {note.front.type}
                      </span>
                      {note.front.tags.length > 0 && (
                        <span className="memory-tags">
                          {note.front.tags.map((t) => (
                            <span key={t} className="memory-tag">
                              {t}
                            </span>
                          ))}
                        </span>
                      )}
                    </div>
                    {note.front.description && (
                      <div className="memory-item-desc">
                        {note.front.description}
                      </div>
                    )}
                  </div>
                  <div className="memory-item-meta">
                    <span title={new Date(note.front.updated_at_ms).toString()}>
                      {relativeTime(note.front.updated_at_ms)}
                    </span>
                  </div>
                  <div
                    className="memory-item-actions"
                    onClick={(e) => e.stopPropagation()}
                  >
                    <button
                      type="button"
                      className="memory-btn"
                      onClick={() => setEditor({ mode: "edit", note })}
                      title="Edit"
                    >
                      Edit
                    </button>
                    <button
                      type="button"
                      className="memory-btn memory-btn-danger"
                      onClick={() => void handleDelete(note)}
                      title="Delete"
                    >
                      Delete
                    </button>
                  </div>
                </div>
                {isOpen && (
                  <div className="memory-item-body">
                    <ReactMarkdown remarkPlugins={[remarkGfm]}>
                      {note.body || "_(empty body)_"}
                    </ReactMarkdown>
                  </div>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
