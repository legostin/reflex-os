import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import {
  MemoryKind,
  MemoryListFilter,
  MemoryNote,
  MemoryScope,
  MEMORY_KINDS,
  MEMORY_SCOPES,
} from "../../types/memory";
import MemoryEditor from "./MemoryEditor";
import RecallView from "./RecallView";
import SearchBox from "./SearchBox";
import "./memory.css";

interface MemoryPanelProps {
  projectRoot?: string | null;
  threadId?: string | null;
  initialScope?: MemoryScope;
  initialView?: MemoryView;
  initialRecallQuery?: string;
}

type MemoryView = "notes" | "recall" | "search";

type ListArgs = {
  scope: MemoryScope;
  projectRoot?: string | null;
  threadId?: string | null;
  filter?: MemoryListFilter;
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
  initialView = "notes",
  initialRecallQuery = "",
}: MemoryPanelProps) {
  const defaultScope: MemoryScope =
    initialScope ?? (projectRoot ? "project" : "global");
  const defaultRecallQuery = initialRecallQuery.trim();
  const [scope, setScope] = useState<MemoryScope>(defaultScope);
  const [view, setView] = useState<MemoryView>(initialView);
  const [notes, setNotes] = useState<MemoryNote[]>([]);
  const [loading, setLoading] = useState<boolean>(false);
  const [error, setError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  const [editor, setEditor] = useState<EditorState>({ mode: "closed" });
  const [queryFilter, setQueryFilter] = useState<string>("");
  const [tagFilter, setTagFilter] = useState<string>("");
  const [kindFilter, setKindFilter] = useState<MemoryKind | "all">("all");
  const [recallInput, setRecallInput] = useState<string>(defaultRecallQuery);
  const [recallQuery, setRecallQuery] = useState<string>(defaultRecallQuery);

  const scopeRequiresProject = scope === "project" || scope === "topic";
  const scopeRequiresThread = scope === "topic";
  const missingProject = scopeRequiresProject && !projectRoot;
  const missingThread = scopeRequiresThread && !threadId;
  const canRecall = !!projectRoot && !!threadId;
  const canSearch = !!projectRoot;

  const activeFilter = useMemo<MemoryListFilter | undefined>(() => {
    const filter: MemoryListFilter = {};
    const q = queryFilter.trim();
    const tag = tagFilter.trim();
    if (kindFilter !== "all") filter.kind = kindFilter;
    if (tag) filter.tag = tag;
    if (q) filter.query = q;
    return filter.kind || filter.tag || filter.query ? filter : undefined;
  }, [kindFilter, queryFilter, tagFilter]);

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
      if (activeFilter) args.filter = activeFilter;
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
    activeFilter,
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

  const filterActive = !!activeFilter;
  const contextLabel = threadId
    ? `Topic ${threadId}`
    : projectRoot
      ? "Project memory"
      : "Global memory";

  function runRecall() {
    const q = recallInput.trim();
    if (!q || !canRecall) return;
    setRecallQuery(q);
  }

  function resetFilters() {
    setQueryFilter("");
    setTagFilter("");
    setKindFilter("all");
  }

  return (
    <div className="memory-root">
      <header className="memory-header">
        <div>
          <h1 className="memory-title">Reflex Memory</h1>
          <p className="memory-subtitle">
            Durable notes, recall context, and indexed project knowledge.
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

      <div className="memory-context-row">
        <span className="memory-context-pill">{contextLabel}</span>
        {projectRoot && (
          <span className="memory-context-path" title={projectRoot}>
            {projectRoot}
          </span>
        )}
      </div>

      <div className="memory-view-tabs" role="tablist" aria-label="Memory view">
        {(["notes", "recall", "search"] as MemoryView[]).map((v) => (
          <button
            key={v}
            type="button"
            className={`memory-view-tab ${view === v ? "active" : ""}`}
            onClick={() => setView(v)}
            disabled={(v === "recall" && !canRecall) || (v === "search" && !canSearch)}
            title={
              v === "recall" && !canRecall
                ? "Open Memory from a topic to run recall"
                : v === "search" && !canSearch
                  ? "Open a project to search indexed memory"
                  : undefined
            }
          >
            {v === "notes" ? "Notes" : v === "recall" ? "Recall" : "Search"}
          </button>
        ))}
      </div>

      {(view === "notes" || editor.mode !== "closed") && (
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
      )}

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

      {view === "notes" && (
        <div className="memory-filter-bar">
          <input
            type="text"
            className="memory-input"
            value={queryFilter}
            placeholder="Filter notes by text..."
            onChange={(e) => setQueryFilter(e.currentTarget.value)}
          />
          <select
            className="memory-select"
            value={kindFilter}
            onChange={(e) =>
              setKindFilter(e.currentTarget.value as MemoryKind | "all")
            }
            aria-label="Filter memory kind"
          >
            <option value="all">All kinds</option>
            {MEMORY_KINDS.map((k) => (
              <option key={k} value={k}>
                {k}
              </option>
            ))}
          </select>
          <input
            type="text"
            className="memory-input memory-tag-filter"
            value={tagFilter}
            placeholder="tag"
            onChange={(e) => setTagFilter(e.currentTarget.value)}
          />
          <button
            type="button"
            className="memory-btn"
            onClick={resetFilters}
            disabled={!filterActive}
          >
            Reset
          </button>
        </div>
      )}

      {view === "recall" && (
        <div className="memory-recall-workbench">
          <div className="memory-search-row">
            <input
              type="text"
              className="memory-input"
              value={recallInput}
              placeholder="Ask what Reflex should recall for this topic..."
              onChange={(e) => setRecallInput(e.currentTarget.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  runRecall();
                }
              }}
              disabled={!canRecall}
            />
            <button
              type="button"
              className="memory-btn memory-btn-primary"
              onClick={runRecall}
              disabled={!canRecall || !recallInput.trim()}
            >
              Recall
            </button>
          </div>
          {!canRecall && (
            <div className="memory-empty">
              Open Memory from a topic to recall topic notes and project RAG
              together.
            </div>
          )}
          {canRecall && recallQuery && projectRoot && threadId && (
            <RecallView
              projectRoot={projectRoot}
              threadId={threadId}
              query={recallQuery}
            />
          )}
          {canRecall && !recallQuery && (
            <div className="memory-empty">
              Enter a query to compose project notes, topic notes, and indexed
              context.
            </div>
          )}
        </div>
      )}

      {view === "search" && (
        <>
          {projectRoot ? (
            <SearchBox projectRoot={projectRoot} />
          ) : (
            <div className="memory-empty">
              Open a project to search indexed memory and documents.
            </div>
          )}
        </>
      )}

      {view === "notes" && missingProject && (
        <div className="memory-empty">
          Open a project to view {scope} memory.
        </div>
      )}
      {view === "notes" && !missingProject && missingThread && (
        <div className="memory-empty">
          Open a topic to view topic memory.
        </div>
      )}

      {view === "notes" && !missingProject && !missingThread && (
        <ul className="memory-list">
          {notes.length === 0 && !loading && (
            <li className="memory-empty">
              {filterActive
                ? "No notes match the current filters."
                : `No notes in ${scope} scope yet.`}
            </li>
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
