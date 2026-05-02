# Reflex OS

Reflex is a local macOS agent layer built with Tauri, React, and Codex CLI.
It combines project-scoped chat threads, generated utilities, a browser/MCP
bridge, long-term memory, file indexing, widgets, and app-driven automations.

## Core Surfaces

- **Projects**: registered folders with sandbox settings, MCP config, topics,
  linked utilities, widgets, files, and RAG indexing state.
- **Topics**: Codex-backed agent threads persisted under `.reflex/topics`.
- **Browser**: project-scoped Playwright sidecar with an MCP bridge and a
  "start chat from tabs" flow.
- **Apps**: generated Reflex utilities served either as static HTML or local
  server runtimes. Apps communicate with Reflex through `window.postMessage`.
- **Memory**: global, project, and topic notes plus vector search over notes,
  files, and selected images.
- **Automations**: manifest-defined schedules and actions executed through the
  same bridge methods that apps use.

## Generated App Runtime

Every generated app has a `manifest.json` and one of two runtimes:

- `static`: Reflex serves app files through `reflexapp://`.
- `server`: Reflex starts `manifest.server.command`, passes `PORT` and
  `REFLEX_PORT`, and embeds `http://localhost:PORT/`.

The injected runtime overlay provides:

- `window.reflexInvoke(method, params)`
- `window.reflexSystemContext()`
- `window.reflexMemorySave(params)`
- `window.reflexMemoryList(params)`
- `window.reflexMemoryRecall(queryOrParams)`
- `window.reflexAppsInvoke(appId, actionId, params)`
- `window.reflexEventOn(topic, handler)`
- `window.reflexEventOff(topic)`
- `window.reflexEventEmit(topic, payload)`

## App Bridge API

Core methods:

- `system.context()` -> app id/root, manifest, app project, linked projects,
  and memory defaults.
- `agent.ask({ prompt })` -> one-shot agent answer.
- `agent.startTopic({ prompt, projectId? })` -> full Reflex topic.
- `agent.task({ prompt, sandbox?, cwd? })` -> isolated sub-agent result.
- `agent.stream({ prompt, sandbox?, cwd? })` -> streamed agent response.
- `storage.get({ key })`, `storage.set({ key, value })`.
- `fs.read({ path })`, `fs.write({ path, content })` inside the app folder.
- `dialog.openDirectory`, `dialog.openFile`, `dialog.saveFile`.
- `notify.show({ title, body })`.
- `net.fetch({ url, method?, headers?, body?, timeoutMs? })`; requires
  `manifest.network.allowed_hosts`.
- `events.emit`, `events.subscribe`, `events.unsubscribe`.
- `apps.invoke({ app_id, action_id, params })`.
- `apps.list_actions({ app_id?, include_steps? })`.

Memory methods:

- `memory.save({ scope?, kind?, name, description?, body, tags?, projectId?, threadId? })`.
- `memory.list({ scope?, filter?, projectId?, threadId? })`.
- `memory.delete({ scope?, relPath, projectId?, threadId? })`.
- `memory.search({ query, projectId?, limit? })`.
- `memory.recall({ query, projectId?, threadId?, maxNotes?, maxRag? })`.
- `memory.indexPath({ path, projectId? })`.
- `memory.pathStatus({ path, projectId? })`.
- `memory.forgetPath({ path, projectId? })`.

`scope` defaults to `project`. If an app is linked to exactly one project,
project memory targets that project; otherwise it targets the app's own project.
For a specific project, call `system.context()` and pass a `projectId` from
`linked_projects`. Global memory requires `memory.global.read` or
`memory.global.write` in `manifest.permissions`.

## Manifest Automation

Apps can expose:

- `schedules`: cron-like workflows run by Reflex while it is alive.
- `actions`: callable workflows for other apps via `apps.invoke`.
- `widgets`: compact pages shown on a linked project's dashboard.

Workflow steps call normal bridge methods and can pass previous results through
`{{steps.<name>.<field>}}` templates. UI-only methods like `dialog.*` are not
valid inside schedules.

## Development

```sh
npm run build
cd src-tauri && cargo check
```
