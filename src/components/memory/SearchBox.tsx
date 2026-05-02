import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { RagHit } from "../../types/memory";
import "./memory.css";

interface SearchBoxProps {
  projectRoot: string;
  defaultLimit?: number;
}

export default function SearchBox({
  projectRoot,
  defaultLimit = 10,
}: SearchBoxProps) {
  const [query, setQuery] = useState<string>("");
  const [hits, setHits] = useState<RagHit[]>([]);
  const [loading, setLoading] = useState<boolean>(false);
  const [error, setError] = useState<string | null>(null);

  async function runSearch() {
    const q = query.trim();
    if (!q || !projectRoot) return;
    setLoading(true);
    setError(null);
    try {
      const result = await invoke<RagHit[]>("memory_search", {
        query: q,
        projectRoot,
        limit: defaultLimit,
      });
      setHits(result);
    } catch (e) {
      setError(String(e));
      setHits([]);
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="memory-search">
      <div className="memory-search-row">
        <input
          type="text"
          className="memory-input"
          placeholder="Поиск по памяти и индексированным документам..."
          value={query}
          onChange={(e) => setQuery(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              void runSearch();
            }
          }}
        />
        <button
          type="button"
          className="memory-btn memory-btn-primary"
          onClick={() => void runSearch()}
          disabled={loading || !query.trim() || !projectRoot}
        >
          {loading ? "Поиск..." : "Искать"}
        </button>
      </div>

      {error && <div className="memory-error">{error}</div>}

      <ul className="memory-search-results">
        {hits.length === 0 && !loading && query && !error && (
          <li className="memory-empty">Ничего не найдено.</li>
        )}
        {hits.map((hit, i) => (
          <li key={`${hit.doc_id}:${i}`} className="recall-list-item">
            <div className="recall-rag-meta">
              <span>{hit.kind}</span>
              <span>оценка {hit.score.toFixed(3)}</span>
              {hit.source && <span>{hit.source}</span>}
            </div>
            <pre className="recall-rag-chunk">{hit.chunk}</pre>
          </li>
        ))}
      </ul>
    </div>
  );
}
