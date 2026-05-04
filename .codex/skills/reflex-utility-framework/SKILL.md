---
name: "reflex-utility-framework"
description: "Use when generating, scaffolding, or refactoring a Reflex OS utility (a generated app under `.reflex` apps or `src-tauri/builtin_apps/*`). Build with React + Tailwind on top of `reflex-os-utility-framework` so every utility shares one bridge wrapper, one design system, and one set of high-level building blocks instead of re-implementing them in raw HTML."
---

# Reflex Utility Framework

This skill is the canonical way to build new Reflex OS utilities. It exists
because the previous default — a single `index.html` with inline `<script>`
and ad-hoc styles — duplicates bridge plumbing across utilities, skips type
safety, and re-invents the same dashboard widgets every time.

> npm package: [`reflex-os-utility-framework`](https://github.com/legostin/reflex-os-utility-framework)
>
> Repo path inside Reflex: `.codex/skills/reflex-utility-framework/SKILL.md`

## When to use

Trigger this skill when the user (or a higher-level workflow) asks for any of:

- A new Reflex utility, generated app, or "small dashboard" inside the host.
- Refactoring an existing built-in app under `src-tauri/builtin_apps/*` or a
  user app under `.reflex/apps/<id>/` to be more maintainable.
- Adding a *real* React UI inside a Reflex utility instead of vanilla DOM.
- Wrapping a Connected App / repo-wrapper template in a typed UI.

## Decision tree (start here)

1. **Is the utility one HTML page with no logic?** Plain HTML is still fine
   for trivial tools (a "Notify me" button). Skip the framework.
2. **Does it touch memory, topics, events, agent, storage, scheduler, or
   project files?** Use the framework. The bridge wrappers carry the right
   permission semantics and types, so you do not have to re-derive defaults
   like `scope = "project"` or how to sanitize linked-project data.
3. **Does it need a dashboard or multi-section UI?** Use the framework. The
   `AppShell`, `Section`, `SplitGrid`, and primitives produce a layout that
   matches the rest of Reflex without bespoke CSS.
4. **Is it a Connected App or repo-wrapper?** Use the framework AND the host
   templates together — the framework gives you the typed bridge, the
   template handles the integration profile and permission requests.

## Quick start

```sh
pnpm create vite my-utility -- --template react-ts
cd my-utility
pnpm add reflex-os-utility-framework
pnpm add -D tailwindcss postcss autoprefixer
npx tailwindcss init -p
```

`tailwind.config.cjs`:

```js
/** @type {import('tailwindcss').Config} */
module.exports = {
  presets: [require("reflex-os-utility-framework/tailwind-preset")],
  content: [
    "./index.html",
    "./src/**/*.{ts,tsx,html}",
    "./node_modules/reflex-os-utility-framework/dist/**/*.{js,cjs}",
  ],
};
```

`src/index.css`:

```css
@tailwind base;
@tailwind components;
@tailwind utilities;
@import "reflex-os-utility-framework/styles.css";
```

`src/main.tsx`:

```tsx
import * as React from "react";
import { createRoot } from "react-dom/client";
import {
  AppShell,
  MemoryComposer,
  MemoryNoteList,
  ProjectPicker,
  ReflexProvider,
  Section,
  SplitGrid,
} from "reflex-os-utility-framework";
import "./index.css";

function App() {
  const [projectId, setProjectId] = React.useState<string | "global">("global");
  return (
    <AppShell title="My Utility" subtitle="Replace this subtitle.">
      <Section title="Project">
        <ProjectPicker value={projectId} onChange={setProjectId} includeGlobal />
      </Section>
      <SplitGrid>
        <Section title="Save"><MemoryComposer source="app:my-utility" /></Section>
        <Section title="Notes"><MemoryNoteList scope="project" /></Section>
      </SplitGrid>
    </AppShell>
  );
}

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ReflexProvider>
      <App />
    </ReflexProvider>
  </React.StrictMode>,
);
```

Build the utility, copy the resulting `dist/` into the app folder as
`index.html` + `assets/`, and reference it from `manifest.json`.

## Building blocks (use these, do not re-derive)

| Layer | Pick this when… |
|---|---|
| `reflexInvoke(method, params)` | You need a one-off bridge call and there is no typed wrapper yet. |
| `bridge.<area>` (`memory`, `topics`, `events`, `agent`, `storage`, `fs`, `projectFiles`, `manifest`, `actions`, `widgets`, `scheduler`, `permissions`, `network`, `system`, `projects`) | Anywhere you would have written `window.reflexMemorySave(...)` etc. — the wrappers add types and inherit host defaults. |
| `useMemoryNotes`, `useStorage`, `useEvent`, `useTopics`, `useAgentStream`, `useSystemContext`, `useBridgeCatalog`, `useProjects`, `useManifest` | Reactive state. Each wraps a bridge area + `useAsync` cache. |
| Primitives: `Button`, `Card`, `Section`, `Field`, `Input`, `Textarea`, `Select`, `Badge`, `EmptyState`, `Toolbar`, `StatusLine` | Anywhere you would have hand-rolled a `<button>` or `<input>`. |
| Layout: `AppShell`, `SplitGrid` | Top-level utility chrome. Mirrors built-in apps. |
| Bricks: `MemoryComposer`, `MemoryNoteList`, `TopicsList`, `EventLog`, `StorageBrowser`, `AgentChat`, `BridgeMethodPicker`, `ActionRunner`, `PermissionRequestBanner`, `MarkdownView`, `ProjectPicker` | High-level features. Compose them; do NOT re-implement. |

## Standardised flows

### Memory

```ts
import { bridge } from "reflex-os-utility-framework/bridge";
await bridge.memory.save({
  scope: "project",
  kind: "fact",
  name: "Onboarding plan",
  body: "...",
  tags: ["onboarding"],
  source: "app:my-utility",          // <- always include `source` so audit trails
                                      //    show which utility wrote the note.
});
```

- `scope` defaults to "project". For app-only notes pass `scope: "topic"`
  with the active `threadId`; for cross-project notes pass `"global"`.
- Always set `source: "app:<app-id>"` so the host can tell utility-written
  notes apart from agent-written ones.

### Topics (events) and topics (threads)

Reflex has two "topic" namespaces: inter-app pubsub (`events.*`) and agent
threads (`topics.*`). The framework keeps them separate:

```ts
import { events } from "reflex-os-utility-framework/bridge";
const stop = await events.subscribe("ops.health", (event) => render(event.payload));
// later:
stop();
```

```ts
import { topics } from "reflex-os-utility-framework/bridge";
const list = await topics.list({ projectId });
await topics.open({ threadId: list[0].thread_id! });
```

### Storage

Use `useStorage(key, fallback)`. It debounces nothing — every `set` is a
durable write to `storage.set`. Avoid `localStorage` inside utilities;
`storage.*` is the only path that survives app restarts and trash/restore.

### Agent

`useAgentStream` for streamed turns; `useAgentTask` for one-shot results.
Set `cwd` to the linked project root when the prompt should run with
project MCP/skills/memory; pass `includeContext: false` only when you
genuinely need a raw prompt.

### Manifest mutations

Whenever the utility writes to its own manifest (actions, schedules,
permissions, network hosts), call `bridge.manifest.update({ patch })` —
the framework's hooks will see fresh manifest data on the next render via
`useManifest().reload()`.

## Permission discipline

When new bridge methods enter the utility, update `manifest.permissions`
to match. Do not catch-all with `*:*`. The bridge methods themselves
document the required grants — a quick reference:

| Bridge method | Required grant(s) |
|---|---|
| `memory.save` (project) | `memory.project:<id>` or `memory.project:*` |
| `memory.save` (global) | `memory.global.write` |
| `agent.task` with foreign `cwd` | `agent.project:<id>` or `agent.cwd:*` |
| `project.files.read` (foreign project) | `project.files.read:<id>` or `…:*` |
| `project.files.write` | `project.files.write:<id>` or `…:*` |
| `scheduler.runNow` (foreign app) | `scheduler.run:<app>` or `scheduler:*` |
| `mcp.servers` raw config | `mcp.read:<id>` or `mcp.read:*` |
| `apps.create` | `apps.create` or `apps:*` |
| `apps.manage` (commit/diff/revert/server) | `apps.manage` or `apps:*` |

For runtime hosts, request via `permissions.request({ hosts, reason })` so
the user sees a single in-place approval dialog. Use
`<PermissionRequestBanner />` to surface pending requests in the UI.

## What NOT to do

- Do not import `tailwindcss` from this framework — it is a peer. Pin it in
  the utility itself.
- Do not call `window.reflexMemorySave`/`reflexMemoryList` directly in new
  code. Always go through `bridge.memory.*` or the hooks.
- Do not write a custom `postMessage` channel to the parent. The framework's
  `reflexInvoke` already handles request/response correlation.
- Do not gate features on `navigator.language` ad-hoc. The host enforces
  English prompts for dashboard widgets and agent intent strings; keep
  `prompt`/`description` fields English while UI labels can be localised.
- Do not store secrets in `storage.*` without explicit user consent — it is
  written to disk under the app folder.

## Output expectations

When this skill drives generation, the resulting utility should:

1. Have a single `manifest.json` with permissions matching the bridge calls
   actually made.
2. Build to a static `index.html` + `assets/` (no Node runtime unless the
   manifest declares `kind: "server"`).
3. Use `<ReflexProvider>` at the root.
4. Call typed `bridge.*` clients or hooks; no raw `window.reflex*` calls.
5. Use `AppShell` + `Section` for top-level chrome unless the utility is a
   single floating widget.
6. Not depend on any non-peered dependency that bloats the bundle past ~150
   KB gz unless the user asked for it.

## Updating the framework itself

The framework lives in
[`reflex-os-utility-framework`](https://github.com/legostin/reflex-os-utility-framework).
When a recurring pattern across utilities turns into copy-paste, lift it
into the framework as a brick and update this skill accordingly. Bricks
should hide bridge details; primitives should not. Do not introduce a brick
that wraps a single bridge call without UI value — just expose the hook.
