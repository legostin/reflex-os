# Tailwind Workspace Shell Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebuild the Reflex OS main interface as a Tailwind-powered workspace with a left navigation tree and a right work area.

**Architecture:** Tailwind v4 is integrated through the official Vite plugin, then the current `ChatThread` monolith is carved into a workspace shell, tree navigation model, and reusable Tailwind UI primitives. Existing routes, panes, tabs, backend commands, project storage, utility storage, and Tauri window behavior remain the runtime contract while the UI layer is migrated.

**Tech Stack:** React 19, TypeScript, Vite 7, Tailwind CSS v4, Tauri v2, existing Rust commands.

---

## File Structure

- Create `src/styles.css`: Tailwind entry file and small global base styles.
- Modify `src/main.tsx`: import `src/styles.css`.
- Modify `src/App.tsx`: remove `App.css` import after global styles move.
- Modify `vite.config.ts`: add `@tailwindcss/vite`.
- Modify `package.json` and `pnpm-lock.yaml`: add Tailwind dependencies and static verification scripts.
- Create `scripts/check-tailwind-stack.mjs`: static guard for Tailwind Vite setup.
- Create `scripts/check-workspace-shell.mjs`: static guard for shell/sidebar/tree integration.
- Create `scripts/check-no-legacy-css-imports.mjs`: static guard for full migration away from component CSS imports.
- Create `src/components/ui/primitives.tsx`: shared Tailwind UI primitives.
- Create `src/components/workspace/workspaceTypes.ts`: shared route/project/thread/app types moved out of `ChatThread.tsx`.
- Create `src/components/workspace/navTree.ts`: deterministic tree builder for sections, projects, folders, topics, files, linked utilities, utilities, and utility folders.
- Create `src/components/workspace/WorkspaceShell.tsx`: two-column shell and titlebar.
- Create `src/components/workspace/WorkspaceSidebar.tsx`: left tree rendering and expand/collapse state.
- Create `src/components/workspace/WorkspaceMain.tsx`: right-side context row and pane container wrapper.
- Create `src/components/workspace/PaneTabs.tsx`: Tailwind pane tab row extracted from `ChatThread.tsx`.
- Modify `src/components/ChatThread.tsx`: use extracted workspace types, shell, sidebar, main, and pane tabs; remove old `Header`.
- Modify UI modules currently importing CSS:
  - `src/components/QuickPanel.tsx`
  - `src/components/automations/AutomationsScreen.tsx`
  - `src/components/automations/RunDetailPanel.tsx`
  - `src/components/automations/RunHistoryView.tsx`
  - `src/components/browser/BrowserScreen.tsx`
  - `src/components/files/FileActionsDrawer.tsx`
  - `src/components/memory/MemoryPanel.tsx`
  - `src/components/memory/MemoryEditor.tsx`
  - `src/components/memory/RecallView.tsx`
  - `src/components/memory/SearchBox.tsx`
  - `src/components/projects/SuggesterModal.tsx`
  - `src/components/settings/SettingsScreen.tsx`
  - `src/components/widgets/WidgetGrid.tsx`
  - `src/components/widgets/WidgetFrame.tsx`
  - `src/components/DiffPanel.tsx`
  - `src/components/TopicComposer.tsx`
- Delete or empty migrated CSS files:
  - `src/App.css`
  - `src/components/ChatThread.css`
  - `src/components/QuickPanel.css`
  - `src/components/automations/automations.css`
  - `src/components/browser/browser.css`
  - `src/components/files/file-drawer.css`
  - `src/components/memory/memory.css`
  - `src/components/projects/suggester.css`
  - `src/components/settings/settings.css`
  - `src/components/widgets/widgets.css`

## Task 1: Tailwind v4 Foundation

**Files:**
- Create: `scripts/check-tailwind-stack.mjs`
- Create: `src/styles.css`
- Modify: `package.json`
- Modify: `pnpm-lock.yaml`
- Modify: `vite.config.ts`
- Modify: `src/main.tsx`
- Modify: `src/App.tsx`
- Delete after migration in this task: `src/App.css`

- [ ] **Step 1: Write the failing Tailwind setup check**

Create `scripts/check-tailwind-stack.mjs`:

```js
import { readFileSync, existsSync } from "node:fs";
import { join } from "node:path";

const root = process.cwd();
const read = (path) => readFileSync(join(root, path), "utf8");
const failures = [];

const pkg = JSON.parse(read("package.json"));
const vite = read("vite.config.ts");
const main = read("src/main.tsx");
const app = read("src/App.tsx");
const stylesPath = "src/styles.css";
const styles = existsSync(join(root, stylesPath)) ? read(stylesPath) : "";

if (!pkg.devDependencies?.tailwindcss) failures.push("tailwindcss missing from devDependencies");
if (!pkg.devDependencies?.["@tailwindcss/vite"]) failures.push("@tailwindcss/vite missing from devDependencies");
if (!pkg.scripts?.["check:tailwind"]) failures.push("check:tailwind script missing");
if (!vite.includes('import tailwindcss from "@tailwindcss/vite"')) failures.push("vite config must import @tailwindcss/vite");
if (!vite.includes("tailwindcss()")) failures.push("vite plugins must include tailwindcss()");
if (!main.includes('import "./styles.css";')) failures.push("main.tsx must import src/styles.css");
if (app.includes('import "./App.css";')) failures.push("App.tsx must not import App.css");
if (!styles.includes('@import "tailwindcss";')) failures.push("src/styles.css must import tailwindcss");
if (!styles.includes("--color-reflex-bg")) failures.push("src/styles.css must define Reflex theme tokens");

if (failures.length > 0) {
  console.error(`Tailwind setup check failed:\n- ${failures.join("\n- ")}`);
  process.exit(1);
}

console.log("Tailwind setup check passed.");
```

- [ ] **Step 2: Wire the failing check into `package.json`**

Add the script before `check:bridge`:

```json
"check:tailwind": "node scripts/check-tailwind-stack.mjs",
"build": "npm run check:tailwind && npm run check:bridge && npm run check:dashboard && npm run check:dashboard:fixtures && tsc && vite build"
```

- [ ] **Step 3: Run the check and verify it fails**

Run: `npm run check:tailwind`

Expected: FAIL with missing `tailwindcss`, `@tailwindcss/vite`, Vite plugin, `src/styles.css`, and `App.css` removal messages.

- [ ] **Step 4: Install Tailwind v4 packages**

Run: `pnpm add -D tailwindcss @tailwindcss/vite`

Expected: `package.json` and `pnpm-lock.yaml` update with the two dev dependencies.

- [ ] **Step 5: Configure Vite**

Modify `vite.config.ts`:

```ts
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [react(), tailwindcss()],

  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
}));
```

- [ ] **Step 6: Add Tailwind global stylesheet**

Create `src/styles.css`:

```css
@import "tailwindcss";

@theme {
  --color-reflex-bg: #151618;
  --color-reflex-panel: #1c1d20;
  --color-reflex-panel-2: #22242a;
  --color-reflex-border: rgba(255, 255, 255, 0.1);
  --color-reflex-muted: rgba(245, 245, 247, 0.58);
  --color-reflex-text: #f5f5f7;
  --color-reflex-accent: #6ea8ff;
  --font-reflex: -apple-system, BlinkMacSystemFont, "SF Pro Text", system-ui, sans-serif;
}

html,
body,
#root {
  margin: 0;
  padding: 0;
  height: 100%;
  font-family: var(--font-reflex);
  font-size: 14px;
  line-height: 1.4;
  color: var(--color-reflex-text);
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
}

html[data-window="main"],
html[data-window="main"] body,
html[data-window="main"] #root {
  background: var(--color-reflex-bg);
}

html[data-window="quick"],
html[data-window="quick"] body,
html[data-window="quick"] #root {
  background: transparent;
  overflow: hidden;
}

* {
  box-sizing: border-box;
}
```

- [ ] **Step 7: Move global style import**

Modify `src/main.tsx` to import `styles.css` after `App`:

```ts
import App from "./App";
import "./styles.css";
import { I18nProvider } from "./i18n";
```

Remove `import "./App.css";` from `src/App.tsx`.

- [ ] **Step 8: Delete `src/App.css`**

Run: `git rm src/App.css`

- [ ] **Step 9: Verify Tailwind setup**

Run: `npm run check:tailwind`

Expected: PASS.

- [ ] **Step 10: Verify build**

Run: `pnpm run build`

Expected: PASS.

- [ ] **Step 11: Commit**

```bash
git add package.json pnpm-lock.yaml vite.config.ts src/main.tsx src/App.tsx src/styles.css scripts/check-tailwind-stack.mjs
git add -u src/App.css
git commit -m "Add Tailwind Vite foundation"
```

## Task 2: Shared Tailwind Primitives

**Files:**
- Create: `src/components/ui/primitives.tsx`
- Create: `scripts/check-ui-primitives.mjs`
- Modify: `package.json`

- [ ] **Step 1: Write the failing primitive check**

Create `scripts/check-ui-primitives.mjs`:

```js
import { readFileSync, existsSync } from "node:fs";
import { join } from "node:path";

const root = process.cwd();
const path = "src/components/ui/primitives.tsx";
const failures = [];
const source = existsSync(join(root, path)) ? readFileSync(join(root, path), "utf8") : "";
for (const name of ["cx", "Button", "IconButton", "Badge", "Panel", "TextInput", "ModalFrame"]) {
  if (!source.includes(`export function ${name}`) && !source.includes(`export const ${name}`)) {
    failures.push(`${name} missing from ${path}`);
  }
}
if (!source.includes("focus-visible:outline-none")) failures.push("focus-visible styling missing");
if (!source.includes("rounded-md")) failures.push("8px-or-less radius baseline missing");
if (failures.length > 0) {
  console.error(`UI primitive check failed:\n- ${failures.join("\n- ")}`);
  process.exit(1);
}
console.log("UI primitive check passed.");
```

- [ ] **Step 2: Wire the check into `package.json`**

Add:

```json
"check:ui": "node scripts/check-ui-primitives.mjs",
"build": "npm run check:tailwind && npm run check:ui && npm run check:bridge && npm run check:dashboard && npm run check:dashboard:fixtures && tsc && vite build"
```

- [ ] **Step 3: Run the check and verify it fails**

Run: `npm run check:ui`

Expected: FAIL because `src/components/ui/primitives.tsx` does not exist.

- [ ] **Step 4: Add UI primitives**

Create `src/components/ui/primitives.tsx`:

```tsx
import type { ButtonHTMLAttributes, HTMLAttributes, InputHTMLAttributes, ReactNode } from "react";

export function cx(...parts: Array<string | false | null | undefined>): string {
  return parts.filter(Boolean).join(" ");
}

const focusRing =
  "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-reflex-accent/70 focus-visible:ring-offset-0";

export function Button({
  className,
  variant = "secondary",
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: "primary" | "secondary" | "ghost" | "danger" }) {
  const variants = {
    primary: "border-reflex-accent/50 bg-reflex-accent/20 text-white hover:bg-reflex-accent/28",
    secondary: "border-white/10 bg-white/[0.045] text-white/80 hover:bg-white/[0.075]",
    ghost: "border-transparent bg-transparent text-white/62 hover:bg-white/[0.06] hover:text-white/86",
    danger: "border-red-400/35 bg-red-500/12 text-red-100 hover:bg-red-500/18",
  };
  return (
    <button
      {...props}
      className={cx(
        "inline-flex min-h-8 items-center justify-center gap-2 rounded-md border px-3 py-1.5 text-xs font-medium transition disabled:cursor-not-allowed disabled:opacity-45",
        focusRing,
        variants[variant],
        className,
      )}
    />
  );
}

export function IconButton({ className, ...props }: ButtonHTMLAttributes<HTMLButtonElement>) {
  return (
    <button
      {...props}
      className={cx(
        "inline-flex size-8 items-center justify-center rounded-md border border-white/10 bg-white/[0.04] text-white/70 transition hover:bg-white/[0.075] hover:text-white disabled:cursor-not-allowed disabled:opacity-45",
        focusRing,
        className,
      )}
    />
  );
}

export function Badge({ className, ...props }: HTMLAttributes<HTMLSpanElement>) {
  return (
    <span
      {...props}
      className={cx(
        "inline-flex items-center rounded-md border border-white/10 bg-white/[0.045] px-2 py-0.5 text-[11px] font-medium text-white/62",
        className,
      )}
    />
  );
}

export function Panel({ className, ...props }: HTMLAttributes<HTMLElement>) {
  return (
    <section
      {...props}
      className={cx("rounded-md border border-white/10 bg-white/[0.035]", className)}
    />
  );
}

export function TextInput({ className, ...props }: InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      {...props}
      className={cx(
        "min-h-8 rounded-md border border-white/10 bg-black/20 px-3 py-1.5 text-sm text-white placeholder:text-white/32",
        focusRing,
        className,
      )}
    />
  );
}

export function ModalFrame({
  title,
  children,
  footer,
  className,
}: {
  title: ReactNode;
  children: ReactNode;
  footer?: ReactNode;
  className?: string;
}) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/55 p-6">
      <section className={cx("max-h-[86vh] w-full max-w-2xl overflow-hidden rounded-md border border-white/12 bg-reflex-panel shadow-2xl", className)}>
        <header className="border-b border-white/10 px-5 py-4 text-base font-semibold text-white">{title}</header>
        <div className="max-h-[65vh] overflow-auto p-5">{children}</div>
        {footer ? <footer className="border-t border-white/10 px-5 py-4">{footer}</footer> : null}
      </section>
    </div>
  );
}
```

- [ ] **Step 5: Verify primitives**

Run: `npm run check:ui`

Expected: PASS.

- [ ] **Step 6: Verify build**

Run: `pnpm run build`

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add package.json scripts/check-ui-primitives.mjs src/components/ui/primitives.tsx
git commit -m "Add Tailwind UI primitives"
```

## Task 3: Workspace Types And Tree Model

**Files:**
- Create: `src/components/workspace/workspaceTypes.ts`
- Create: `src/components/workspace/navTree.ts`
- Create: `scripts/check-workspace-tree.mjs`
- Modify: `package.json`
- Modify: `src/components/ChatThread.tsx`

- [ ] **Step 1: Write the failing tree check**

Create `scripts/check-workspace-tree.mjs`:

```js
import { readFileSync, existsSync } from "node:fs";
import { join } from "node:path";

const root = process.cwd();
const read = (path) => readFileSync(join(root, path), "utf8");
const failures = [];

const typesPath = "src/components/workspace/workspaceTypes.ts";
const treePath = "src/components/workspace/navTree.ts";
const types = existsSync(join(root, typesPath)) ? read(typesPath) : "";
const tree = existsSync(join(root, treePath)) ? read(treePath) : "";
const chat = read("src/components/ChatThread.tsx");

for (const name of ["Route", "Project", "ProjectFolder", "Thread", "AppManifest", "AppFolder"]) {
  if (!types.includes(`export type ${name}`)) failures.push(`${name} not exported from workspaceTypes.ts`);
}
for (const token of ["WorkspaceTreeNode", "buildWorkspaceTree", "sections", "projects", "utilities", "topics", "files"]) {
  if (!tree.includes(token)) failures.push(`${token} missing from navTree.ts`);
}
if (!chat.includes('from "./workspace/workspaceTypes"')) failures.push("ChatThread must import shared workspace types");

if (failures.length > 0) {
  console.error(`Workspace tree check failed:\n- ${failures.join("\n- ")}`);
  process.exit(1);
}
console.log("Workspace tree check passed.");
```

- [ ] **Step 2: Wire the check into `package.json`**

Add:

```json
"check:workspace-tree": "node scripts/check-workspace-tree.mjs",
"build": "npm run check:tailwind && npm run check:ui && npm run check:workspace-tree && npm run check:bridge && npm run check:dashboard && npm run check:dashboard:fixtures && tsc && vite build"
```

- [ ] **Step 3: Run the check and verify it fails**

Run: `npm run check:workspace-tree`

Expected: FAIL because workspace type and tree files do not exist.

- [ ] **Step 4: Move shared types out of `ChatThread.tsx`**

Create `src/components/workspace/workspaceTypes.ts` by moving the existing type definitions for:

```ts
export type QuickContext = { frontmost_app: string | null; finder_target: string | null };
export type Project = { id: string; name: string; root: string; created_at_ms: number; folder_path?: string | null; sandbox?: string; mcp_servers?: Record<string, any> | null; description?: string | null; agent_instructions?: string | null; skills?: string[]; apps?: string[] };
export type ProjectFolder = { path: string; name: string; parent_path?: string | null; project_count?: number; created_at_ms?: number };
export type BrowserTabSnapshot = { url: string; title: string };
export type Thread = { id: string; project_id: string; project_name: string; prompt: string; cwd: string; ctx: QuickContext; created_at_ms: number; events: ThreadEvent[]; exit_code: number | null | undefined; done: boolean; session_id: string | null; title: string | null; goal: string | null; pending_questions: ThreadQuestion[]; plan_mode: boolean; plan_confirmed: boolean; source: string; browser_tabs: BrowserTabSnapshot[] };
export type Route = { kind: "home" } | { kind: "project"; project_id: string } | { kind: "topic"; thread_id: string } | { kind: "apps"; initialTemplate?: string; openCreate?: boolean; createRequestId?: number; project_id?: string } | { kind: "app"; app_id: string } | { kind: "memory"; project_id?: string; thread_id?: string } | { kind: "automations" } | { kind: "browser"; project_id?: string } | { kind: "settings" };
```

Also move the existing app and folder types needed by navigation:

```ts
export type AppManifest = { id: string; name: string; icon?: string | null; description?: string | null; entry: string; permissions: string[]; kind: string; created_at_ms: number; folder_path?: string | null; ready?: boolean; runtime?: "static" | "server" | "external" | string | null; apps?: string[] };
export type AppFolder = { path: string; name: string; parent_path?: string | null; created_at_ms?: number };
export type ThreadEvent = { seq: number; stream: "stdout" | "stderr" | "error" | "user"; raw: string; parsed: any | null };
export type ThreadQuestion = { question_id: string; method: string; params: any; thread_id: string | null };
```

Update `ChatThread.tsx` to import those types and remove the local duplicates.

- [ ] **Step 5: Add tree builder**

Create `src/components/workspace/navTree.ts`:

```ts
import type { AppFolder, AppManifest, Project, ProjectFolder, Route, Thread } from "./workspaceTypes";

export type WorkspaceTreeNodeKind =
  | "group"
  | "section"
  | "project-folder"
  | "project"
  | "project-topics"
  | "project-files"
  | "project-utilities"
  | "topic"
  | "utility-folder"
  | "utility";

export type WorkspaceTreeNode = {
  id: string;
  kind: WorkspaceTreeNodeKind;
  label: string;
  icon?: string;
  route?: Route;
  children?: WorkspaceTreeNode[];
  count?: number;
};

type BuildWorkspaceTreeInput = {
  projects: Project[];
  projectFolders: ProjectFolder[];
  threads: Thread[];
  apps: AppManifest[];
  appFolders: AppFolder[];
};

const byName = <T extends { name: string }>(items: T[]) =>
  items.slice().sort((a, b) => a.name.localeCompare(b.name));

export function buildWorkspaceTree({
  projects,
  projectFolders,
  threads,
  apps,
  appFolders,
}: BuildWorkspaceTreeInput): WorkspaceTreeNode[] {
  const topicsByProject = new Map<string, Thread[]>();
  for (const thread of threads) {
    const list = topicsByProject.get(thread.project_id) ?? [];
    list.push(thread);
    topicsByProject.set(thread.project_id, list);
  }

  const appsByFolder = new Map<string, AppManifest[]>();
  for (const app of apps) {
    const folder = app.folder_path ?? "";
    const list = appsByFolder.get(folder) ?? [];
    list.push(app);
    appsByFolder.set(folder, list);
  }

  const toProjectNode = (project: Project): WorkspaceTreeNode => {
    const projectThreads = (topicsByProject.get(project.id) ?? []).slice().sort(
      (a, b) => b.created_at_ms - a.created_at_ms,
    );
    const linkedApps = apps.filter((app) => project.apps?.includes(app.id));
    return {
      id: `project:${project.id}`,
      kind: "project" as const,
      label: project.name,
      icon: "folder",
      route: { kind: "project", project_id: project.id },
      children: [
        {
          id: `project:${project.id}:topics`,
          kind: "project-topics" as const,
          label: "Topics",
          count: projectThreads.length,
          children: projectThreads.map((thread) => ({
            id: `topic:${thread.id}`,
            kind: "topic" as const,
            label: thread.title ?? thread.prompt.slice(0, 42) || thread.id,
            route: { kind: "topic", thread_id: thread.id },
          })),
        },
        {
          id: `project:${project.id}:files`,
          kind: "project-files" as const,
          label: "Files",
          route: { kind: "project", project_id: project.id },
        },
        {
          id: `project:${project.id}:utilities`,
          kind: "project-utilities" as const,
          label: "Utilities",
          count: linkedApps.length,
          route: { kind: "apps", project_id: project.id },
          children: linkedApps.map((app) => ({
            id: `project:${project.id}:utility:${app.id}`,
            kind: "utility" as const,
            label: app.name,
            icon: app.icon ?? undefined,
            route: { kind: "app", app_id: app.id },
          })),
        },
      ],
    };
  };

  const projectsByFolder = new Map<string, Project[]>();
  for (const project of projects) {
    const folder = project.folder_path ?? "";
    const list = projectsByFolder.get(folder) ?? [];
    list.push(project);
    projectsByFolder.set(folder, list);
  }

  const projectFolderNodes = byName(projectFolders).map((folder) => ({
    id: `project-folder:${folder.path}`,
    kind: "project-folder" as const,
    label: folder.name,
    count: folder.project_count,
    children: byName(projectsByFolder.get(folder.path) ?? []).map(toProjectNode),
  }));

  const rootProjectNodes = byName(projectsByFolder.get("") ?? []).map(toProjectNode);

  const utilityFolderNodes = byName(appFolders).map((folder) => ({
    id: `utility-folder:${folder.path}`,
    kind: "utility-folder" as const,
    label: folder.name,
    children: byName(appsByFolder.get(folder.path) ?? []).map((app) => ({
      id: `utility:${app.id}`,
      kind: "utility" as const,
      label: app.name,
      icon: app.icon ?? undefined,
      route: { kind: "app", app_id: app.id },
    })),
  }));

  const rootUtilities = byName(appsByFolder.get("") ?? []).map((app) => ({
    id: `utility:${app.id}`,
    kind: "utility" as const,
    label: app.name,
    icon: app.icon ?? undefined,
    route: { kind: "app", app_id: app.id },
  }));

  return [
    {
      id: "sections",
      kind: "group",
      label: "Sections",
      children: [
        { id: "section:home", kind: "section", label: "Home", route: { kind: "home" } },
        { id: "section:memory", kind: "section", label: "Memory", route: { kind: "memory" } },
        { id: "section:automations", kind: "section", label: "Automations", route: { kind: "automations" } },
        { id: "section:browser", kind: "section", label: "Browser", route: { kind: "browser" } },
        { id: "section:settings", kind: "section", label: "Settings", route: { kind: "settings" } },
      ],
    },
    {
      id: "projects",
      kind: "group",
      label: "Projects",
      count: projects.length,
      children: [...projectFolderNodes, ...rootProjectNodes],
    },
    {
      id: "utilities",
      kind: "group",
      label: "Utilities",
      count: apps.length,
      children: [...utilityFolderNodes, ...rootUtilities],
    },
  ];
}
```

Project grouping is deterministic: `Project.folder_path` points to a `ProjectFolder.path`, and projects without `folder_path` render directly under the Projects root. This migration does not add backend folder fields; it consumes the existing project folder command output and keeps missing folder values root-level.

- [ ] **Step 6: Verify tree check**

Run: `npm run check:workspace-tree`

Expected: PASS.

- [ ] **Step 7: Verify TypeScript**

Run: `pnpm run build`

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add package.json scripts/check-workspace-tree.mjs src/components/workspace/workspaceTypes.ts src/components/workspace/navTree.ts src/components/ChatThread.tsx
git commit -m "Extract workspace types and tree model"
```

## Task 4: Workspace Shell And Sidebar

**Files:**
- Create: `src/components/workspace/WorkspaceShell.tsx`
- Create: `src/components/workspace/WorkspaceSidebar.tsx`
- Create: `src/components/workspace/WorkspaceMain.tsx`
- Create: `scripts/check-workspace-shell.mjs`
- Modify: `package.json`
- Modify: `src/components/ChatThread.tsx`

- [ ] **Step 1: Write the failing shell check**

Create `scripts/check-workspace-shell.mjs`:

```js
import { readFileSync, existsSync } from "node:fs";
import { join } from "node:path";

const root = process.cwd();
const read = (path) => readFileSync(join(root, path), "utf8");
const failures = [];
const files = [
  "src/components/workspace/WorkspaceShell.tsx",
  "src/components/workspace/WorkspaceSidebar.tsx",
  "src/components/workspace/WorkspaceMain.tsx",
];
for (const file of files) {
  if (!existsSync(join(root, file))) failures.push(`${file} missing`);
}
const shell = existsSync(join(root, files[0])) ? read(files[0]) : "";
const sidebar = existsSync(join(root, files[1])) ? read(files[1]) : "";
const main = existsSync(join(root, files[2])) ? read(files[2]) : "";
const chat = read("src/components/ChatThread.tsx");

if (!shell.includes("data-tauri-drag-region")) failures.push("WorkspaceShell must preserve drag regions");
if (!sidebar.includes("WorkspaceTreeNode")) failures.push("WorkspaceSidebar must render workspace tree nodes");
if (!main.includes("children")) failures.push("WorkspaceMain must render children");
if (!chat.includes("<WorkspaceShell")) failures.push("ChatThread must render WorkspaceShell");
if (chat.includes("function Header(")) failures.push("old Header function must be removed");

if (failures.length > 0) {
  console.error(`Workspace shell check failed:\n- ${failures.join("\n- ")}`);
  process.exit(1);
}
console.log("Workspace shell check passed.");
```

- [ ] **Step 2: Wire the check into `package.json`**

Add:

```json
"check:workspace-shell": "node scripts/check-workspace-shell.mjs",
"build": "npm run check:tailwind && npm run check:ui && npm run check:workspace-tree && npm run check:workspace-shell && npm run check:bridge && npm run check:dashboard && npm run check:dashboard:fixtures && tsc && vite build"
```

- [ ] **Step 3: Run the check and verify it fails**

Run: `npm run check:workspace-shell`

Expected: FAIL because shell components do not exist.

- [ ] **Step 4: Implement `WorkspaceShell`**

Create `src/components/workspace/WorkspaceShell.tsx`:

```tsx
import type { ReactNode } from "react";
import { cx } from "../ui/primitives";

export function WorkspaceShell({
  sidebar,
  titlebar,
  children,
}: {
  sidebar: ReactNode;
  titlebar: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="flex h-screen w-screen overflow-hidden bg-reflex-bg text-reflex-text">
      <aside className="flex w-[280px] shrink-0 flex-col border-r border-white/10 bg-reflex-panel/95">
        <div className="h-7 shrink-0" data-tauri-drag-region />
        {sidebar}
      </aside>
      <section className="flex min-w-0 flex-1 flex-col">
        <div className="min-h-10 shrink-0 border-b border-white/10 bg-reflex-bg/96" data-tauri-drag-region>
          <div className={cx("flex min-h-10 items-center px-4", "[&_*]:select-none")}>{titlebar}</div>
        </div>
        <div className="min-h-0 flex-1 overflow-hidden">{children}</div>
      </section>
    </div>
  );
}
```

- [ ] **Step 5: Implement `WorkspaceMain`**

Create `src/components/workspace/WorkspaceMain.tsx`:

```tsx
import type { ReactNode } from "react";
import { Badge, Button } from "../ui/primitives";
import type { Route } from "./workspaceTypes";

export function WorkspaceMain({
  title,
  route,
  onAddPane,
  children,
}: {
  title: string;
  route: Route;
  onAddPane: () => void;
  children: ReactNode;
}) {
  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex min-h-12 shrink-0 items-center justify-between gap-3 border-b border-white/10 bg-reflex-bg px-4">
        <div className="min-w-0">
          <div className="truncate text-sm font-semibold text-white">{title}</div>
          <div className="mt-0.5 text-[11px] uppercase tracking-[0.08em] text-white/38">{route.kind}</div>
        </div>
        <div className="flex items-center gap-2">
          <Badge>Workspace</Badge>
          <Button variant="secondary" onClick={onAddPane}>New pane</Button>
        </div>
      </div>
      <div className="min-h-0 flex-1 overflow-hidden">{children}</div>
    </div>
  );
}
```

- [ ] **Step 6: Implement `WorkspaceSidebar`**

Create `src/components/workspace/WorkspaceSidebar.tsx`:

```tsx
import { useMemo, useState } from "react";
import { Badge, Button, cx } from "../ui/primitives";
import type { Route } from "./workspaceTypes";
import type { WorkspaceTreeNode } from "./navTree";

export function WorkspaceSidebar({
  tree,
  activeRouteKey,
  routeKey,
  onNavigate,
  onCreateProject,
}: {
  tree: WorkspaceTreeNode[];
  activeRouteKey: string;
  routeKey: (route: Route) => string;
  onNavigate: (route: Route) => void;
  onCreateProject: () => void;
}) {
  const initialExpanded = useMemo(() => new Set(["sections", "projects", "utilities"]), []);
  const [expanded, setExpanded] = useState(initialExpanded);

  const toggle = (id: string) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="border-b border-white/10 px-3 pb-3" data-tauri-drag-region>
        <div className="text-[11px] font-semibold uppercase tracking-[0.14em] text-white/36">Reflex OS</div>
        <div className="mt-2 flex items-center gap-2">
          <Button className="flex-1" variant="primary" onClick={onCreateProject}>New project</Button>
        </div>
      </div>
      <nav className="min-h-0 flex-1 overflow-auto px-2 py-3">
        {tree.map((node) => (
          <TreeNode
            key={node.id}
            node={node}
            depth={0}
            expanded={expanded}
            activeRouteKey={activeRouteKey}
            routeKey={routeKey}
            onToggle={toggle}
            onNavigate={onNavigate}
          />
        ))}
      </nav>
    </div>
  );
}

function TreeNode({
  node,
  depth,
  expanded,
  activeRouteKey,
  routeKey,
  onToggle,
  onNavigate,
}: {
  node: WorkspaceTreeNode;
  depth: number;
  expanded: Set<string>;
  activeRouteKey: string;
  routeKey: (route: Route) => string;
  onToggle: (id: string) => void;
  onNavigate: (route: Route) => void;
}) {
  const hasChildren = !!node.children?.length;
  const open = expanded.has(node.id);
  const active = node.route ? routeKey(node.route) === activeRouteKey : false;
  const action = () => {
    if (hasChildren && !node.route) onToggle(node.id);
    else if (node.route) onNavigate(node.route);
    else onToggle(node.id);
  };

  return (
    <div>
      <button
        className={cx(
          "flex min-h-8 w-full items-center gap-2 rounded-md px-2 text-left text-xs transition",
          active ? "bg-reflex-accent/18 text-white" : "text-white/64 hover:bg-white/[0.06] hover:text-white/88",
        )}
        style={{ paddingLeft: 8 + depth * 14 }}
        onClick={action}
      >
        <span className="w-3 text-white/34">{hasChildren ? (open ? "▾" : "▸") : ""}</span>
        <span className="min-w-0 flex-1 truncate">{node.label}</span>
        {typeof node.count === "number" ? <Badge className="px-1.5 py-0 text-[10px]">{node.count}</Badge> : null}
      </button>
      {hasChildren && open ? (
        <div className="mt-0.5">
          {node.children!.map((child) => (
            <TreeNode
              key={child.id}
              node={child}
              depth={depth + 1}
              expanded={expanded}
              activeRouteKey={activeRouteKey}
              routeKey={routeKey}
              onToggle={onToggle}
              onNavigate={onNavigate}
            />
          ))}
        </div>
      ) : null}
    </div>
  );
}
```

- [ ] **Step 7: Integrate shell in `ChatThread.tsx`**

In `ChatThread.tsx`:

- Import `WorkspaceShell`, `WorkspaceSidebar`, `WorkspaceMain`, and `buildWorkspaceTree`.
- Add state for `appFolders`, `projectFolders`, and `appsForNav`.
- Load them with existing commands where projects and threads are loaded:

```ts
const [projectFolders, setProjectFolders] = useState<ProjectFolder[]>([]);
const [appFolders, setAppFolders] = useState<AppFolder[]>([]);
const [appsForNav, setAppsForNav] = useState<AppManifest[]>([]);
```

Use:

```ts
const workspaceTree = useMemo(
  () => buildWorkspaceTree({ projects, projectFolders, threads, apps: appsForNav, appFolders }),
  [projects, projectFolders, threads, appsForNav, appFolders],
);
```

Replace the root JSX with:

```tsx
<WorkspaceShell
  sidebar={
    <WorkspaceSidebar
      tree={workspaceTree}
      activeRouteKey={routeKey(currentRoute)}
      routeKey={routeKey}
      onNavigate={navigate}
      onCreateProject={() => void createNewProject()}
    />
  }
  titlebar={<span className="truncate text-xs text-white/42">{tabLabel(currentRoute, projects, threads, t)}</span>}
>
  <WorkspaceMain
    title={tabLabel(currentRoute, projects, threads, t)}
    route={currentRoute}
    onAddPane={addPane}
  >
    <div className="flex h-full min-h-0 flex-row" ref={containerRef}>
      {/* existing panes/new-pane drop zone move here */}
    </div>
  </WorkspaceMain>
</WorkspaceShell>
```

Remove the old `Header` component and old `<div className="chat-titlebar" />`.

- [ ] **Step 8: Verify shell check**

Run: `npm run check:workspace-shell`

Expected: PASS.

- [ ] **Step 9: Verify build**

Run: `pnpm run build`

Expected: PASS.

- [ ] **Step 10: Commit**

```bash
git add package.json scripts/check-workspace-shell.mjs src/components/workspace/WorkspaceShell.tsx src/components/workspace/WorkspaceSidebar.tsx src/components/workspace/WorkspaceMain.tsx src/components/ChatThread.tsx
git commit -m "Add Tailwind workspace shell"
```

## Task 5: Migrate Pane Tabs And Main Workspace Frame

**Files:**
- Create: `src/components/workspace/PaneTabs.tsx`
- Modify: `src/components/ChatThread.tsx`
- Modify: `scripts/check-workspace-shell.mjs`

- [ ] **Step 1: Extend shell check for pane tabs**

Add to `scripts/check-workspace-shell.mjs`:

```js
const paneTabsPath = "src/components/workspace/PaneTabs.tsx";
if (!existsSync(join(root, paneTabsPath))) failures.push(`${paneTabsPath} missing`);
const paneTabs = existsSync(join(root, paneTabsPath)) ? read(paneTabsPath) : "";
if (!paneTabs.includes("export function PaneTabs")) failures.push("PaneTabs export missing");
if (chat.includes('className="tabs-row"')) failures.push("legacy tabs-row usage must be removed");
```

- [ ] **Step 2: Run check and verify it fails**

Run: `npm run check:workspace-shell`

Expected: FAIL because `PaneTabs.tsx` is missing and legacy `tabs-row` remains.

- [ ] **Step 3: Create Tailwind `PaneTabs`**

Move `PaneTabsRow` behavior from `ChatThread.tsx` to `src/components/workspace/PaneTabs.tsx`, preserving:

- tab activate
- tab close
- pane close
- drag/drop payload type
- route labels/icons

Use Tailwind classes equivalent to:

```tsx
className="flex min-h-9 shrink-0 items-end gap-1 overflow-x-auto border-b border-white/10 bg-reflex-panel-2/70 px-2 pt-1"
```

Use tab buttons:

```tsx
className={cx(
  "group inline-flex max-w-56 items-center gap-1 rounded-t-md border border-b-0 px-2.5 py-1.5 text-xs transition",
  active ? "border-white/14 bg-reflex-bg text-white" : "border-white/8 bg-white/[0.035] text-white/55 hover:bg-white/[0.06] hover:text-white/82",
)}
```

- [ ] **Step 4: Replace `PaneTabsRow` usage**

In `PaneView`, replace `<PaneTabsRow ... />` with `<PaneTabs ... />` and delete the local `PaneTabsRow` function.

- [ ] **Step 5: Verify**

Run: `npm run check:workspace-shell`

Expected: PASS.

Run: `pnpm run build`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add scripts/check-workspace-shell.mjs src/components/workspace/PaneTabs.tsx src/components/ChatThread.tsx
git commit -m "Migrate pane tabs to Tailwind"
```

## Task 6: Migrate Main `ChatThread` Screens To Tailwind

**Files:**
- Modify: `src/components/ChatThread.tsx`
- Modify: `scripts/check-no-legacy-css-imports.mjs`
- Modify: `package.json`

- [ ] **Step 1: Write the failing legacy CSS import check**

Create `scripts/check-no-legacy-css-imports.mjs`:

```js
import { readFileSync } from "node:fs";
import { execSync } from "node:child_process";

const files = execSync("rg -l \"import .*\\\\.css\" src", { encoding: "utf8" })
  .split("\n")
  .filter(Boolean);
const allowed = new Set(["src/styles.css"]);
const failures = [];

for (const file of files) {
  const source = readFileSync(file, "utf8");
  for (const match of source.matchAll(/import\s+[\"'](.+\.css)[\"'];/g)) {
    if (!allowed.has(match[1]) && !file.endsWith("main.tsx")) {
      failures.push(`${file} imports ${match[1]}`);
    }
  }
}

if (failures.length > 0) {
  console.error(`Legacy CSS import check failed:\n- ${failures.join("\n- ")}`);
  process.exit(1);
}
console.log("Legacy CSS import check passed.");
```

- [ ] **Step 2: Wire the check into `package.json` but keep it last**

Add:

```json
"check:no-legacy-css": "node scripts/check-no-legacy-css-imports.mjs"
```

Do not add it to `build` until Task 10, because screens are still migrating.

- [ ] **Step 3: Migrate `ChatThread.tsx` route shell classes**

Replace these legacy class groups with Tailwind:

- `.pane`, `.pane-focused`, `.pane-body`, `.route-pane`
- `.pane-divider`, `.pane-newzone`
- `.chat-empty`, `.chat-list`, `.chat-item`, `.chat-events`
- `.chat-followup-*`, `.question-*`, `.plan-banner`
- modal classes used directly in `ChatThread.tsx`

Use primitives from `src/components/ui/primitives.tsx` for buttons, panels, badges, text inputs, and modals.

- [ ] **Step 4: Remove `import "./ChatThread.css";`**

Remove the CSS import from `ChatThread.tsx`.

- [ ] **Step 5: Verify**

Run: `pnpm run build`

Expected: PASS.

Run: `npm run check:no-legacy-css`

Expected: still FAIL because other component CSS imports remain. Confirm `src/components/ChatThread.tsx` is not listed.

- [ ] **Step 6: Commit**

```bash
git add package.json scripts/check-no-legacy-css-imports.mjs src/components/ChatThread.tsx
git commit -m "Migrate core thread workspace to Tailwind"
```

## Task 7: Migrate Utility And Project Screens

**Files:**
- Modify: `src/components/ChatThread.tsx`
- Modify: `src/components/projects/SuggesterModal.tsx`
- Delete: `src/components/projects/suggester.css`

- [ ] **Step 1: Migrate Home, Project, Apps, App Viewer, and Suggester UI**

In `ChatThread.tsx`, migrate:

- `HomeScreen`
- `ProjectScreen`
- `AppsScreen`
- `AppViewer`
- dashboard widget wrappers owned by `ChatThread.tsx`
- project modals
- app modals

In `SuggesterModal.tsx`, replace `.modal-*` and `.suggester-*` classes with Tailwind primitives and remove `import "./suggester.css";`.

- [ ] **Step 2: Delete `suggester.css`**

Run: `git rm src/components/projects/suggester.css`

- [ ] **Step 3: Verify**

Run: `pnpm run build`

Expected: PASS.

Run: `npm run check:no-legacy-css`

Expected: FAIL for remaining non-migrated CSS imports, but not for `suggester.css`.

- [ ] **Step 4: Commit**

```bash
git add src/components/ChatThread.tsx src/components/projects/SuggesterModal.tsx
git add -u src/components/projects/suggester.css
git commit -m "Migrate project and utility screens to Tailwind"
```

## Task 8: Migrate Memory, Browser, Automations, Settings, Files, Widgets, Composer, Diff

**Files:**
- Modify: `src/components/automations/AutomationsScreen.tsx`
- Modify: `src/components/automations/RunDetailPanel.tsx`
- Modify: `src/components/automations/RunHistoryView.tsx`
- Modify: `src/components/browser/BrowserScreen.tsx`
- Modify: `src/components/files/FileActionsDrawer.tsx`
- Modify: `src/components/memory/MemoryPanel.tsx`
- Modify: `src/components/memory/MemoryEditor.tsx`
- Modify: `src/components/memory/RecallView.tsx`
- Modify: `src/components/memory/SearchBox.tsx`
- Modify: `src/components/settings/SettingsScreen.tsx`
- Modify: `src/components/widgets/WidgetGrid.tsx`
- Modify: `src/components/widgets/WidgetFrame.tsx`
- Modify: `src/components/TopicComposer.tsx`
- Modify: `src/components/DiffPanel.tsx`
- Delete migrated CSS files listed in the file structure.

- [ ] **Step 1: Migrate each module off CSS imports**

For each file, remove its CSS import and replace class names with Tailwind classes or primitives:

- `AutomationsScreen`: table, summary cards, toolbar, state pills.
- `RunDetailPanel`: right drawer, step list, error/output blocks.
- `RunHistoryView`: filters, runs table, status pills.
- `BrowserScreen`: tab bar, URL bar, stage, chat bar.
- `FileActionsDrawer`: modal drawer, action rows, tags.
- `MemoryPanel`, `MemoryEditor`, `RecallView`, `SearchBox`: memory layout, inputs, result lists.
- `SettingsScreen`: tabs, sections, capability cards, logs table.
- `WidgetGrid`, `WidgetFrame`: widget grid and iframe frame.
- `TopicComposer`: composer toolbar, textarea, attachments, menu.
- `DiffPanel`: modal, file/hunk rows, commit controls.

Use consistent Tailwind patterns:

```tsx
"rounded-md border border-white/10 bg-white/[0.035]"
"text-white/62"
"hover:bg-white/[0.06]"
"focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-reflex-accent/70"
```

- [ ] **Step 2: Delete migrated CSS files**

Run:

```bash
git rm src/components/QuickPanel.css
git rm src/components/automations/automations.css
git rm src/components/browser/browser.css
git rm src/components/files/file-drawer.css
git rm src/components/memory/memory.css
git rm src/components/settings/settings.css
git rm src/components/widgets/widgets.css
```

If `ChatThread.css` is fully unused after Task 7, also run:

```bash
git rm src/components/ChatThread.css
```

- [ ] **Step 3: Verify no CSS imports remain except `src/styles.css`**

Run: `npm run check:no-legacy-css`

Expected: PASS.

- [ ] **Step 4: Verify build**

Run: `pnpm run build`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/components
git add -u src/components
git commit -m "Migrate remaining screens to Tailwind"
```

## Task 9: Quick Panel Tailwind Migration

**Files:**
- Modify: `src/components/QuickPanel.tsx`
- Delete: `src/components/QuickPanel.css`

- [ ] **Step 1: Migrate QuickPanel**

Remove `import "./QuickPanel.css";`.

Use Tailwind classes that preserve transparent quick window behavior:

```tsx
<div className="flex h-screen w-screen items-center justify-center bg-transparent p-2">
  <div className="w-full rounded-md border border-white/12 bg-reflex-panel/96 p-3 shadow-2xl">
```

Keep input focus, project selector, app selector, error state, and action buttons.

- [ ] **Step 2: Delete QuickPanel CSS**

Run: `git rm src/components/QuickPanel.css`

- [ ] **Step 3: Verify**

Run: `npm run check:no-legacy-css`

Expected: PASS.

Run: `pnpm run build`

Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/components/QuickPanel.tsx
git add -u src/components/QuickPanel.css
git commit -m "Migrate quick panel to Tailwind"
```

## Task 10: Final Cleanup And Verification

**Files:**
- Modify: `package.json`
- Modify: any files with stale imports or class helpers

- [ ] **Step 1: Add legacy CSS check to build permanently**

Update `package.json` build script:

```json
"build": "npm run check:tailwind && npm run check:ui && npm run check:workspace-tree && npm run check:workspace-shell && npm run check:no-legacy-css && npm run check:bridge && npm run check:dashboard && npm run check:dashboard:fixtures && tsc && vite build"
```

- [ ] **Step 2: Run static checks**

Run:

```bash
npm run check:tailwind
npm run check:ui
npm run check:workspace-tree
npm run check:workspace-shell
npm run check:no-legacy-css
```

Expected: all PASS.

- [ ] **Step 3: Run full frontend build**

Run: `pnpm run build`

Expected: PASS.

- [ ] **Step 4: Run backend tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`

Expected: PASS. If sandbox blocks a local listener test, rerun with escalation.

- [ ] **Step 5: Run diff hygiene check**

Run: `git diff --check`

Expected: no output.

- [ ] **Step 6: Start the app**

Run: `pnpm tauri dev`

Expected:

- Vite ready at `http://localhost:1420/`.
- `target/debug/reflex-os` runs.
- app-server initializes.

- [ ] **Step 7: Verify dev server**

Run: `curl -I http://localhost:1420/`

Expected: `HTTP/1.1 200 OK`.

- [ ] **Step 8: Manual verification**

Check in the running app:

- Sidebar tree shows Sections, Projects, and Utilities.
- Project folders and utility folders are visible.
- Project click opens project route in the right workspace.
- Topic click opens topic route.
- Utility click opens utility route.
- Pane tabs can open, close, drag, and move between panes.
- Window drags from titlebar/sidebar header empty space.
- Buttons and inputs in the sidebar do not drag the window.
- Existing project folder and utility folder drag/drop behavior still works in their respective management screens.
- Quick panel still opens and submits.

- [ ] **Step 9: Commit final cleanup**

```bash
git add package.json pnpm-lock.yaml src scripts
git commit -m "Complete Tailwind workspace UI migration"
```

- [ ] **Step 10: Push**

Run: `git push`

Expected: branch pushes to `origin/main`.

## Self-Review

- Spec coverage: Tailwind Vite setup is covered by Task 1; shared primitives by Task 2; left tree data model by Task 3; shell/sidebar by Task 4; panes by Task 5; full UI migration by Tasks 6-9; verification by Task 10.
- Backend boundary: no backend storage or app-server behavior changes are planned.
- Window dragging: preserved in Task 4 and manually verified in Task 10.
- Risk: The largest risk is the size of `ChatThread.tsx`. The plan reduces this by extracting shared workspace files before screen migration and by committing after each bounded migration slice.
