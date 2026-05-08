import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useI18n, type Translate } from "../i18n";

export interface TopicComposerApp {
  id: string;
  name: string;
  icon?: string | null;
  description?: string | null;
  ready?: boolean;
}

interface ProjectFileEntry {
  name: string;
  path: string;
  relative_path: string;
  kind: "file" | "directory" | "symlink";
  size?: number | null;
  modified_ms?: number | null;
}

interface ImageAttachment {
  path: string;
  name: string;
}

export interface TopicComposerSendMeta {
  goal?: string | null;
  planMode?: boolean;
}

interface TopicComposerProps {
  threadId?: string | null;
  projectRoot: string | null;
  running: boolean;
  showPlanBanner: boolean;
  submitting: boolean;
  stopping: boolean;
  apps: TopicComposerApp[];
  memoryScope?: "topic" | "project";
  onSend: (
    prompt: string,
    imagePaths: string[],
    meta?: TopicComposerSendMeta,
  ) => Promise<void>;
  onStop: () => Promise<void>;
  onOpenApp?: (appId: string) => void;
}

type CommandId =
  | "remember"
  | "dream"
  | "run"
  | "plan"
  | "goal"
  | "file"
  | "image"
  | "app";

interface ComposerCommand {
  id: CommandId;
  token: string;
  title: string;
  description: string;
}

function commands(t: Translate): ComposerCommand[] {
  return [
    {
      id: "remember",
      token: "/remember",
      title: t("topicComposer.commandRemember"),
      description: t("topicComposer.commandRememberHint"),
    },
    {
      id: "dream",
      token: "/dream",
      title: t("topicComposer.commandDream"),
      description: t("topicComposer.commandDreamHint"),
    },
    {
      id: "run",
      token: "/run",
      title: t("topicComposer.commandRun"),
      description: t("topicComposer.commandRunHint"),
    },
    {
      id: "plan",
      token: "/plan",
      title: t("topicComposer.commandPlan"),
      description: t("topicComposer.commandPlanHint"),
    },
    {
      id: "goal",
      token: "/goal",
      title: t("topicComposer.commandGoal"),
      description: t("topicComposer.commandGoalHint"),
    },
    {
      id: "file",
      token: "@",
      title: t("topicComposer.commandFile"),
      description: t("topicComposer.commandFileHint"),
    },
    {
      id: "image",
      token: "/image",
      title: t("topicComposer.commandImage"),
      description: t("topicComposer.commandImageHint"),
    },
    {
      id: "app",
      token: "/app",
      title: t("topicComposer.commandApp"),
      description: t("topicComposer.commandAppHint"),
    },
  ];
}

function commandPrompt(raw: string): string {
  const run = raw.match(/^\/run\s+([\s\S]+)$/i);
  if (run) {
    const command = run[1].trim();
    return [
      "Run this command in the current project workspace.",
      "Use the configured sandbox and request approval if required.",
      "After it finishes, summarize the result and any changed files.",
      "",
      "Command:",
      "```sh",
      command,
      "```",
    ].join("\n");
  }

  const plan = raw.match(/^\/plan\s+([\s\S]+)$/i);
  if (plan) {
    return [
      "Create a concrete execution plan before editing files.",
      "Inspect the relevant project context first, cite the files you inspected, and wait for confirmation before making changes.",
      "",
      "Task:",
      plan[1].trim(),
    ].join("\n");
  }

  return raw;
}

function goalPrompt(goal: string): string {
  return [
    "Set this as the active goal for the thread and work toward it until it is complete.",
    "Treat the goal as the user's requested outcome, not as UI copy.",
    "",
    "Goal:",
    goal,
  ].join("\n");
}

function displayPath(path: string): string {
  const normalized = path.replace(/\\/g, "/");
  const parts = normalized.split("/");
  return parts[parts.length - 1] || normalized;
}

function memoryTitle(body: string): string {
  return body
    .split(/\n/)
    .map((line) => line.trim())
    .find(Boolean)
    ?.slice(0, 80) || "Topic note";
}

function appMatches(app: TopicComposerApp, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  return `${app.name} ${app.id} ${app.description ?? ""}`
    .toLowerCase()
    .includes(q);
}

function fileMatches(file: ProjectFileEntry, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  return `${file.name} ${file.relative_path}`.toLowerCase().includes(q);
}

function currentFileQuery(draft: string): string | null {
  const match = draft.match(/(^|\s)@([^\s@]*)$/);
  return match ? match[2] : null;
}

function slashQuery(draft: string): string | null {
  const match = draft.match(/^\s*\/([a-z]*)$/i);
  return match ? match[1].toLowerCase() : null;
}

function appQuery(draft: string): string | null {
  const match = draft.match(/^\s*\/app\s+([\s\S]*)$/i);
  return match ? match[1].trim() : null;
}

function replaceFileTrigger(draft: string, file: ProjectFileEntry): string {
  const mention = `@file("${file.relative_path}") `;
  return draft.replace(/(^|\s)@([^\s@]*)$/, `$1${mention}`);
}

function referencedFiles(
  draft: string,
  files: ProjectFileEntry[],
): ProjectFileEntry[] {
  const byRel = new Map(files.map((file) => [file.relative_path, file]));
  const seen = new Set<string>();
  const out: ProjectFileEntry[] = [];
  for (const match of draft.matchAll(/@file\("([^"]+)"\)/g)) {
    const rel = match[1];
    if (seen.has(rel)) continue;
    seen.add(rel);
    const file = byRel.get(rel);
    if (file) out.push(file);
  }
  return out;
}

function withContext(
  draft: string,
  files: ProjectFileEntry[],
  images: ImageAttachment[],
): string {
  const refs = referencedFiles(draft, files);
  if (refs.length === 0 && images.length === 0) return draft;

  const blocks: string[] = [];
  if (refs.length > 0) {
    blocks.push(
      [
        "Project file references:",
        ...refs.map((file) => `- ${file.relative_path}: ${file.path}`),
      ].join("\n"),
    );
  }
  if (images.length > 0) {
    blocks.push(
      [
        "Attached images:",
        ...images.map((image) => `- ${image.name}: ${image.path}`),
      ].join("\n"),
    );
  }

  return `${draft}\n\n[Reflex topic context]\n${blocks.join("\n\n")}`;
}

export function TopicComposer({
  threadId,
  projectRoot,
  running,
  showPlanBanner,
  submitting,
  stopping,
  apps,
  memoryScope,
  onSend,
  onStop,
  onOpenApp,
}: TopicComposerProps) {
  const { t } = useI18n();
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const [draft, setDraft] = useState("");
  const [files, setFiles] = useState<ProjectFileEntry[]>([]);
  const [images, setImages] = useState<ImageAttachment[]>([]);
  const [savingMemory, setSavingMemory] = useState(false);
  const [pickingImage, setPickingImage] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const resolvedMemoryScope = memoryScope ?? (threadId ? "topic" : "project");
  const commandQuery = slashQuery(draft);
  const fileQuery = currentFileQuery(draft);
  const activeAppQuery = appQuery(draft);

  useEffect(() => {
    if (!projectRoot) {
      setFiles([]);
      return;
    }
    let alive = true;
    invoke<ProjectFileEntry[]>("list_project_files", {
      projectRoot,
      query: fileQuery ?? undefined,
      limit: 160,
    })
      .then((nextFiles) => {
        if (alive) setFiles(nextFiles);
      })
      .catch((e) => {
        if (alive) console.warn("[reflex] list_project_files failed", e);
      });
    return () => {
      alive = false;
    };
  }, [projectRoot, fileQuery]);

  const commandItems = useMemo(() => commands(t), [t]);
  const visibleCommands = useMemo(() => {
    if (commandQuery == null) return [];
    return commandItems.filter(
      (cmd) =>
        cmd.id.includes(commandQuery) ||
        cmd.token.slice(1).includes(commandQuery) ||
        cmd.title.toLowerCase().includes(commandQuery),
    );
  }, [commandItems, commandQuery]);
  const visibleFiles = useMemo(() => {
    if (fileQuery == null) return [];
    return files.filter((file) => fileMatches(file, fileQuery)).slice(0, 8);
  }, [fileQuery, files]);
  const visibleApps = useMemo(() => {
    if (activeAppQuery == null) return [];
    return apps.filter((app) => appMatches(app, activeAppQuery)).slice(0, 8);
  }, [activeAppQuery, apps]);

  async function saveMemory(body: string) {
    const text = body.trim();
    if (!text || !projectRoot || savingMemory) return;
    setSavingMemory(true);
    setError(null);
    setStatus(null);
    try {
      await invoke("memory_save", {
        scope: resolvedMemoryScope,
        kind: "fact",
        name: memoryTitle(text),
        description: "Saved from topic composer",
        body: text,
        projectRoot,
        threadId: resolvedMemoryScope === "topic" ? threadId : undefined,
        tags: [resolvedMemoryScope, "composer"],
        source: "topic-composer",
      });
      setStatus(t("topicComposer.memorySaved"));
      setDraft("");
    } catch (e) {
      setError(String(e));
    } finally {
      setSavingMemory(false);
    }
  }

  async function pickImage() {
    if (pickingImage) return;
    setPickingImage(true);
    setError(null);
    try {
      const path = await invoke<string | null>("pick_open_file", {
        title: t("topicComposer.pickImageTitle"),
        filterName: "Images",
        filterExtensions: ["png", "jpg", "jpeg", "gif", "webp", "heic"],
      });
      if (!path) return;
      setImages((prev) => {
        if (prev.some((item) => item.path === path)) return prev;
        return [...prev, { path, name: displayPath(path) }];
      });
      textareaRef.current?.focus();
    } catch (e) {
      setError(String(e));
    } finally {
      setPickingImage(false);
    }
  }

  function insertCommand(cmd: ComposerCommand) {
    if (cmd.id === "image") {
      void pickImage();
      return;
    }
    if (cmd.id === "file") {
      setDraft("@");
      return;
    }
    setDraft(`${cmd.token} `);
    requestAnimationFrame(() => textareaRef.current?.focus());
  }

  function insertFile(file: ProjectFileEntry) {
    setDraft((prev) => replaceFileTrigger(prev, file));
    requestAnimationFrame(() => textareaRef.current?.focus());
  }

  function openApp(app: TopicComposerApp) {
    onOpenApp?.(app.id);
    setDraft("");
  }

  async function submit() {
    const text = draft.trim();
    if (!text || submitting || savingMemory) return;
    setError(null);
    setStatus(null);

    const remember = text.match(/^\/remember\s+([\s\S]+)$/i);
    if (remember) {
      await saveMemory(remember[1]);
      return;
    }

    const goalMatch = text.match(/^\/goal\s+([\s\S]+)$/i);
    if (goalMatch) {
      const goal = goalMatch[1].trim();
      if (!goal) return;
      try {
        const prompt = withContext(goalPrompt(goal), files, images);
        await onSend(
          prompt,
          images.map((image) => image.path),
          { goal },
        );
        setDraft("");
        setImages([]);
      } catch (e) {
        setError(String(e));
      }
      return;
    }

    const appMatch = text.match(/^\/app\s+([\s\S]+)$/i);
    if (appMatch) {
      const app = apps.find((item) => appMatches(item, appMatch[1]));
      if (app && onOpenApp) {
        openApp(app);
        return;
      }
    }

    try {
      const planCommand = /^\/plan\s+([\s\S]+)$/i.test(text);
      const prompt = withContext(commandPrompt(text), files, images);
      await onSend(
        prompt,
        images.map((image) => image.path),
        planCommand ? { planMode: true } : undefined,
      );
      setDraft("");
      setImages([]);
    } catch (e) {
      setError(String(e));
    }
  }

  const canSend = !!draft.trim() && !submitting && !savingMemory;
  const placeholder = running
    ? t("thread.placeholderInterrupt")
    : showPlanBanner
      ? t("thread.placeholderPlan")
      : t("topicComposer.placeholder");

  return (
    <div className="topic-composer">
      {running && (
        <div className="topic-composer-running">
          {t("thread.codexWorking")}
        </div>
      )}

      <div
        className="topic-composer-toolbar"
        aria-label={t("topicComposer.toolbar")}
      >
        <button
          type="button"
          className="topic-composer-tool"
          onClick={() => setDraft((prev) => (prev.trim() ? prev : "/"))}
          title={t("topicComposer.commandsTitle")}
        >
          /
        </button>
        <button
          type="button"
          className="topic-composer-tool"
          onClick={() => setDraft((prev) => (prev.trim() ? `${prev} @` : "@"))}
          disabled={!projectRoot}
          title={t("topicComposer.filesTitle")}
        >
          @
        </button>
        <button
          type="button"
          className="topic-composer-tool"
          onClick={() => void pickImage()}
          disabled={pickingImage}
          title={t("topicComposer.imageTitle")}
        >
          img
        </button>
        <button
          type="button"
          className="topic-composer-tool"
          onClick={() => void saveMemory(draft)}
          disabled={
            !draft.trim() ||
            !projectRoot ||
            savingMemory ||
            (resolvedMemoryScope === "topic" && !threadId)
          }
          title={t("topicComposer.saveMemoryTitle")}
        >
          mem
        </button>
        <button
          type="button"
          className="topic-composer-tool"
          onClick={() => setDraft("/app ")}
          disabled={!onOpenApp || apps.length === 0}
          title={t("topicComposer.commandAppHint")}
        >
          apps
        </button>
      </div>

      <div className="topic-composer-box">
        <textarea
          ref={textareaRef}
          className="topic-composer-input"
          rows={3}
          value={draft}
          placeholder={placeholder}
          onChange={(e) => setDraft(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
              e.preventDefault();
              void submit();
            }
          }}
          disabled={submitting}
        />
        <div className="topic-composer-actions">
          {running && (
            <button
              type="button"
              className="chat-followup-button chat-followup-stop"
              onClick={() => void onStop()}
              disabled={stopping || submitting}
              title={t("thread.stopTitle")}
            >
              {stopping ? "..." : t("thread.stop")}
            </button>
          )}
          <button
            type="button"
            className="chat-followup-button"
            onClick={() => void submit()}
            disabled={!canSend}
            title={
              running ? t("thread.interruptSendTitle") : t("thread.sendTitle")
            }
          >
            {running ? "send" : "run"}
          </button>
        </div>
      </div>

      {images.length > 0 && (
        <div className="topic-composer-attachments">
          {images.map((image) => (
            <span
              key={image.path}
              className="topic-composer-chip"
              title={image.path}
            >
              {image.name}
              <button
                type="button"
                onClick={() =>
                  setImages((prev) =>
                    prev.filter((item) => item.path !== image.path),
                  )
                }
                aria-label={t("topicComposer.removeAttachment")}
              >
                x
              </button>
            </span>
          ))}
        </div>
      )}

      {(visibleCommands.length > 0 ||
        visibleFiles.length > 0 ||
        visibleApps.length > 0) && (
        <div className="topic-composer-menu">
          {visibleCommands.map((cmd) => (
            <button
              type="button"
              key={cmd.id}
              className="topic-composer-menu-row"
              onClick={() => insertCommand(cmd)}
            >
              <span className="topic-composer-menu-token">{cmd.token}</span>
              <span className="topic-composer-menu-main">
                <strong>{cmd.title}</strong>
                <span>{cmd.description}</span>
              </span>
            </button>
          ))}
          {visibleFiles.map((file) => (
            <button
              type="button"
              key={file.path}
              className="topic-composer-menu-row"
              onClick={() => insertFile(file)}
            >
              <span className="topic-composer-menu-token">
                {file.kind === "directory" ? "dir" : "file"}
              </span>
              <span className="topic-composer-menu-main">
                <strong>{file.name}</strong>
                <span>{file.relative_path}</span>
              </span>
            </button>
          ))}
          {visibleApps.map((app) => (
            <button
              type="button"
              key={app.id}
              className="topic-composer-menu-row"
              onClick={() => openApp(app)}
              disabled={app.ready === false || !onOpenApp}
            >
              <span className="topic-composer-menu-token">
                {app.icon ?? "app"}
              </span>
              <span className="topic-composer-menu-main">
                <strong>{app.name}</strong>
                <span>{app.description ?? app.id}</span>
              </span>
            </button>
          ))}
        </div>
      )}

      {(status || error) && (
        <div
          className={error ? "chat-followup-error" : "topic-composer-status"}
        >
          {error ?? status}
        </div>
      )}
    </div>
  );
}
