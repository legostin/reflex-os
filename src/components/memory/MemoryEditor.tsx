import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  MemoryKind,
  MemoryNote,
  MemoryScope,
  MEMORY_KINDS,
  MEMORY_SCOPES,
} from "../../types/memory";

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

function scopeLabel(scope: MemoryScope): string {
  if (scope === "global") return "Глобальная";
  if (scope === "project") return "Проект";
  return "Топик";
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

export default function MemoryEditor(props: MemoryEditorProps) {
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
      setError("Укажи название");
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
            Область
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
                {scopeLabel(s)}
              </option>
            ))}
          </select>
        </div>
        <div className="memory-editor-row">
          <label className="memory-label" htmlFor="memory-kind">
            Тип
          </label>
          <select
            id="memory-kind"
            className="memory-select"
            value={kind}
            onChange={(e) => setKind(e.currentTarget.value as MemoryKind)}
          >
            {MEMORY_KINDS.map((k) => (
              <option key={k} value={k}>
                {kindLabel(k)}
              </option>
            ))}
          </select>
        </div>
      </div>

      <div className="memory-editor-row">
        <label className="memory-label" htmlFor="memory-name">
          Название
        </label>
        <input
          id="memory-name"
          className="memory-input"
          type="text"
          value={name}
          placeholder="Короткое понятное название"
          onChange={(e) => setName(e.currentTarget.value)}
        />
      </div>

      <div className="memory-editor-row">
        <label className="memory-label" htmlFor="memory-description">
          Описание
        </label>
        <input
          id="memory-description"
          className="memory-input"
          type="text"
          value={description}
          placeholder="Краткое описание в одну строку"
          onChange={(e) => setDescription(e.currentTarget.value)}
        />
      </div>

      <div className="memory-editor-row">
        <label className="memory-label" htmlFor="memory-tags">
          Теги через запятую
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
          Текст заметки
        </label>
        <textarea
          id="memory-body"
          className="memory-textarea"
          value={body}
          placeholder="Напиши заметку в Markdown..."
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
          Отмена
        </button>
        <button
          type="button"
          className="memory-btn memory-btn-primary"
          onClick={() => void handleSave()}
          disabled={saving}
        >
          {saving ? "Сохранение..." : isEdit ? "Сохранить" : "Создать заметку"}
        </button>
      </div>
    </div>
  );
}
