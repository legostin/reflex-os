import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { MemoryRef, RagHit, RecallResult } from "../../types/memory";
import { useI18n } from "../../i18n";
import "./memory.css";

interface RecallViewProps {
  projectRoot: string;
  threadId: string;
  query: string;
}

interface CollapsedSections {
  project: boolean;
  topic: boolean;
  rag: boolean;
}

function isProjectScope(ref: MemoryRef): boolean {
  return ref.scope === "project";
}

function isTopicScope(ref: MemoryRef): boolean {
  return ref.scope === "topic";
}

export default function RecallView({
  projectRoot,
  threadId,
  query,
}: RecallViewProps) {
  const { t } = useI18n();
  const [result, setResult] = useState<RecallResult | null>(null);
  const [loading, setLoading] = useState<boolean>(false);
  const [error, setError] = useState<string | null>(null);
  const [collapsed, setCollapsed] = useState<CollapsedSections>({
    project: false,
    topic: false,
    rag: true,
  });

  useEffect(() => {
    let alive = true;
    if (!projectRoot || !threadId || !query.trim()) {
      setResult(null);
      return;
    }
    setLoading(true);
    setError(null);
    invoke<RecallResult>("memory_recall", {
      projectRoot,
      threadId,
      query,
    })
      .then((r) => {
        if (!alive) return;
        setResult(r);
      })
      .catch((e) => {
        if (!alive) return;
        setError(String(e));
        setResult(null);
      })
      .finally(() => {
        if (alive) setLoading(false);
      });
    return () => {
      alive = false;
    };
  }, [projectRoot, threadId, query]);

  function toggle(key: keyof CollapsedSections) {
    setCollapsed((prev) => ({ ...prev, [key]: !prev[key] }));
  }

  const projectNotes = (result?.notes ?? []).filter(isProjectScope);
  const topicNotes = (result?.notes ?? []).filter(isTopicScope);
  const ragHits: RagHit[] = result?.rag ?? [];

  return (
    <div className="recall-root">
      <div className="recall-header">
        <h3 className="recall-title">{t("recall.title")}</h3>
        <span className="recall-query" title={query}>
          {query || t("recall.noQuery")}
        </span>
      </div>

      {loading && <div className="recall-loading">{t("recall.loading")}</div>}
      {error && <div className="memory-error">{error}</div>}

      {result && (
        <>
          {result.markdown.trim().length > 0 && (
            <div className="recall-markdown">
              <ReactMarkdown remarkPlugins={[remarkGfm]}>
                {result.markdown}
              </ReactMarkdown>
            </div>
          )}

          <section className="recall-section">
            <button
              type="button"
              className="recall-section-header"
              onClick={() => toggle("project")}
            >
              <span>{collapsed.project ? ">" : "v"}</span>
              <span>{t("recall.projectMemory")}</span>
              <span className="recall-section-count">
                {projectNotes.length}
              </span>
            </button>
            {!collapsed.project && (
              <ul className="recall-list">
                {projectNotes.length === 0 ? (
                  <li className="memory-empty">{t("recall.noProjectNotes")}</li>
                ) : (
                  projectNotes.map((n) => (
                    <li key={n.rel_path} className="recall-list-item">
                      {n.rel_path}
                    </li>
                  ))
                )}
              </ul>
            )}
          </section>

          <section className="recall-section">
            <button
              type="button"
              className="recall-section-header"
              onClick={() => toggle("topic")}
            >
              <span>{collapsed.topic ? ">" : "v"}</span>
              <span>{t("recall.topicMemory")}</span>
              <span className="recall-section-count">{topicNotes.length}</span>
            </button>
            {!collapsed.topic && (
              <ul className="recall-list">
                {topicNotes.length === 0 ? (
                  <li className="memory-empty">{t("recall.noTopicNotes")}</li>
                ) : (
                  topicNotes.map((n) => (
                    <li key={n.rel_path} className="recall-list-item">
                      {n.rel_path}
                    </li>
                  ))
                )}
              </ul>
            )}
          </section>

          <section className="recall-section">
            <button
              type="button"
              className="recall-section-header"
              onClick={() => toggle("rag")}
            >
              <span>{collapsed.rag ? ">" : "v"}</span>
              <span>{t("recall.ragMatches")}</span>
              <span className="recall-section-count">{ragHits.length}</span>
            </button>
            {!collapsed.rag && (
              <ul className="recall-list">
                {ragHits.length === 0 ? (
                  <li className="memory-empty">{t("recall.noRagMatches")}</li>
                ) : (
                  ragHits.map((hit, i) => (
                    <li
                      key={`${hit.doc_id}:${i}`}
                      className="recall-list-item"
                    >
                      <div className="recall-rag-meta">
                        <span>{hit.kind}</span>
                        <span>
                          {t("recall.score", {
                            score: hit.score.toFixed(3),
                          })}
                        </span>
                        {hit.source && <span>{hit.source}</span>}
                      </div>
                      <pre className="recall-rag-chunk">{hit.chunk}</pre>
                    </li>
                  ))
                )}
              </ul>
            )}
          </section>
        </>
      )}
    </div>
  );
}
