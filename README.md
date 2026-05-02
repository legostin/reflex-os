# Reflex OS

Reflex is a local macOS agent layer built with Tauri, React, and Codex CLI.
It combines project-scoped chat threads, generated utilities, a browser/MCP
bridge, long-term memory, file indexing, widgets, and app-driven automations.

## Core Surfaces

- **Projects**: registered folders with sandbox settings, MCP config, agent
  profile instructions, preferred skills, topics, linked utilities, widgets,
  files, and RAG indexing state.
- **Topics**: Codex-backed agent threads persisted under `.reflex/topics`.
- **Browser**: project-scoped Playwright sidecar with an MCP bridge and a
  "start chat from tabs" flow.
- **Apps**: generated Reflex utilities served either as static HTML or local
  server runtimes. Apps communicate with Reflex through `window.postMessage`.
- **Memory**: global, project, and topic notes plus vector search over notes,
  files, and selected images.
- **Automations**: manifest-defined schedules and actions executed through the
  same bridge methods that apps use.

## Project Agent Profile

Each project can define:

- `description`: human context for the project.
- `agent_instructions`: project-specific operating rules injected into every
  new, continued, and auto-resumed topic.
- `skills`: preferred Codex skills or workflow names to consider before work.
- `mcp_servers`: project-scoped MCP server config passed to Codex sessions.

The profile is stored in `.reflex/project.json` and is surfaced in the Project
settings screen. Reflex injects it into the final prompt together with memory
recall, so the agent sees the project's operating rules, available MCP servers,
preferred skills, and long-term memory in one context block.

## Generated App Runtime

Every generated app has a `manifest.json` and one of two runtimes:

- `static`: Reflex serves app files through `reflexapp://`.
- `server`: Reflex starts `manifest.server.command`, passes `PORT` and
  `REFLEX_PORT`, then embeds it through `reflexserver://<app-id>/` so HTML
  still receives the runtime overlay.

The injected runtime overlay provides:

- `window.reflexInvoke(method, params)`
- `window.reflexSystemContext()`
- `window.reflexManifestGet()`
- `window.reflexManifestUpdate(patch)`
- `window.reflexCapabilities()`
- `window.reflexAgentAsk(promptOrParams)`
- `window.reflexAgentStartTopic(promptOrParams, projectId?)`
- `window.reflexAgentTask(promptOrParams)`
- `window.reflexAgentStream(promptOrParams)`
- `window.reflexAgentStreamAbort(threadIdOrParams)`
- `window.reflexStorageGet(keyOrParams)`
- `window.reflexStorageSet(keyOrParams, value?)`
- `window.reflexFsRead(pathOrParams)`
- `window.reflexFsWrite(pathOrParams, content?)`
- `window.reflexNetFetch(urlOrParams, options?)`
- `window.reflexDialogOpenDirectory(params)`
- `window.reflexDialogOpenFile(params)`
- `window.reflexDialogSaveFile(params)`
- `window.reflexNotifyShow(titleOrParams, body?)`
- `window.reflexProjectsList(params)`
- `window.reflexTopicsList(params)`
- `window.reflexSkillsList(params)`
- `window.reflexMcpServers(params)`
- `window.reflexBrowserInit(params)`
- `window.reflexBrowserTabs()`
- `window.reflexBrowserOpen(url)`
- `window.reflexBrowserNavigate(tabId, url)`
- `window.reflexBrowserReadText(tabId)`
- `window.reflexBrowserReadOutline(tabId)`
- `window.reflexBrowserScreenshot(tabIdOrParams, fullPage?)`
- `window.reflexBrowserClickText(tabIdOrParams, text?, exact?)`
- `window.reflexBrowserClickSelector(tabIdOrParams, selector?)`
- `window.reflexBrowserFill(tabIdOrParams, selector?, value?)`
- `window.reflexSchedulerList(params)`
- `window.reflexSchedulerRunNow(scheduleId)`
- `window.reflexSchedulerSetPaused(scheduleId, paused)`
- `window.reflexSchedulerRuns(params)`
- `window.reflexMemorySave(params)`
- `window.reflexMemoryList(params)`
- `window.reflexMemoryDelete(relPathOrParams)`
- `window.reflexMemorySearch(queryOrParams)`
- `window.reflexMemoryRecall(queryOrParams)`
- `window.reflexMemoryIndexPath(pathOrParams)`
- `window.reflexMemoryPathStatus(pathOrParams)`
- `window.reflexMemoryForgetPath(pathOrParams)`
- `window.reflexAppsInvoke(appId, actionId, params)`
- `window.reflexAppsListActions(appIdOrParams, includeSteps?)`
- `window.reflexEventOn(topic, handler)`
- `window.reflexEventOff(topic)`
- `window.reflexEventEmit(topic, payload)`

## App Bridge API

Core methods:

- `system.context()` -> app id/root, manifest, app project summary, linked
  project summaries, and memory defaults.
- `manifest.get()` -> current `manifest.json`.
- `manifest.update({ patch })` -> merge-update this app's manifest; useful for
  adding `actions`, `widgets`, `schedules`, permissions, or network hosts.
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
- `projects.list({ includeAll? })`.
- `topics.list({ projectId?, limit?, includeAll? })`.
- `skills.list({ projectId?, includeAll? })`.
- `mcp.servers({ projectId?, includeAll?, includeConfig? })`.
- `browser.init`, `browser.tabs.list`, `browser.open`, `browser.navigate`.
- `browser.readText`, `browser.readOutline`, `browser.screenshot`.
- `browser.clickText`, `browser.clickSelector`, `browser.fill`.
- `events.emit`, `events.subscribe`, `events.unsubscribe`.
- `apps.invoke({ app_id, action_id, params })`.
- `apps.list_actions({ app_id?, include_steps? })`.

Project/topic methods return sanitized summaries for linked projects by
default. Cross-project overview requires `projects.read:*`,
`topics.read:<project>`, or `topics.read:*` in `manifest.permissions`.
Skills and MCP server names are available for linked projects; cross-project
skills require `skills.read:<project>` or `skills.read:*`, and raw MCP config
requires `mcp.read:<project>` or `mcp.read:*`.
Browser methods require `browser.read` for read-only inspection or
`browser.control` for init/open/navigate/click/fill. Project browser state
requires a linked project or `browser.project:<project>`.

Scheduler methods:

- `scheduler.list({ appId?, includeAll? })`.
- `scheduler.runNow({ scheduleId })`; accepts a local schedule id or
  `<app_id>::<schedule_id>`.
- `scheduler.setPaused({ scheduleId, paused })`.
- `scheduler.runs({ limit?, beforeTs?, appId?, includeAll? })`.
- `scheduler.runDetail({ runId })`.

Apps can inspect and control their own schedules without extra permissions.
Cross-app scheduler access requires `scheduler.read:*`, `scheduler.run:<app>`,
`scheduler.write:<app>::<schedule>`, or `scheduler:*`.

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
valid inside schedules. `scheduler.runNow` and `scheduler.setPaused` are also
blocked inside schedule steps to prevent unattended recursive runs.

## Development

```sh
npm run build
cd src-tauri && cargo check
```
