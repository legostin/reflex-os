import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./file-drawer.css";

export type DrawerTarget = {
  name: string;
  path: string;
  kind: "file" | "directory" | "symlink";
  modified_ms: number | null;
  is_hidden: boolean;
};

type FileClass =
  | "text"
  | "code"
  | "image"
  | "binary"
  | "toolarge"
  | "unsupported";

interface PathStatus {
  path: string;
  kind: string;
  class: FileClass;
  indexed: boolean;
  indexed_under: number | null;
}

interface IndexOutcome {
  indexed: number;
  skipped: { path: string; reason: string }[];
}

interface Props {
  target: DrawerTarget | null;
  projectRoot: string;
  onClose: () => void;
  onStartTopic: (prompt: string, planMode?: boolean) => void | Promise<void>;
}

const CLASS_LABEL: Record<FileClass, string> = {
  text: "Документ",
  code: "Исходный код",
  image: "Изображение",
  binary: "Бинарный файл",
  toolarge: "Слишком большой",
  unsupported: "Не поддерживается",
};

export function FileActionsDrawer({
  target,
  projectRoot,
  onClose,
  onStartTopic,
}: Props) {
  const [status, setStatus] = useState<PathStatus | null>(null);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [outcome, setOutcome] = useState<IndexOutcome | null>(null);

  useEffect(() => {
    if (!target) {
      setStatus(null);
      setOutcome(null);
      setError(null);
      return;
    }
    setLoading(true);
    setError(null);
    setOutcome(null);
    invoke<PathStatus>("memory_path_status", {
      projectRoot,
      path: target.path,
    })
      .then((s) => setStatus(s))
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  }, [target?.path, projectRoot]);

  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    if (target) window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [target, onClose]);

  if (!target) return null;

  const isDir = target.kind === "directory";
  const cls = status?.class;
  const indexable =
    isDir ||
    cls === "text" ||
    cls === "code" ||
    cls === "image";
  const canEdit = !isDir && (cls === "text" || cls === "code");

  async function refreshStatus() {
    if (!target) return;
    try {
      const s = await invoke<PathStatus>("memory_path_status", {
        projectRoot,
        path: target.path,
      });
      setStatus(s);
    } catch (e) {
      setError(String(e));
    }
  }

  async function doIndex(reindex: boolean) {
    if (!target) return;
    setBusy(reindex ? "reindex" : "index");
    setError(null);
    setOutcome(null);
    try {
      if (reindex) {
        await invoke<number>("memory_forget_path", {
          projectRoot,
          path: target.path,
        });
      }
      const out = await invoke<IndexOutcome>("memory_index_path", {
        projectRoot,
        path: target.path,
      });
      setOutcome(out);
      await refreshStatus();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  }

  async function doForget() {
    if (!target) return;
    setBusy("forget");
    setError(null);
    setOutcome(null);
    try {
      await invoke<number>("memory_forget_path", {
        projectRoot,
        path: target.path,
      });
      await refreshStatus();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  }

  function doReveal() {
    if (!target) return;
    invoke("reveal_in_finder", { path: target.path }).catch((e) =>
      console.error("[reflex] reveal_in_finder", e),
    );
  }

  function talkPrompt(): string {
    if (isDir) {
      return `Расскажи про папку \`${target!.path}\`: что в ней лежит, какова её роль в проекте, на что обратить внимание.`;
    }
    if (cls === "image") {
      return `Посмотри на картинку \`${target!.path}\` и расскажи что на ней изображено и зачем она нужна в этом проекте.`;
    }
    return `Прочитай файл \`${target!.path}\` и расскажи о его содержимом: назначение, ключевые места, потенциальные проблемы.`;
  }

  function editPrompt(): string {
    return `Открой \`${target!.path}\`, опиши что в нём сейчас, и спроси у меня какие изменения внести. Пока ничего не правь.`;
  }

  function startTalk() {
    void onStartTopic(talkPrompt());
    onClose();
  }

  function startEdit() {
    void onStartTopic(editPrompt());
    onClose();
  }

  const indexButtonLabel = (() => {
    if (busy === "index") return "Индексирую…";
    if (busy === "reindex") return "Переиндексирую…";
    if (status?.indexed) return "Переиндексировать";
    return "Проиндексировать";
  })();

  return (
    <div className="file-drawer-backdrop" onClick={onClose}>
      <aside
        className="file-drawer"
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-label="Действия с файлом"
      >
        <header className="file-drawer-header">
          <div className="file-drawer-icon">
            {isDir ? "📁" : cls === "image" ? "🖼" : "📄"}
          </div>
          <div className="file-drawer-titles">
            <h3 className="file-drawer-title">{target.name}</h3>
            <div className="file-drawer-path" title={target.path}>
              {target.path}
            </div>
            {status && (
              <div className="file-drawer-meta">
                <span className="file-drawer-tag">
                  {CLASS_LABEL[status.class] ?? status.kind}
                </span>
                {status.indexed && (
                  <span className="file-drawer-tag file-drawer-tag-on">
                    {isDir
                      ? `в RAG: ${status.indexed_under ?? "?"}`
                      : "в RAG"}
                  </span>
                )}
              </div>
            )}
          </div>
          <button
            className="file-drawer-close"
            onClick={onClose}
            title="Закрыть (Esc)"
          >
            ✕
          </button>
        </header>

        {loading && <div className="file-drawer-loading">Загружаю статус…</div>}

        <div className="file-drawer-actions">
          <button
            className="file-drawer-btn file-drawer-btn-primary"
            onClick={startTalk}
          >
            <span className="file-drawer-btn-title">Поговорить о содержимом</span>
            <span className="file-drawer-btn-hint">
              Запустит топик с этим путём в контексте
            </span>
          </button>

          {canEdit && (
            <button className="file-drawer-btn" onClick={startEdit}>
              <span className="file-drawer-btn-title">Изменить</span>
              <span className="file-drawer-btn-hint">
                Топик с предложением правок
              </span>
            </button>
          )}

          {indexable && (
            <button
              className="file-drawer-btn"
              onClick={() => doIndex(!!status?.indexed)}
              disabled={busy !== null || loading}
            >
              <span className="file-drawer-btn-title">{indexButtonLabel}</span>
              <span className="file-drawer-btn-hint">
                {isDir
                  ? "Рекурсивно: файлы, картинки, исходники"
                  : cls === "image"
                    ? "Через codex описание + bge-m3"
                    : "Через bge-m3 (Ollama)"}
              </span>
            </button>
          )}

          {!indexable && cls && (
            <div className="file-drawer-note">
              {cls === "binary" && "Бинарный файл индексировать нельзя."}
              {cls === "toolarge" &&
                "Файл слишком большой (лимит: 1 MB для текста, 5 MB для картинок)."}
              {cls === "unsupported" && "Файл этого типа не индексируется."}
            </div>
          )}

          {status?.indexed && (
            <button
              className="file-drawer-btn file-drawer-btn-danger"
              onClick={doForget}
              disabled={busy !== null}
            >
              <span className="file-drawer-btn-title">Удалить из памяти</span>
              <span className="file-drawer-btn-hint">
                Очистит RAG-записи для этого пути
              </span>
            </button>
          )}

          <button className="file-drawer-btn" onClick={doReveal}>
            <span className="file-drawer-btn-title">Открыть в Finder</span>
          </button>
        </div>

        {error && <div className="file-drawer-error">{error}</div>}

        {outcome && (
          <div className="file-drawer-outcome">
            <div>
              Индексировано: <strong>{outcome.indexed}</strong>
            </div>
            {outcome.skipped.length > 0 && (
              <details>
                <summary>Пропущено: {outcome.skipped.length}</summary>
                <ul>
                  {outcome.skipped.slice(0, 30).map((s) => (
                    <li key={s.path}>
                      <code>{s.path}</code> — {s.reason}
                    </li>
                  ))}
                </ul>
              </details>
            )}
          </div>
        )}
      </aside>
    </div>
  );
}
