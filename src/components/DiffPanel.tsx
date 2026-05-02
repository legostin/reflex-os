import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "../i18n";

export type DiffLine = { op: " " | "+" | "-" | "\\"; text: string };
export type Hunk = {
  header: string;
  oldStart: number;
  oldCount: number;
  newStart: number;
  newCount: number;
  lines: DiffLine[];
};
export type FileDiff = {
  path: string;
  oldPath?: string;
  isNew: boolean;
  isDeleted: boolean;
  isBinary: boolean;
  hunks: Hunk[];
};

function parseHunkHeader(line: string): Pick<
  Hunk,
  "oldStart" | "oldCount" | "newStart" | "newCount" | "header"
> | null {
  // @@ -L,N +L,N @@ ctx
  const m = line.match(/^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/);
  if (!m) return null;
  return {
    oldStart: parseInt(m[1], 10),
    oldCount: m[2] ? parseInt(m[2], 10) : 1,
    newStart: parseInt(m[3], 10),
    newCount: m[4] ? parseInt(m[4], 10) : 1,
    header: line,
  };
}

export function parseUnifiedDiff(text: string): FileDiff[] {
  const lines = text.split("\n");
  const files: FileDiff[] = [];
  let cur: FileDiff | null = null;
  let curHunk: Hunk | null = null;

  const finishHunk = () => {
    if (cur && curHunk) cur.hunks.push(curHunk);
    curHunk = null;
  };
  const finishFile = () => {
    finishHunk();
    if (cur) files.push(cur);
    cur = null;
  };

  for (let i = 0; i < lines.length; i++) {
    const ln = lines[i];
    if (ln.startsWith("diff --git ")) {
      finishFile();
      // diff --git a/foo b/bar
      const parts = ln.slice("diff --git ".length).split(" ");
      const aPath = parts[0]?.replace(/^a\//, "");
      const bPath = parts[1]?.replace(/^b\//, "");
      cur = {
        path: bPath || aPath || "(unknown)",
        oldPath: aPath,
        isNew: false,
        isDeleted: false,
        isBinary: false,
        hunks: [],
      };
      continue;
    }
    if (!cur) continue;
    if (ln.startsWith("new file mode")) cur.isNew = true;
    else if (ln.startsWith("deleted file mode")) cur.isDeleted = true;
    else if (ln.startsWith("Binary files ")) cur.isBinary = true;
    else if (ln.startsWith("--- ")) {
      // ignore — we already have aPath
    } else if (ln.startsWith("+++ ")) {
      const path = ln.slice(4).replace(/^b\//, "");
      if (path !== "/dev/null") cur.path = path;
    } else if (ln.startsWith("@@")) {
      finishHunk();
      const parsed = parseHunkHeader(ln);
      if (parsed) {
        curHunk = { ...parsed, lines: [] };
      }
    } else if (curHunk) {
      // body line
      if (ln.length === 0 && i === lines.length - 1) continue;
      const op = ln[0];
      if (op === "+" || op === "-" || op === " " || op === "\\") {
        curHunk.lines.push({ op, text: ln.slice(1) });
      }
    }
  }
  finishFile();
  return files;
}

function renderHunkPatch(hunk: Hunk): string {
  const body = hunk.lines.map((l) => l.op + l.text).join("\n");
  return `${hunk.header}\n${body}`;
}

function renderFileHeader(file: FileDiff): string {
  const a = file.isNew ? "/dev/null" : `a/${file.oldPath ?? file.path}`;
  const b = file.isDeleted ? "/dev/null" : `b/${file.path}`;
  let head = `diff --git a/${file.oldPath ?? file.path} b/${file.path}\n`;
  if (file.isNew) head += "new file mode 100644\n";
  if (file.isDeleted) head += "deleted file mode 100644\n";
  head += `--- ${a}\n+++ ${b}\n`;
  return head;
}

function buildPatch(
  files: FileDiff[],
  selected: Set<string>,
): string {
  // selected key = `${file.path}::${hunkIdx}`
  const out: string[] = [];
  for (const file of files) {
    if (file.isBinary) continue;
    const includedHunks: Hunk[] = [];
    for (let i = 0; i < file.hunks.length; i++) {
      if (selected.has(`${file.path}::${i}`)) {
        includedHunks.push(file.hunks[i]);
      }
    }
    if (includedHunks.length === 0) continue;
    out.push(renderFileHeader(file));
    for (const h of includedHunks) {
      out.push(renderHunkPatch(h));
    }
  }
  // git apply expects trailing newline
  return out.length === 0 ? "" : out.join("\n") + "\n";
}

export function DiffPanel({
  appId,
  onClose,
  onApplied,
}: {
  appId: string;
  onClose: () => void;
  onApplied: () => void;
}) {
  const { t } = useI18n();
  const [raw, setRaw] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [commitMsg, setCommitMsg] = useState("partial revision");

  useEffect(() => {
    let alive = true;
    invoke<string>("app_diff", { appId })
      .then((d) => {
        if (!alive) return;
        setRaw(d);
        // pre-select all hunks
        const files = parseUnifiedDiff(d);
        const sel = new Set<string>();
        for (const f of files) {
          for (let i = 0; i < f.hunks.length; i++) sel.add(`${f.path}::${i}`);
        }
        setSelected(sel);
      })
      .catch((e) => alive && setError(String(e)));
    return () => {
      alive = false;
    };
  }, [appId]);

  const files = useMemo(() => (raw ? parseUnifiedDiff(raw) : []), [raw]);

  const toggle = (key: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };
  const toggleFile = (file: FileDiff) => {
    setSelected((prev) => {
      const next = new Set(prev);
      const allKeys = file.hunks.map((_, i) => `${file.path}::${i}`);
      const allSelected = allKeys.every((k) => next.has(k));
      if (allSelected) {
        for (const k of allKeys) next.delete(k);
      } else {
        for (const k of allKeys) next.add(k);
      }
      return next;
    });
  };

  async function apply() {
    if (busy) return;
    const patch = buildPatch(files, selected);
    if (!patch) {
      setError(t("diff.noSelection"));
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await invoke("app_save_partial", {
        appId,
        patch,
        message: commitMsg.trim() || "partial revision",
      });
      onApplied();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="modal-backdrop" onClick={() => !busy && onClose()}>
      <div
        className="modal diff-modal"
        onClick={(e) => e.stopPropagation()}
      >
        <header className="diff-header">
          <h2 className="modal-title">🔍 Diff</h2>
          <input
            className="diff-commit-msg"
            type="text"
            placeholder={t("diff.commitPlaceholder")}
            value={commitMsg}
            onChange={(e) => setCommitMsg(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                void apply();
              }
            }}
          />
          <div className="diff-header-actions">
            <button
              className="modal-btn modal-btn-primary"
              onClick={() => void apply()}
              disabled={busy || files.length === 0 || selected.size === 0}
            >
              {busy
                ? t("diff.applying")
                : t("diff.applyCount", { count: selected.size })}
            </button>
            <button className="modal-btn" onClick={onClose} disabled={busy}>
              {t("diff.close")}
            </button>
          </div>
        </header>
        {error && <div className="apps-error">{error}</div>}
        <div className="diff-body">
          {raw === null && !error && (
            <div className="diff-empty">{t("diff.loading")}</div>
          )}
          {raw !== null && files.length === 0 && (
            <div className="diff-empty">{t("diff.noChanges")}</div>
          )}
          {files.map((file) => {
            const allKeys = file.hunks.map((_, i) => `${file.path}::${i}`);
            const allSelected =
              allKeys.length > 0 && allKeys.every((k) => selected.has(k));
            const someSelected = allKeys.some((k) => selected.has(k));
            return (
              <div key={file.path} className="diff-file">
                <header className="diff-file-header">
                  <label className="diff-checkbox">
                    <input
                      type="checkbox"
                      checked={allSelected}
                      ref={(el) => {
                        if (el)
                          el.indeterminate = !allSelected && someSelected;
                      }}
                      onChange={() => toggleFile(file)}
                      disabled={file.isBinary}
                    />
                  </label>
                  <span className="diff-file-path">
                    {file.isNew && (
                      <span className="diff-tag diff-tag-new">
                        {t("diff.new")}
                      </span>
                    )}
                    {file.isDeleted && (
                      <span className="diff-tag diff-tag-del">
                        {t("diff.deleted")}
                      </span>
                    )}
                    {file.isBinary && (
                      <span className="diff-tag diff-tag-bin">
                        {t("diff.binary")}
                      </span>
                    )}
                    {file.path}
                  </span>
                </header>
                {file.isBinary && (
                  <div className="diff-empty">
                    {t("diff.binaryUnavailable")}
                  </div>
                )}
                {file.hunks.map((h, i) => {
                  const key = `${file.path}::${i}`;
                  return (
                    <div key={i} className="diff-hunk">
                      <header className="diff-hunk-header">
                        <label className="diff-checkbox">
                          <input
                            type="checkbox"
                            checked={selected.has(key)}
                            onChange={() => toggle(key)}
                          />
                        </label>
                        <code className="diff-hunk-h">{h.header}</code>
                      </header>
                      <pre className="diff-hunk-body">
                        {h.lines.map((l, j) => (
                          <span
                            key={j}
                            className={
                              l.op === "+"
                                ? "diff-add"
                                : l.op === "-"
                                  ? "diff-del"
                                  : "diff-ctx"
                            }
                          >
                            {l.op}
                            {l.text}
                            {"\n"}
                          </span>
                        ))}
                      </pre>
                    </div>
                  );
                })}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
