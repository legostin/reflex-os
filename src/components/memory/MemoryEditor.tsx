import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  MemoryKind,
  MemoryNote,
  MemoryScope,
  MEMORY_KINDS,
  MEMORY_SCOPES,
} from "../../types/memory";
import { useI18n, type Translate } from "../../i18n";

interface MemoryEditorProps {
  initialScope: MemoryScope;
  projectRoot?: string | null;
  threadId?: string | null;
  existing?: MemoryNote | null;
  onSaved: (note: MemoryNote) => void;
  onCancel: () => void;
}

type SaveArgs = {
  scope: MemoryScope;
  kind: MemoryKind;
  name: string;
  description: string;
  body: string;
  projectRoot?: string | null;
  threadId?: string | null;
  tags?: string[];
  source?: string | null;
  [key: string]: unknown;
};

function parseTags(input: string): string[] {
  return input
    .split(",")
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
}

function scopeLabel(scope: MemoryScope, t: Translate): string {
  if (scope === "global") return t("memory.scope.global");
  if (scope === "project") return t("memory.scope.project");
  return t("memory.scope.topic");
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

export default function MemoryEditor(props: MemoryEditorProps) {
  const { t } = useI18n();
  const { initialScope, projectRoot, threadId, existing, onSaved, onCancel } =
    props;

  const [scope, setScope] = useState<MemoryScope>(
    existing?.scope ?? initialScope,
  );
  const [kind, setKind] = useState<MemoryKind>(
    existing?.front.type ?? "fact",
  );
  const [name, setName] = useState<string>(existing?.front.name ?? "");
  const [description, setDescription] = useState<string>(
    existing?.front.description ?? "",
  );
  const [tagsInput, setTagsInput] = useState<string>(
    (existing?.front.tags ?? []).join(", "),
  );
  const [body, setBody] = useState<string>(existing?.body ?? "");
  const [saving, setSaving] = useState<boolean>(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (existing) {
      setScope(existing.scope);
      setKind(existing.front.type);
      setName(existing.front.name);
      setDescription(existing.front.description);
      setTagsInput(existing.front.tags.join(", "));
      setBody(existing.body);
    }
  }, [existing]);

  async function handleSave() {
    if (saving) return;
    if (!name.trim()) {
      setError(t("memory.editor.nameRequired"));
      return;
    }
    setSaving(true);
    setError(null);
    try {
      const args: SaveArgs = {
        scope,
        kind,
        name: name.trim(),
        description: description.trim(),
        body,
        tags: parseTags(tagsInput),
      };
      if (scope === "project" || scope === "topic") {
        args.projectRoot = projectRoot ?? null;
      }
      if (scope === "topic") {
        args.threadId = threadId ?? null;
      }
      const note = await invoke<MemoryNote>("memory_save", args);
      onSaved(note);
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }

  const isEdit = !!existing;

  return (
    <div className="memory-editor">
      <div className="memory-editor-row-inline">
        <div className="memory-editor-row">
          <label className="memory-label" htmlFor="memory-scope">
            {t("memory.editor.scope")}
          </label>
          <select
            id="memory-scope"
            className="memory-select"
            value={scope}
            disabled={isEdit}
            onChange={(e) => setScope(e.currentTarget.value as MemoryScope)}
          >
            {MEMORY_SCOPES.map((s) => (
              <option key={s} value={s}>
                {scopeLabel(s, t)}
              </option>
            ))}
          </select>
        </div>
        <div className="memory-editor-row">
          <label className="memory-label" htmlFor="memory-kind">
            {t("memory.editor.kind")}
          </label>
          <select
            id="memory-kind"
            className="memory-select"
            value={kind}
            onChange={(e) => setKind(e.currentTarget.value as MemoryKind)}
          >
            {MEMORY_KINDS.map((k) => (
              <option key={k} value={k}>
                {kindLabel(k, t)}
              </option>
            ))}
          </select>
        </div>
      </div>

      <div className="memory-editor-row">
        <label className="memory-label" htmlFor="memory-name">
          {t("memory.editor.name")}
        </label>
        <input
          id="memory-name"
          className="memory-input"
          type="text"
          value={name}
          placeholder={t("memory.editor.namePlaceholder")}
          onChange={(e) => setName(e.currentTarget.value)}
        />
      </div>

      <div className="memory-editor-row">
        <label className="memory-label" htmlFor="memory-description">
          {t("memory.editor.description")}
        </label>
        <input
          id="memory-description"
          className="memory-input"
          type="text"
          value={description}
          placeholder={t("memory.editor.descriptionPlaceholder")}
          onChange={(e) => setDescription(e.currentTarget.value)}
        />
      </div>

      <div className="memory-editor-row">
        <label className="memory-label" htmlFor="memory-tags">
          {t("memory.editor.tags")}
        </label>
        <input
          id="memory-tags"
          className="memory-input"
          type="text"
          value={tagsInput}
          placeholder="rust, codex, build"
          onChange={(e) => setTagsInput(e.currentTarget.value)}
        />
      </div>

      <div className="memory-editor-row">
        <label className="memory-label" htmlFor="memory-body">
          {t("memory.editor.body")}
        </label>
        <textarea
          id="memory-body"
          className="memory-textarea"
          value={body}
          placeholder={t("memory.editor.bodyPlaceholder")}
          onChange={(e) => setBody(e.currentTarget.value)}
        />
      </div>

      {error && <div className="memory-error">{error}</div>}

      <div className="memory-editor-buttons">
        <button
          type="button"
          className="memory-btn"
          onClick={onCancel}
          disabled={saving}
        >
          {t("memory.editor.cancel")}
        </button>
        <button
          type="button"
          className="memory-btn memory-btn-primary"
          onClick={() => void handleSave()}
          disabled={saving}
        >
          {saving
            ? t("memory.editor.saving")
            : isEdit
              ? t("memory.editor.save")
              : t("memory.editor.createNote")}
        </button>
      </div>
    </div>
  );
}
