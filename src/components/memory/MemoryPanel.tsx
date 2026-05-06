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
import { useI18n, type Translate } from "../../i18n";
import MemoryEditor from "./MemoryEditor";
import RecallView from "./RecallView";
import SearchBox from "./SearchBox";

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

interface MemoryKindStats {
  kind: string;
  docs: number;
  chunks: number;
}

interface MemoryStats {
  docs: number;
  chunks: number;
  sources: number;
  stale: number;
  missing: number;
  last_indexed_at_ms?: number | null;
  kinds: MemoryKindStats[];
}

function relativeTime(ms: number, t: Translate): string {
  const diff = Date.now() - ms;
  if (diff < 0) return t("memory.time.justNow");
  const sec = Math.floor(diff / 1000);
  if (sec < 60) return t("memory.time.secondsAgo", { count: sec });
  const min = Math.floor(sec / 60);
  if (min < 60) return t("memory.time.minutesAgo", { count: min });
  const hr = Math.floor(min / 60);
  if (hr < 24) return t("memory.time.hoursAgo", { count: hr });
  const day = Math.floor(hr / 24);
  if (day < 30) return t("memory.time.daysAgo", { count: day });
  const mon = Math.floor(day / 30);
  if (mon < 12) return t("memory.time.monthsAgo", { count: mon });
  const yr = Math.floor(day / 365);
  return t("memory.time.yearsAgo", { count: yr });
}

function indexedLabel(
  stats: MemoryStats | null,
  loading: boolean,
  t: Translate,
): string {
  if (loading) return t("memory.index.loading");
  if (!stats?.last_indexed_at_ms) return t("memory.index.none");
  return relativeTime(stats.last_indexed_at_ms, t);
}

function memoryHealthLabel(stats: MemoryStats | null, t: Translate): string {
  if (!stats) return t("memory.index.title");
  if (stats.missing > 0) {
    return t("memory.index.missing", { count: stats.missing });
  }
  if (stats.stale > 0) {
    return t("memory.index.stale", { count: stats.stale });
  }
  return t("memory.index.ok");
}

function scopeLabel(scope: MemoryScope, t: Translate): string {
  if (scope === "global") return t("memory.scope.global");
  if (scope === "project") return t("memory.scope.project");
  return t("memory.scope.topic");
}

function viewLabel(view: MemoryView, t: Translate): string {
  if (view === "notes") return t("memory.view.notes");
  if (view === "recall") return t("memory.view.recall");
  return t("memory.view.search");
}

function kindLabel(kind: MemoryKind, t: Translate): string {
  if (kind === "user") return t("memory.kind.user");
  if (kind === "project") return t("memory.kind.project");
  if (kind === "feedback") return t("memory.kind.feedback");
  if (kind === "reference") return t("memory.kind.reference");
  if (kind === "tool") return t("memory.kind.tool");
  if (kind === "system") return t("memory.kind.system");
  return t("memory.kind.fact");
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
  const { t } = useI18n();
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
  const [stats, setStats] = useState<MemoryStats | null>(null);
  const [statsLoading, setStatsLoading] = useState<boolean>(false);
  const [statsError, setStatsError] = useState<string | null>(null);
  const [reindexing, setReindexing] = useState<boolean>(false);
  const [reindexMessage, setReindexMessage] = useState<string | null>(null);

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

  const refreshStats = useCallback(async () => {
    if (!projectRoot) {
      setStats(null);
      setStatsError(null);
      setReindexMessage(null);
      return;
    }
    setStatsLoading(true);
    setStatsError(null);
    try {
      const nextStats = await invoke<MemoryStats>("memory_stats", {
        projectRoot,
      });
      setStats(nextStats);
    } catch (e) {
      setStats(null);
      setStatsError(String(e));
    } finally {
      setStatsLoading(false);
    }
  }, [projectRoot]);

  const reindexProject = useCallback(async () => {
    if (!projectRoot || reindexing) return;
    setReindexing(true);
    setStatsError(null);
    setReindexMessage(null);
    try {
      const indexed = await invoke<number>("memory_reindex", { projectRoot });
      setReindexMessage(t("memory.reindexedDocs", { count: indexed }));
      await refreshStats();
    } catch (e) {
      setStatsError(String(e));
    } finally {
      setReindexing(false);
    }
  }, [projectRoot, refreshStats, reindexing, t]);

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

  useEffect(() => {
    void refreshStats();
  }, [refreshStats]);

  function toggleExpanded(relPath: string) {
    setExpanded((prev) => ({ ...prev, [relPath]: !prev[relPath] }));
  }

  async function handleDelete(note: MemoryNote) {
    const ok = window.confirm(
      t("memory.deleteConfirm", { name: note.front.name }),
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
      if (projectRoot) void refreshStats();
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
    if (projectRoot) void refreshStats();
  }

  const newDisabled = useMemo(() => {
    if (scope === "project" && !projectRoot) return true;
    if (scope === "topic" && (!projectRoot || !threadId)) return true;
    return false;
  }, [scope, projectRoot, threadId]);

  const filterActive = !!activeFilter;
  const contextLabel = threadId
    ? t("memory.context.topic", { id: threadId })
    : projectRoot
      ? t("memory.context.projectMemory")
      : t("memory.context.globalMemory");

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
          <h1 className="memory-title">{t("memory.title")}</h1>
          <p className="memory-subtitle">
            {t("memory.subtitle")}
          </p>
        </div>
        <div className="memory-actions">
          <button
            type="button"
            className="memory-btn"
            onClick={() => {
              void refresh();
              void refreshStats();
            }}
            disabled={loading}
            title={t("memory.refreshTitle")}
          >
            {loading ? t("memory.loading") : t("memory.refresh")}
          </button>
          <button
            type="button"
            className="memory-btn memory-btn-primary"
            onClick={() => setEditor({ mode: "create" })}
            disabled={newDisabled}
            title={
              newDisabled
                ? t("memory.newDisabledTitle")
                : t("memory.createNoteTitle")
            }
          >
            {t("memory.new")}
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

      {projectRoot && (
        <div
          className={`memory-stats-row ${
            stats && (stats.stale > 0 || stats.missing > 0)
              ? "memory-stats-row-attention"
              : ""
          }`}
        >
          <span className="memory-stat">
            <strong>{stats?.docs ?? 0}</strong>
            <span>{t("memory.stats.docs")}</span>
          </span>
          <span className="memory-stat">
            <strong>{stats?.chunks ?? 0}</strong>
            <span>{t("memory.stats.chunks")}</span>
          </span>
          <span className="memory-stat">
            <strong>{stats?.sources ?? 0}</strong>
            <span>{t("memory.stats.sources")}</span>
          </span>
          <span className="memory-stat">
            <strong>{stats?.stale ?? 0}</strong>
            <span>{t("memory.stats.stale")}</span>
          </span>
          <span className="memory-stat">
            <strong>{stats?.missing ?? 0}</strong>
            <span>{t("memory.stats.missing")}</span>
          </span>
          <span className="memory-stat memory-stat-wide">
            <strong>{indexedLabel(stats, statsLoading, t)}</strong>
            <span>{memoryHealthLabel(stats, t)}</span>
          </span>
          <button
            type="button"
            className="memory-reindex-btn"
            onClick={() => void reindexProject()}
            disabled={reindexing || statsLoading}
            title={t("memory.reindexTitle")}
          >
            {reindexing ? t("memory.reindexing") : t("memory.reindex")}
          </button>
          {reindexMessage && (
            <span className="memory-stat-note">{reindexMessage}</span>
          )}
          {statsError && (
            <span className="memory-stat-error" title={statsError}>
              {t("memory.statsUnavailable")}
            </span>
          )}
        </div>
      )}

      <div
        className="memory-view-tabs"
        role="tablist"
        aria-label={t("memory.tabsAria")}
      >
        {(["notes", "recall", "search"] as MemoryView[]).map((v) => (
          <button
            key={v}
            type="button"
            className={`memory-view-tab ${view === v ? "active" : ""}`}
            onClick={() => setView(v)}
            disabled={(v === "recall" && !canRecall) || (v === "search" && !canSearch)}
            title={
              v === "recall" && !canRecall
                ? t("memory.recallDisabledTitle")
                : v === "search" && !canSearch
                  ? t("memory.searchDisabledTitle")
                  : undefined
            }
          >
            {viewLabel(v, t)}
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
              {scopeLabel(s, t)}
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
            placeholder={t("memory.filter.placeholder")}
            onChange={(e) => setQueryFilter(e.currentTarget.value)}
          />
          <select
            className="memory-select"
            value={kindFilter}
            onChange={(e) =>
              setKindFilter(e.currentTarget.value as MemoryKind | "all")
            }
            aria-label={t("memory.filter.kindAria")}
          >
            <option value="all">{t("memory.filter.allTypes")}</option>
            {MEMORY_KINDS.map((k) => (
              <option key={k} value={k}>
                {kindLabel(k, t)}
              </option>
            ))}
          </select>
          <input
            type="text"
            className="memory-input memory-tag-filter"
            value={tagFilter}
            placeholder={t("memory.filter.tag")}
            onChange={(e) => setTagFilter(e.currentTarget.value)}
          />
          <button
            type="button"
            className="memory-btn"
            onClick={resetFilters}
            disabled={!filterActive}
          >
            {t("memory.filter.reset")}
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
              placeholder={t("memory.recall.placeholder")}
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
              {t("memory.recall.button")}
            </button>
          </div>
          {!canRecall && (
            <div className="memory-empty">
              {t("memory.recallUnavailable")}
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
              {t("memory.recallStartHint")}
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
              {t("memory.searchNoProject")}
            </div>
          )}
        </>
      )}

      {view === "notes" && missingProject && (
        <div className="memory-empty">
          {t("memory.openProjectForScope", { scope: scopeLabel(scope, t) })}
        </div>
      )}
      {view === "notes" && !missingProject && missingThread && (
        <div className="memory-empty">
          {t("memory.openTopicForMemory")}
        </div>
      )}

      {view === "notes" && !missingProject && !missingThread && (
        <ul className="memory-list">
          {notes.length === 0 && !loading && (
            <li className="memory-empty">
              {filterActive
                ? t("memory.emptyFiltered")
                : t("memory.emptyScope", { scope: scopeLabel(scope, t) })}
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
                        {kindLabel(note.front.type, t)}
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
                      {relativeTime(note.front.updated_at_ms, t)}
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
                      title={t("memory.editTitle")}
                    >
                      {t("memory.edit")}
                    </button>
                    <button
                      type="button"
                      className="memory-btn memory-btn-danger"
                      onClick={() => void handleDelete(note)}
                      title={t("memory.deleteTitle")}
                    >
                      {t("memory.delete")}
                    </button>
                  </div>
                </div>
                {isOpen && (
                  <div className="memory-item-body">
                    <ReactMarkdown remarkPlugins={[remarkGfm]}>
                      {note.body || t("memory.emptyMarkdown")}
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
