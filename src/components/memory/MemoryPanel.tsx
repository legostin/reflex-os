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

function relativeTime(ms: number): string {
  const diff = Date.now() - ms;
  if (diff < 0) return "только что";
  const sec = Math.floor(diff / 1000);
  if (sec < 60) return `${sec}с назад`;
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}м назад`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}ч назад`;
  const day = Math.floor(hr / 24);
  if (day < 30) return `${day}д назад`;
  const mon = Math.floor(day / 30);
  if (mon < 12) return `${mon}мес назад`;
  const yr = Math.floor(day / 365);
  return `${yr}г назад`;
}

function indexedLabel(stats: MemoryStats | null, loading: boolean): string {
  if (loading) return "загрузка";
  if (!stats?.last_indexed_at_ms) return "нет индекса";
  return relativeTime(stats.last_indexed_at_ms);
}

function memoryHealthLabel(stats: MemoryStats | null): string {
  if (!stats) return "RAG индекс";
  if (stats.missing > 0) return `${stats.missing} отсутствует`;
  if (stats.stale > 0) return `${stats.stale} устарело`;
  return "в норме";
}

function scopeLabel(scope: MemoryScope): string {
  if (scope === "global") return "Глобальная";
  if (scope === "project") return "Проект";
  return "Топик";
}

function viewLabel(view: MemoryView): string {
  if (view === "notes") return "Заметки";
  if (view === "recall") return "Вспомнить";
  return "Поиск";
}

function kindLabel(kind: MemoryKind): string {
  if (kind === "user") return "Пользователь";
  if (kind === "project") return "Проект";
  if (kind === "feedback") return "Обратная связь";
  if (kind === "reference") return "Справка";
  if (kind === "tool") return "Инструмент";
  if (kind === "system") return "Система";
  return "Факт";
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
      setReindexMessage(`переиндексировано ${indexed} док.`);
      await refreshStats();
    } catch (e) {
      setStatsError(String(e));
    } finally {
      setReindexing(false);
    }
  }, [projectRoot, refreshStats, reindexing]);

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
      `Удалить память "${note.front.name}"? Это действие нельзя отменить.`,
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
    ? `Топик ${threadId}`
    : projectRoot
      ? "Память проекта"
      : "Глобальная память";

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
          <h1 className="memory-title">Память Reflex</h1>
          <p className="memory-subtitle">
            Долгая память, контекст вспоминания и индексированные знания проекта.
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
            title="Обновить"
          >
            {loading ? "Загрузка..." : "Обновить"}
          </button>
          <button
            type="button"
            className="memory-btn memory-btn-primary"
            onClick={() => setEditor({ mode: "create" })}
            disabled={newDisabled}
            title={
              newDisabled
                ? "Открой проект или топик, чтобы добавить память в нужной области"
                : "Создать заметку"
            }
          >
            + Новая
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
            <span>док.</span>
          </span>
          <span className="memory-stat">
            <strong>{stats?.chunks ?? 0}</strong>
            <span>чанки</span>
          </span>
          <span className="memory-stat">
            <strong>{stats?.sources ?? 0}</strong>
            <span>источники</span>
          </span>
          <span className="memory-stat">
            <strong>{stats?.stale ?? 0}</strong>
            <span>устар.</span>
          </span>
          <span className="memory-stat">
            <strong>{stats?.missing ?? 0}</strong>
            <span>нет</span>
          </span>
          <span className="memory-stat memory-stat-wide">
            <strong>{indexedLabel(stats, statsLoading)}</strong>
            <span>{memoryHealthLabel(stats)}</span>
          </span>
          <button
            type="button"
            className="memory-reindex-btn"
            onClick={() => void reindexProject()}
            disabled={reindexing || statsLoading}
            title="Переиндексировать поддерживаемые файлы проекта"
          >
            {reindexing ? "Индексация..." : "Переиндекс."}
          </button>
          {reindexMessage && (
            <span className="memory-stat-note">{reindexMessage}</span>
          )}
          {statsError && (
            <span className="memory-stat-error" title={statsError}>
              статистика недоступна
            </span>
          )}
        </div>
      )}

      <div className="memory-view-tabs" role="tablist" aria-label="Раздел памяти">
        {(["notes", "recall", "search"] as MemoryView[]).map((v) => (
          <button
            key={v}
            type="button"
            className={`memory-view-tab ${view === v ? "active" : ""}`}
            onClick={() => setView(v)}
            disabled={(v === "recall" && !canRecall) || (v === "search" && !canSearch)}
            title={
              v === "recall" && !canRecall
                ? "Открой память из топика, чтобы вспомнить контекст"
                : v === "search" && !canSearch
                  ? "Открой проект, чтобы искать в индексе памяти"
                  : undefined
            }
          >
            {viewLabel(v)}
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
              {scopeLabel(s)}
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
            placeholder="Фильтр заметок по тексту..."
            onChange={(e) => setQueryFilter(e.currentTarget.value)}
          />
          <select
            className="memory-select"
            value={kindFilter}
            onChange={(e) =>
              setKindFilter(e.currentTarget.value as MemoryKind | "all")
            }
            aria-label="Фильтр типа памяти"
          >
            <option value="all">Все типы</option>
            {MEMORY_KINDS.map((k) => (
              <option key={k} value={k}>
                {kindLabel(k)}
              </option>
            ))}
          </select>
          <input
            type="text"
            className="memory-input memory-tag-filter"
            value={tagFilter}
            placeholder="тег"
            onChange={(e) => setTagFilter(e.currentTarget.value)}
          />
          <button
            type="button"
            className="memory-btn"
            onClick={resetFilters}
            disabled={!filterActive}
          >
            Сброс
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
              placeholder="Что Reflex должен вспомнить для этого топика?"
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
              Вспомнить
            </button>
          </div>
          {!canRecall && (
            <div className="memory-empty">
              Открой память из топика, чтобы собрать заметки топика и RAG
              проекта вместе.
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
              Введи запрос, чтобы собрать проектные заметки, заметки топика и
              индексированный контекст.
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
              Открой проект, чтобы искать по индексированной памяти и документам.
            </div>
          )}
        </>
      )}

      {view === "notes" && missingProject && (
        <div className="memory-empty">
          Открой проект, чтобы смотреть память: {scopeLabel(scope)}.
        </div>
      )}
      {view === "notes" && !missingProject && missingThread && (
        <div className="memory-empty">
          Открой топик, чтобы смотреть память топика.
        </div>
      )}

      {view === "notes" && !missingProject && !missingThread && (
        <ul className="memory-list">
          {notes.length === 0 && !loading && (
            <li className="memory-empty">
              {filterActive
                ? "Нет заметок под текущие фильтры."
                : `В области ${scopeLabel(scope)} пока нет заметок.`}
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
                        {kindLabel(note.front.type)}
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
                      title="Редактировать"
                    >
                      Править
                    </button>
                    <button
                      type="button"
                      className="memory-btn memory-btn-danger"
                      onClick={() => void handleDelete(note)}
                      title="Удалить"
                    >
                      Удалить
                    </button>
                  </div>
                </div>
                {isOpen && (
                  <div className="memory-item-body">
                    <ReactMarkdown remarkPlugins={[remarkGfm]}>
                      {note.body || "_(пусто)_"}
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
