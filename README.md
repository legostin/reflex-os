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
  server runtimes. Apps communicate with Reflex through `window.postMessage`
  and can be created from a Project so widgets/actions link back automatically.
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
- `window.reflexBridgeCatalog()`
- `window.reflexSystemContext()`
- `window.reflexSystemOpenUrl(urlOrParams)`
- `window.reflexSystemOpenPath(pathOrParams)`
- `window.reflexSystemRevealPath(pathOrParams)`
- `window.reflexLog(levelOrParams, message?)`
- `window.reflexLogList(params)`
- `window.reflexManifestGet()`
- `window.reflexManifestUpdate(patch)`
- `window.reflexWidgetsList()`
- `window.reflexWidgetsUpsert(widgetOrParams)`
- `window.reflexWidgetsDelete(widgetIdOrParams, deleteEntry?)`
- `window.reflexActionsList()`
- `window.reflexActionsUpsert(actionOrParams)`
- `window.reflexActionsDelete(actionIdOrParams)`
- `window.reflexCapabilities()`
- `window.reflexAgentAsk(promptOrParams)`
- `window.reflexAgentStartTopic(promptOrParams, projectId?)`
- `window.reflexAgentTask(promptOrParams)`
- `window.reflexAgentStream(promptOrParams)`
- `window.reflexAgentStreamAbort(threadIdOrParams)`
- `window.reflexStorageGet(keyOrParams)`
- `window.reflexStorageSet(keyOrParams, value?)`
- `window.reflexStorageList(params)`
- `window.reflexStorageDelete(keyOrParams)`
- `window.reflexFsRead(pathOrParams)`
- `window.reflexFsList(pathOrParams, recursive?)`
- `window.reflexFsWrite(pathOrParams, content?)`
- `window.reflexFsDelete(pathOrParams, recursive?)`
- `window.reflexClipboardReadText()`
- `window.reflexClipboardWriteText(textOrParams)`
- `window.reflexNetFetch(urlOrParams, options?)`
- `window.reflexDialogOpenDirectory(params)`
- `window.reflexDialogOpenFile(params)`
- `window.reflexDialogSaveFile(params)`
- `window.reflexNotifyShow(titleOrParams, body?)`
- `window.reflexProjectsList(params)`
- `window.reflexProjectsOpen(projectIdOrParams)`
- `window.reflexTopicsList(params)`
- `window.reflexTopicsOpen(threadIdOrParams, projectId?)`
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
- `window.reflexSchedulerUpsert(scheduleOrParams)`
- `window.reflexSchedulerDelete(scheduleIdOrParams)`
- `window.reflexSchedulerRunNow(scheduleId)`
- `window.reflexSchedulerSetPaused(scheduleId, paused)`
- `window.reflexSchedulerRuns(params)`
- `window.reflexSchedulerRunDetail(runIdOrParams)`
- `window.reflexMemorySave(params)`
- `window.reflexMemoryRead(relPathOrParams)`
- `window.reflexMemoryUpdate(relPathOrParams, patch?)`
- `window.reflexMemoryList(params)`
- `window.reflexMemoryDelete(relPathOrParams)`
- `window.reflexMemorySearch(queryOrParams)`
- `window.reflexMemoryRecall(queryOrParams)`
- `window.reflexMemoryIndexPath(pathOrParams)`
- `window.reflexMemoryPathStatus(pathOrParams)`
- `window.reflexMemoryForgetPath(pathOrParams)`
- `window.reflexAppsList(params)`
- `window.reflexAppsOpen(appIdOrParams)`
- `window.reflexAppsInvoke(appId, actionId, params)`
- `window.reflexAppsListActions(appIdOrParams, includeSteps?)`
- `window.reflexEventOn(topic, handler)`
- `window.reflexEventOff(topic)`
- `window.reflexEventEmit(topic, payload)`

## App Bridge API

Core methods:

- `bridge.catalog()` -> runtime catalog of bridge methods, overlay helpers,
  permission hints, and this app's current bridge grants.
- `system.context()` -> app id/root, manifest, app project summary, linked
  project summaries, and memory defaults.
- `system.openUrl({ url })` -> open an `http`, `https`, `mailto`, or `tel`
  URL in the system default app.
- `system.openPath({ path })` -> open an existing local file/folder; relative
  paths resolve from the app folder.
- `system.revealPath({ path })` -> reveal an existing local file/folder in
  Finder; relative paths resolve from the app folder.
- `logs.write({ level?, source?, message })` -> write an app-scoped diagnostic
  event into Settings -> Logs.
- `logs.list({ limit?, sinceSeq?, source?, level? })` -> read this app's own
  diagnostic events from the in-memory log ring.
- `manifest.get()` -> current `manifest.json`.
- `manifest.update({ patch })` -> merge-update this app's manifest; useful for
  adding `actions`, `widgets`, `schedules`, permissions, or network hosts.
- `widgets.list()` -> this app's dashboard widgets.
- `widgets.upsert({ id, name?, entry?, size?, description?, html? })` or
  `widgets.upsert({ widget, html? })` -> create/update a dashboard widget and
  optionally write its HTML entry file.
- `widgets.delete({ widgetId, deleteEntry? })` -> remove a dashboard widget and
  optionally delete its entry file.
- `actions.list()` -> this app's manifest actions.
- `actions.upsert({ id, name?, description?, public?, params_schema?, steps })`
  or `actions.upsert({ action })` -> create/update a callable workflow.
- `actions.delete({ actionId })` -> remove this app's callable workflow.
- `agent.ask({ prompt })` -> one-shot agent answer.
- `agent.startTopic({ prompt, projectId? })` -> full Reflex topic.
- `agent.task({ prompt, sandbox?, cwd?, memoryThreadId?, includeContext? })` -> isolated sub-agent result.
- `agent.stream({ prompt, sandbox?, cwd?, memoryThreadId?, includeContext? })` -> streamed agent response.
- `agent.streamAbort({ threadId })` -> abort a streamed app-agent turn.
  `cwd` may be the app root or a linked project; another project requires
  `agent.project:<project>` or `agent.project:*`, and arbitrary cwd requires
  `agent.cwd:*`. Project cwd automatically receives that project's MCP config,
  preferred skills, project profile, and memory/RAG context; set
  `includeContext: false` only for raw prompts. `memoryThreadId` selects topic memory.
- `storage.get({ key })`, `storage.set({ key, value })`.
- `storage.list({ prefix? })` -> `{ keys, entries }`.
- `storage.delete({ key })` or `storage.delete({ keys })` -> `{ ok, deleted, missing }`.
- `fs.read({ path })`, `fs.write({ path, content })` inside the app folder.
- `fs.list({ path?, recursive?, includeHidden? })` -> `{ entries }`.
- `fs.delete({ path, recursive? })` -> `{ ok, path, kind }`; deleting the app
  root is blocked.
- `clipboard.readText()` -> `{ text }`; requires `clipboard.read` or
  `clipboard:*` in `manifest.permissions`.
- `clipboard.writeText({ text })` -> `{ ok }`; requires `clipboard.write` or
  `clipboard:*` in `manifest.permissions`.
- `dialog.openDirectory`, `dialog.openFile`, `dialog.saveFile`.
- `notify.show({ title, body })`.
- `net.fetch({ url, method?, headers?, body?, timeoutMs? })`; requires
  `manifest.network.allowed_hosts`.
- `projects.list({ includeAll? })`.
- `projects.open({ projectId })` -> asks Reflex to open that project in the
  main UI.
- `topics.list({ projectId?, limit?, includeAll? })`.
- `topics.open({ threadId, projectId? })` -> asks Reflex to open that topic in
  the main UI.
- `skills.list({ projectId?, includeAll? })`.
- `mcp.servers({ projectId?, includeAll?, includeConfig? })`.
- `browser.init`, `browser.tabs.list`, `browser.open`, `browser.navigate`.
- `browser.readText`, `browser.readOutline`, `browser.screenshot`.
- `browser.clickText`, `browser.clickSelector`, `browser.fill`.
- `events.emit`, `events.subscribe`, `events.unsubscribe`.
- `apps.list()`.
- `apps.open({ app_id })` -> asks Reflex to open that app in the main UI.
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
- `scheduler.upsert({ id, name?, cron, enabled?, catch_up?, steps })` or
  `scheduler.upsert({ schedule })` -> create/update this app's own schedule.
- `scheduler.delete({ scheduleId })` -> delete this app's own schedule.
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
- `memory.read({ scope?, relPath, projectId?, threadId? })`.
- `memory.update({ scope?, relPath, name?, description?, body?, tags?, kind?, projectId?, threadId? })`.
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
  Actions may include optional `params_schema` JSON Schema metadata; caller
  input is available to workflow steps as `{{input.<field>}}`.
  Apps can publish their own callable API at runtime with `actions.upsert`.
- `widgets`: compact pages shown on a linked project's dashboard.
  Apps can manage their own dashboard widgets at runtime with `widgets.upsert`
  and `widgets.delete`.

Workflow steps call normal bridge methods and can pass previous results through
`{{steps.<name>.<field>}}` templates. UI-only methods like `dialog.*`,
`clipboard.*`, `system.openUrl`, `system.openPath`, `system.revealPath`, and
`apps.open` are not valid inside schedules. `projects.open`, `topics.open`,
`scheduler.runNow`, `scheduler.setPaused`, `scheduler.upsert`, and
`scheduler.delete` are also blocked inside schedule steps to prevent unattended
recursive runs.

## Development

```sh
npm run build
cd src-tauri && cargo check
```
