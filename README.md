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

Every generated app has a `manifest.json` and one of three runtimes:

- `static`: Reflex serves app files through `reflexapp://`.
- `server`: Reflex starts `manifest.server.command`, passes `PORT` and
  `REFLEX_PORT`, then embeds it through `reflexserver://<app-id>/` so HTML
  still receives the runtime overlay.
- `external`: Reflex embeds `manifest.external.url` as a connected service
  panel and can store an `integration` profile/MCP plan in the manifest.

The injected runtime overlay provides:

- `window.reflexInvoke(method, params)`
- `window.reflexBridgeCatalog()`
- `window.reflexSystemContext()`
- `window.reflexSystemOpenPanel(panelOrParams, projectId?, threadId?)`
- `window.reflexSystemOpenUrl(urlOrParams)`
- `window.reflexSystemOpenPath(pathOrParams)`
- `window.reflexSystemRevealPath(pathOrParams)`
- `window.reflexLog(levelOrParams, message?)`
- `window.reflexLogList(params)`
- `window.reflexManifestGet()`
- `window.reflexManifestUpdate(patch)`
- `window.reflexIntegrationCatalog(providerOrParams?)`
- `window.reflexIntegrationProfile()`
- `window.reflexIntegrationUpdate(patchOrParams, external?)`
- `window.reflexIntegrationLearnVisible(params?)`
- `window.reflexIntegrationMcpQuery(queryOrParams?)`
- `window.reflexPermissionsList()`
- `window.reflexPermissionsEnsure(permissionOrParams)`
- `window.reflexPermissionsRevoke(permissionOrParams)`
- `window.reflexNetworkHosts()`
- `window.reflexNetworkAllowHost(hostOrParams)`
- `window.reflexNetworkRevokeHost(hostOrParams)`
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
- `window.reflexProjectProfileUpdate(patch)`
- `window.reflexProjectSandboxSet(sandboxOrParams)`
- `window.reflexProjectAppsLink(appIdOrParams?)`
- `window.reflexProjectAppsUnlink(appIdOrParams?)`
- `window.reflexTopicsList(params)`
- `window.reflexTopicsOpen(threadIdOrParams, projectId?)`
- `window.reflexSkillsList(params)`
- `window.reflexProjectSkillsEnsure(skillOrParams)`
- `window.reflexProjectSkillsRevoke(skillOrParams)`
- `window.reflexMcpServers(params)`
- `window.reflexProjectMcpUpsert(nameOrParams, config?)`
- `window.reflexProjectMcpDelete(nameOrParams)`
- `window.reflexProjectFilesList(pathOrParams, recursive?)`
- `window.reflexProjectFilesRead(pathOrParams)`
- `window.reflexProjectFilesSearch(queryOrParams, includeContent?)`
- `window.reflexProjectFilesWrite(pathOrParams, content?)`
- `window.reflexProjectFilesMkdir(pathOrParams)`
- `window.reflexProjectFilesMove(fromOrParams, to?)`
- `window.reflexProjectFilesCopy(fromOrParams, to?)`
- `window.reflexProjectFilesDelete(pathOrParams, recursive?)`
- `window.reflexBrowserInit(params)`
- `window.reflexProjectBrowserSetEnabled(projectIdOrParams, enabled?)`
- `window.reflexBrowserTabs()`
- `window.reflexBrowserOpen(url)`
- `window.reflexBrowserClose(tabIdOrParams)`
- `window.reflexBrowserSetActive(tabIdOrParams)`
- `window.reflexBrowserNavigate(tabId, url)`
- `window.reflexBrowserBack(tabIdOrParams)`
- `window.reflexBrowserForward(tabIdOrParams)`
- `window.reflexBrowserReload(tabIdOrParams)`
- `window.reflexBrowserCurrentUrl(tabIdOrParams)`
- `window.reflexBrowserReadText(tabId)`
- `window.reflexBrowserReadOutline(tabId)`
- `window.reflexBrowserScreenshot(tabIdOrParams, fullPage?)`
- `window.reflexBrowserClickText(tabIdOrParams, text?, exact?)`
- `window.reflexBrowserClickSelector(tabIdOrParams, selector?)`
- `window.reflexBrowserFill(tabIdOrParams, selector?, value?)`
- `window.reflexBrowserScroll(tabIdOrParams, dx?, dy?)`
- `window.reflexBrowserWaitFor(tabIdOrParams, selector?, timeoutMs?)`
- `window.reflexSchedulerList(params)`
- `window.reflexSchedulerUpsert(scheduleOrParams)`
- `window.reflexSchedulerDelete(scheduleIdOrParams)`
- `window.reflexSchedulerRunNow(scheduleId)`
- `window.reflexSchedulerSetPaused(scheduleId, paused)`
- `window.reflexSchedulerRuns(params)`
- `window.reflexSchedulerStats(params)`
- `window.reflexSchedulerRunDetail(runIdOrParams)`
- `window.reflexMemorySave(params)`
- `window.reflexMemoryRead(relPathOrParams)`
- `window.reflexMemoryUpdate(relPathOrParams, patch?)`
- `window.reflexMemoryList(params)`
- `window.reflexMemoryDelete(relPathOrParams)`
- `window.reflexMemorySearch(queryOrParams)`
- `window.reflexMemoryRecall(queryOrParams)`
- `window.reflexMemoryStats(params)`
- `window.reflexMemoryReindex(params)`
- `window.reflexMemoryIndexPath(pathOrParams)`
- `window.reflexMemoryPathStatus(pathOrParams)`
- `window.reflexMemoryPathStatusBatch(pathsOrParams)`
- `window.reflexMemoryForgetPath(pathOrParams)`
- `window.reflexAppsList(params)`
- `window.reflexAppsCreate(descriptionOrParams, template?)`
- `window.reflexAppsExport(appIdOrParams, targetPath?)`
- `window.reflexAppsImport(zipPathOrParams)`
- `window.reflexAppsDelete(appIdOrParams)`
- `window.reflexAppsTrashList()`
- `window.reflexAppsRestore(trashIdOrParams)`
- `window.reflexAppsPurge(trashIdOrParams)`
- `window.reflexAppsStatus(appIdOrParams)`
- `window.reflexAppsDiff(appIdOrParams)`
- `window.reflexAppsCommit(appIdOrParams, message?)`
- `window.reflexAppsCommitPartial(appIdOrParams, patch?, message?)`
- `window.reflexAppsRevert(appIdOrParams)`
- `window.reflexAppsServerStatus(appIdOrParams)`
- `window.reflexAppsServerLogs(appIdOrParams)`
- `window.reflexAppsServerStart(appIdOrParams)`
- `window.reflexAppsServerStop(appIdOrParams)`
- `window.reflexAppsServerRestart(appIdOrParams)`
- `window.reflexAppsOpen(appIdOrParams)`
- `window.reflexAppsInvoke(appId, actionId, params)`
- `window.reflexAppsListActions(appIdOrParams, includeSteps?)`
- `window.reflexEventOn(topic, handler)`
- `window.reflexEventOff(topic)`
- `window.reflexEventEmit(topic, payload)`
- `window.reflexEventRecent(topicOrParams?, limit?)`
- `window.reflexEventSubscriptions()`
- `window.reflexEventClearSubscriptions()`

The Apps screen also has a `Connected app` installer for Telegram and generic
web services. It creates a local utility with `manifest.integration`, Browser
bridge permissions, and callable actions for summarizing visible web-session
text. Raw visible text is exposed only through the non-public
`read_visible_session` action or an explicit panel click. The installed utility
also has scoped `mcp.read`/`mcp.write` permission on its own app project and a
panel form that writes provider MCP server config through `project.mcp.upsert`.
It publishes a public `query_mcp_data` action so other utilities can call the
configured MCP bridge through `apps.invoke`.
The Telegram adapter also publishes `read_recent_messages`, which reads only
messages available through the configured Telegram MCP server.
The panel and public `learn_visible_interface` action can read the visible
Browser outline/text through `integration.learnVisible`, ask the agent for a
data/workflow profile, and persist it into app storage plus
`manifest.integration.data_model.learned_profile`.

## App Bridge API

Core methods:

- `bridge.catalog()` -> runtime catalog of bridge methods, overlay helpers,
  permission hints, and this app's current bridge grants.
- `system.context()` -> app id/root, manifest, app project summary, linked
  project summaries, and memory defaults.
- `system.openPanel({ panel, projectId?, threadId? })` -> ask Reflex to open
  a main UI panel: `apps`, `memory`, `automations`, `browser`, or `settings`.
  For `memory`, optional `projectId`/`threadId` selects the initial context.
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
- `integration.catalog({ provider? })` -> built-in connected-app recipes,
  including expected display, data, auth, and MCP bridge shape.
- `integration.profile()` -> this app's `integration`/`external` profile plus
  linked project/app context.
- `integration.update({ integration?, external? })` or
  `integration.update({ patch })` -> merge-update connected-app manifest
  fields.
- `integration.learnVisible({ tabId?, serviceUrl?, visibleText?, outline? })`
  -> learn a connected-app adapter profile from visible browser text/outline,
  save it in app storage, emit a connected-app event, and merge the learned
  profile into `manifest.integration.data_model`.
- `integration.mcpQuery({ query?, serviceUrl? })` -> run an English-wrapped
  agent query against configured project MCP servers, store the latest MCP
  result, emit a connected-app event, and update `manifest.integration.mcp`.
- `permissions.list()`, `permissions.ensure({ permission })` or
  `permissions.ensure({ permissions })`, `permissions.revoke(...)` -> targeted
  updates to this app's manifest permissions.
- `network.hosts()`, `network.allowHost({ host })` or
  `network.allowHost({ hosts })`, `network.revokeHost(...)` -> targeted updates
  to `manifest.network.allowed_hosts` for `net.fetch`.
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
- `project.profile.update({ projectId?, description?, agentInstructions? })`
  -> `{ ok, changed, project }`; requires `projects.write:<project>` or
  `projects.write:*`. `null` or an empty string clears a field.
- `project.sandbox.set({ projectId?, sandbox })` ->
  `{ ok, changed, sandbox, project }`; requires `projects.write:<project>` or
  `projects.write:*`. `sandbox` must be `read-only`, `workspace-write`, or
  `danger-full-access`.
- `project.apps.link({ projectId?, appId? })` ->
  `{ ok, linked, app_id, project }`; requires `projects.write:<project>` or
  `projects.write:*`. Omit `appId` to link the current app.
- `project.apps.unlink({ projectId?, appId? })` ->
  `{ ok, unlinked, app_id, project }`; same permission.
- `topics.list({ projectId?, limit?, includeAll? })`.
- `topics.open({ threadId, projectId? })` -> asks Reflex to open that topic in
  the main UI.
- `skills.list({ projectId?, includeAll? })`.
- `project.skills.ensure({ projectId?, skill })` or
  `project.skills.ensure({ projectId?, skills })` -> `{ ok, added, skills }`;
  requires `skills.write:<project>` or `skills.write:*`.
- `project.skills.revoke({ projectId?, skill })` or
  `project.skills.revoke({ projectId?, skills })` -> `{ ok, removed, skills }`;
  requires `skills.write:<project>` or `skills.write:*`.
- `mcp.servers({ projectId?, includeAll?, includeConfig? })`.
- `project.mcp.upsert({ projectId?, name, config })` or
  `project.mcp.upsert({ projectId?, serverName, config })` ->
  `{ ok, name, replaced, server, server_names }`; requires
  `mcp.write:<project>` or `mcp.write:*`.
- `project.mcp.delete({ projectId?, name })` or
  `project.mcp.delete({ projectId?, names })` ->
  `{ ok, removed, server_names }`; requires `mcp.write:<project>` or
  `mcp.write:*`.
- `project.files.list({ projectId?, path?, recursive?, includeHidden? })` ->
  `{ project_id, project_name, entries }`; linked projects are available by
  default, other projects require `project.files.read:<project>` or
  `project.files.read:*`.
- `project.files.read({ projectId?, path })` ->
  `{ project_id, project_name, path, size, content }`; reads UTF-8 text up to
  1 MiB. `.reflex` internals are always blocked.
- `project.files.search({ projectId?, query, path?, recursive?, includeHidden?,
  includeContent?, limit? })` -> `{ project_id, project_name, query, matches,
  scanned, truncated }`; searches paths by default and scans UTF-8 file content
  up to 256 KiB per file when `includeContent` is true. Requires the same read
  access as `project.files.read`.
- `project.files.write({ projectId?, path, content, createDirs?, overwrite? })`
  -> `{ ok, project_id, project_name, path, created, size }`; requires
  `project.files.write:<project>` or `project.files.write:*`.
- `project.files.mkdir({ projectId?, path, recursive? })` ->
  `{ ok, project_id, project_name, path, created }`; requires
  `project.files.write:<project>` or `project.files.write:*`.
- `project.files.move({ projectId?, from, to, createDirs?, overwrite? })` ->
  `{ ok, project_id, project_name, from, to, kind }`; requires write permission.
- `project.files.copy({ projectId?, from, to, createDirs?, overwrite?, recursive? })`
  -> `{ ok, project_id, project_name, from, to, kind }`; directory copies require
  `recursive: true` and write permission.
- `project.files.delete({ projectId?, path, recursive? })` ->
  `{ ok, project_id, project_name, path, kind }`; refuses to delete the project
  root and requires `project.files.write:<project>` or `project.files.write:*`.
- `browser.init`, `project.browser.setEnabled`,
  `browser.tabs.list`, `browser.open`, `browser.close`, `browser.setActive`,
  `browser.navigate`, `browser.back`, `browser.forward`, `browser.reload`.
- `browser.readText`, `browser.readOutline`, `browser.screenshot`.
- `browser.currentUrl`, `browser.clickText`, `browser.clickSelector`,
  `browser.fill`, `browser.scroll`, `browser.waitFor`.
- `events.emit`, `events.subscribe`, `events.unsubscribe`,
  `events.subscriptions`, `events.recent`,
  `events.clearSubscriptions`.
- `apps.list()`.
- `apps.create({ description, template?, projectId? })`; requires `apps.create`
  or `apps:*`. Passing `projectId` also requires `projects.write:<project>` or
  `projects.write:*`. Built-in templates: `blank`, `chat`, `dashboard`,
  `health-dashboard`, `form`, `api-client`, `connected-app`, `automation`, and
  `node-server`.
- `apps.export({ app_id, targetPath })` and `apps.import({ zipPath })`;
  require `apps.manage` or `apps:*`. Exports omit app storage, project metadata,
  `.git`, and dependency folders from the `.reflexapp` bundle.
- `apps.delete({ app_id })`, `apps.trashList()`,
  `apps.restore({ trash_id })`, and `apps.purge({ trash_id })`; require
  `apps.manage` or `apps:*`. Delete moves an app to trash; purge permanently
  removes a trashed app.
- `apps.status({ app_id })`; requires `apps.manage` or `apps:*` and returns
  revision, dirty state, last commit message, and entry readiness.
- `apps.diff({ app_id })` -> `{ app_id, diff }`;
  `apps.commit({ app_id, message? })`, `apps.commitPartial({ app_id, patch, message? })`,
  and `apps.revert({ app_id })` require `apps.manage` or `apps:*` and manage app
  code revisions.
- `apps.server.status({ app_id })`, `apps.server.logs({ app_id })`,
  `apps.server.start({ app_id })`, `apps.server.stop({ app_id })`, and
  `apps.server.restart({ app_id })`; require `apps.manage` or `apps:*` and
  control server-runtime apps.
- `apps.open({ app_id })` -> asks Reflex to open that app in the main UI.
- `apps.invoke({ app_id, action_id, params })`.
- `apps.list_actions({ app_id?, include_steps? })`.

Project/topic methods return sanitized summaries for linked projects by
default. Cross-project overview requires `projects.read:*`,
`topics.read:<project>`, or `topics.read:*` in `manifest.permissions`. Project
profile, sandbox, and linked-app updates require `projects.write:<project>` or
`projects.write:*`.
Skills and MCP server names are available for linked projects; cross-project
skills require `skills.read:<project>` or `skills.read:*`, skill mutations
require `skills.write:<project>` or `skills.write:*`, and raw MCP config requires
`mcp.read:<project>` or `mcp.read:*`. MCP server mutations require
`mcp.write:<project>` or `mcp.write:*`.
Browser methods require `browser.read` for read-only inspection or
`browser.control` for init, open, close, setActive, navigate, back, forward,
reload, click, fill, and scroll. Project browser state requires a linked project
or `browser.project:<project>`. Enabling the Reflex Browser MCP server for a
project uses `project.browser.setEnabled` and requires `mcp.write:<project>` or
`mcp.write:*`.

Scheduler methods:

- `scheduler.list({ appId?, includeAll? })`.
- `scheduler.upsert({ id, name?, cron, enabled?, catch_up?, steps })` or
  `scheduler.upsert({ schedule })` -> create/update this app's own schedule.
- `scheduler.delete({ scheduleId })` -> delete this app's own schedule.
- `scheduler.runNow({ scheduleId })`; accepts a local schedule id or
  `<app_id>::<schedule_id>`.
- `scheduler.setPaused({ scheduleId, paused })`.
- `scheduler.runs({ limit?, beforeTs?, appId?, includeAll? })`.
- `scheduler.stats({ appId?, includeAll?, recentLimit? })` -> schedule counts,
  next fire timestamp, recent run counts, and last error summary.
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
- `memory.stats({ projectId? })` -> RAG document/chunk counts, kind counts,
  last indexed timestamp, and stale/missing source counts.
- `memory.reindex({ projectId? })` -> rebuild supported project-file RAG entries.
- `memory.indexPath({ path, projectId? })`.
- `memory.pathStatus({ path, projectId? })`.
- `memory.pathStatusBatch({ paths, projectId? })`.
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
`clipboard.*`, `system.openPanel`, `system.openUrl`, `system.openPath`,
`system.revealPath`, `apps.create`, `apps.import`, `apps.commit`,
`apps.commitPartial`, `apps.delete`, `apps.restore`, `apps.revert`,
`apps.purge`, and `apps.open` are not valid inside schedules. `projects.open`,
`topics.open`, `scheduler.runNow`,
`scheduler.setPaused`, `scheduler.upsert`, and
`scheduler.delete` are also blocked inside schedule steps to prevent unattended
recursive runs. Non-UI methods such as `project.files.*` can run in schedules
when the app has the required manifest permissions.

## Development

```sh
npm run build
cd src-tauri && cargo check
```
