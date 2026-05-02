import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "../../i18n";
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

export interface PathStatus {
  path: string;
  kind: string;
  class: FileClass;
  indexed: boolean;
  indexed_under: number | null;
  indexed_at_ms: number | null;
  modified_ms: number | null;
  stale: boolean;
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
  onStatusChanged?: () => void;
}

const CLASS_LABEL_KEY: Record<FileClass, string> = {
  text: "file.class.text",
  code: "file.class.code",
  image: "file.class.image",
  binary: "file.class.binary",
  toolarge: "file.class.toolarge",
  unsupported: "file.class.unsupported",
};

export function FileActionsDrawer({
  target,
  projectRoot,
  onClose,
  onStartTopic,
  onStatusChanged,
}: Props) {
  const { t } = useI18n();
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
      onStatusChanged?.();
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
      onStatusChanged?.();
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
      return `For the folder \`${target!.path}\`, explain what it contains, its role in the project, and what I should pay attention to.`;
    }
    if (cls === "image") {
      return `Look at the image \`${target!.path}\` and describe what is shown and why it matters in this project.`;
    }
    return `Read the file \`${target!.path}\` and explain its contents: purpose, key sections, and potential issues.`;
  }

  function editPrompt(): string {
    return `Open \`${target!.path}\`, describe its current contents, and ask me what changes to make. Do not edit anything yet.`;
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
    if (busy === "index") return t("file.indexing");
    if (busy === "reindex") return t("file.reindexing");
    if (status?.indexed) return t("file.reindex");
    return t("file.index");
  })();

  return (
    <div className="file-drawer-backdrop" onClick={onClose}>
      <aside
        className="file-drawer"
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-label={t("file.actionsAria")}
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
                  {t(CLASS_LABEL_KEY[status.class] ?? status.kind)}
                </span>
                {status.indexed && (
                  <span className="file-drawer-tag file-drawer-tag-on">
                    {isDir
                      ? t("file.inRagCount", {
                          count: status.indexed_under ?? "?",
                        })
                      : t("file.inRag")}
                  </span>
                )}
              </div>
            )}
          </div>
          <button
            className="file-drawer-close"
            onClick={onClose}
            title={t("file.closeEsc")}
          >
            ✕
          </button>
        </header>

        {loading && (
          <div className="file-drawer-loading">{t("file.loadingStatus")}</div>
        )}

        <div className="file-drawer-actions">
          <button
            className="file-drawer-btn file-drawer-btn-primary"
            onClick={startTalk}
          >
            <span className="file-drawer-btn-title">{t("file.talkTitle")}</span>
            <span className="file-drawer-btn-hint">
              {t("file.talkHint")}
            </span>
          </button>

          {canEdit && (
            <button className="file-drawer-btn" onClick={startEdit}>
              <span className="file-drawer-btn-title">{t("file.editTitle")}</span>
              <span className="file-drawer-btn-hint">
                {t("file.editHint")}
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
                  ? t("file.indexDirHint")
                  : cls === "image"
                    ? t("file.indexImageHint")
                    : t("file.indexTextHint")}
              </span>
            </button>
          )}

          {!indexable && cls && (
            <div className="file-drawer-note">
              {cls === "binary" && t("file.binaryNote")}
              {cls === "toolarge" && t("file.tooLargeNote")}
              {cls === "unsupported" && t("file.unsupportedNote")}
            </div>
          )}

          {status?.indexed && (
            <button
              className="file-drawer-btn file-drawer-btn-danger"
              onClick={doForget}
              disabled={busy !== null}
            >
              <span className="file-drawer-btn-title">
                {t("file.forgetTitle")}
              </span>
              <span className="file-drawer-btn-hint">
                {t("file.forgetHint")}
              </span>
            </button>
          )}

          <button className="file-drawer-btn" onClick={doReveal}>
            <span className="file-drawer-btn-title">{t("file.openFinder")}</span>
          </button>
        </div>

        {error && <div className="file-drawer-error">{error}</div>}

        {outcome && (
          <div className="file-drawer-outcome">
            <div>
              {t("file.indexedCount", { count: outcome.indexed })}
            </div>
            {outcome.skipped.length > 0 && (
              <details>
                <summary>
                  {t("file.skippedCount", { count: outcome.skipped.length })}
                </summary>
                <ul>
                  {outcome.skipped.slice(0, 30).map((s) => (
                    <li key={s.path}>
                      <code>{s.path}</code> - {s.reason}
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
