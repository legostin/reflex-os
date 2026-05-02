import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";

export type LanguageSetting = "auto" | "en" | "ru";
export type Locale = "en" | "ru";

type Dictionary = Record<string, string>;
export type Translate = (
  key: string,
  values?: Record<string, string | number>,
) => string;

type I18nContextValue = {
  language: LanguageSetting;
  locale: Locale;
  setLanguage: (language: LanguageSetting) => void;
  t: Translate;
};

const STORAGE_KEY = "reflex-ui-language";

const dictionaries: Record<Locale, Dictionary> = {
  en: {
    "language.auto": "Auto",
    "language.en": "English",
    "language.ru": "Russian",
    "nav.home": "Home",
    "nav.apps": "Utilities",
    "nav.memory": "Memory",
    "nav.memoryWithName": "Memory · {name}",
    "nav.automations": "Automations",
    "nav.browser": "Browser",
    "nav.settings": "Settings",
    "nav.newProject": "+ Project",
    "nav.newProjectTitle": "New project",
    "nav.newPane": "+ Pane",
    "nav.newPaneTitle": "Add pane",
    "nav.activeProject": "Active project",
    "nav.noProjects": "No projects",
    "nav.chooseProject": "Choose project",
    "nav.closeTab": "Close tab",
    "nav.closePane": "Close pane",
    "header.threadLabel": "threads",
    "header.projectLabel": "projects",
    "settings.title": "Settings",
    "settings.capabilities": "Capabilities",
    "settings.logs": "Logs and events",
    "settings.languageLabel": "Interface language",
    "settings.layerTitle": "Reflex OS Layer",
    "settings.layerBody":
      "Reflex is a local macOS layer over Codex CLI: projects, topics, browser/MCP bridge, generated utilities, widgets, memory, RAG, and scheduled automations live in one workspace.",
    "settings.summaryLabel": "Reflex OS summary",
    "settings.systemMap": "System Map",
    "settings.bridgeTitle": "Bridge for Generated Utilities",
    "settings.bridgeSearch": "Search API, helpers, permissions...",
    "settings.methodsCount": "{visible}/{total} methods",
    "settings.noMatches": "No matches.",
    "settings.recipesTitle": "Bridge Workflows",
    "settings.recipesCount": "{visible}/{total} workflows",
    "settings.helpersTitle": "Runtime Helpers",
    "settings.helpersCount": "{visible}/{total} helpers",
    "settings.helpersHint":
      "Generated utilities should use these helpers instead of manual postMessage; permissions and manifest.network rules still apply to the underlying bridge method.",
    "settings.permissionsTitle": "Permissions",
    "settings.grantsCount": "{visible}/{total} manifest grants",
    "settings.automationFlow": "Automation Flow",
    "settings.flowSchedules": "manifest.schedules",
    "settings.flowRunner": "scheduler runner",
    "settings.flowBridge": "bridge steps",
    "settings.flowHistory": "run history",
    "settings.automationHint":
      "Generated utilities can update their own manifest, add schedules/actions, inspect runs, and expose widgets or public actions to other apps.",
    "settings.copy": "Copy",
    "settings.copied": "Copied",
    "settings.allSources": "all sources",
    "settings.logSearch": "Search text...",
    "settings.resume": "Resume",
    "settings.pause": "Pause",
    "settings.clear": "Clear",
    "settings.clearTitle": "Clear view; backend buffer is untouched",
    "settings.rows": "{count} rows",
    "settings.noLogs": "No logs.",
    "appViewer.manifestCapabilities": "Manifest capabilities",
    "appViewer.manifestPermissions": "Manifest permissions",
    "appViewer.permissions": "Permissions",
    "appViewer.networkHosts": "Network hosts",
    "appViewer.bridgeCatalog": "Runtime bridge catalog",
    "appViewer.bridgeSearch": "Search methods, helpers, workflows...",
    "appViewer.methodsCount": "{visible}/{total} methods",
    "appViewer.helpersCount": "{visible}/{total} helpers",
    "appViewer.noBridgeMatches": "No bridge matches.",
    "appViewer.copy": "Copy",
    "appViewer.methods": "Methods",
    "appViewer.helpers": "Helpers",
    "appViewer.exportDialogTitle": "Save .reflexapp",
    "appViewer.revertConfirm":
      "Revert to the previous version? Unsaved changes will be lost.",
    "appViewer.restartServer": "Restart",
    "appViewer.restartServerTitle": "Restart server",
    "appViewer.serverLogsTitle": "Show server logs",
    "appViewer.logs": "Logs",
    "appViewer.runtimeHelpersTitle": "Show runtime overlay helpers",
    "appViewer.inspectorTitle":
      "Click an element in the utility and describe what to change",
    "appViewer.inspector": "Inspector",
    "appViewer.editExistingThreadTitle":
      "Open an existing thread for revisions and attach it to this utility",
    "appViewer.edit": "Edit",
    "appViewer.newThreadTitle":
      "Create a new thread for isolated changes",
    "appViewer.newThread": "New thread",
    "appViewer.exportTitle": "Export utility to a .reflexapp file",
    "appViewer.export": "Export",
    "appViewer.actions": "Actions",
    "appViewer.public": "public",
    "appViewer.params": "params",
    "appViewer.actionParamsEditor": "Action parameter editor",
    "appViewer.run": "Run",
    "appViewer.unsavedChanges": "There are unsaved changes.",
    "appViewer.diffTitle": "View diff and apply selectively",
    "appViewer.commit": "Commit",
    "appViewer.save": "Save",
    "appViewer.revert": "Revert",
    "appViewer.reload": "Reload",
    "appViewer.saveRevision": "Save revision",
    "appViewer.appCrashed": "App crashed:",
    "appViewer.errorFixTitle": "Send the error to Codex and ask it to fix it",
    "appViewer.dismiss": "Dismiss",
    "appViewer.close": "Close",
    "appViewer.selected": "selected",
    "appViewer.inspectorPlaceholder":
      "What should change in this element? (Cmd+Enter)",
    "appViewer.applying": "Applying...",
    "appViewer.startingServer": "Starting the local utility server...",
    "appViewer.serverStartFailed": "Server did not start: {error}",
    "appViewer.serverCrashed": "Server crashed: {error}",
    "appViewer.generationIncomplete": "Generation is not complete",
    "appViewer.generationIncompleteBefore": "Codex has not written",
    "appViewer.generationIncompleteAfter":
      "yet. The process may have been interrupted or may be waiting for plan approval.",
    "appViewer.checkAgain": "Check again",
    "appViewer.serverLogs": "server logs",
    "appViewer.clearLogsTitle": "Clear local log buffer",
    "appViewer.clear": "clear",
    "appViewer.empty": "empty",
    "appViewer.selectThread": "Choose a thread on the left.",
    "apps.newUtility": "+ New utility",
    "apps.deletedAppsTitle": "Deleted apps",
    "apps.trash": "Trash",
    "apps.headerHint":
      "Utilities talk to the agent through the Reflex bridge. Describe what you need and Codex will build it.",
    "apps.trashTitle": "Trash",
    "apps.trashEmpty": "Empty.",
    "apps.deletedAt": "deleted {age}",
    "apps.restore": "Restore",
    "apps.deleteForever": "Delete forever",
    "apps.empty": "No utilities yet.",
    "apps.open": "Open",
    "apps.writingFiles": "Codex is still writing files; click to view",
    "apps.moveToTrash": "Move to trash",
    "apps.creatingBadge": "creating...",
    "apps.newUtilityTitle": "New Reflex utility",
    "apps.chooseTemplate": "Choose a template for the task.",
    "apps.linkedToProject": "Will be linked to {name}",
    "apps.cancel": "Cancel",
    "apps.importTitle": "Import .reflexapp",
    "apps.importBundleTitle": "Import .reflexapp bundle",
    "apps.importBundle": "Import .reflexapp",
    "apps.next": "Next ->",
    "apps.back": "<- Back",
    "apps.describeHint":
      "Describe what the utility should do. Codex will write the files in the background.",
    "apps.createShortcut": "Create (⌘↵)",
    "apps.creating": "Creating...",
    "apps.deleteConfirm": "Move \"{name}\" to trash?",
    "apps.purgeConfirm":
      "Delete \"{name}\" permanently? This action cannot be undone.",
    "apps.justNow": "just now",
    "apps.minutesAgo": "{count} min ago",
    "apps.hoursAgo": "{count} h ago",
    "apps.daysAgo": "{count} d ago",
    "template.blank.name": "Blank",
    "template.blank.description": "Empty utility; Codex decides the structure",
    "template.blank.placeholder":
      "Example: a counter with a save-to-storage button; a weather widget; ...",
    "template.chat.name": "Chat utility",
    "template.chat.description": "Agent chat with streamed responses",
    "template.chat.placeholder":
      "Example: an assistant for my calendar; a translation helper; ...",
    "template.dashboard.name": "Dashboard",
    "template.dashboard.description":
      "Data through agent.task as tables or cards",
    "template.dashboard.placeholder":
      "Example: status of all projects from ~/projects; latest commits list; ...",
    "template.healthDashboard.name": "Health dashboard",
    "template.healthDashboard.description":
      "Operational overview of scheduler, memory/RAG, and linked apps",
    "template.healthDashboard.placeholder":
      "Example: monitor project automations, memory index, and server apps with a compact widget; ...",
    "template.form.name": "Form",
    "template.form.description": "Fields -> Run -> result through agent.task",
    "template.form.placeholder":
      "Example: rewrite text in a specific style; generate a regex from a description; ...",
    "template.apiClient.name": "API client",
    "template.apiClient.description": "External API requests through net.fetch",
    "template.apiClient.placeholder":
      "Example: show issues from github.com/owner/repo; currency converter through open.er-api.com; ...",
    "template.automation.name": "Automation",
    "template.automation.description":
      "Schedule, action, and widget for a background task",
    "template.automation.placeholder":
      "Example: check important emails hourly and save a brief summary; collect project status every morning; ...",
    "template.nodeServer.name": "Node server",
    "template.nodeServer.description":
      "runtime=server: custom backend on Node.js stdlib",
    "template.nodeServer.placeholder":
      "Example: WebSocket chat room; sqlite viewer; markdown preview; ...",
    "stats.bridgeMethods": "Bridge methods",
    "stats.overlayHelpers": "Overlay helpers",
    "stats.workflows": "Workflows",
    "stats.permissionForms": "Permission forms",
    "stats.dispatchApi": "dispatch API",
    "stats.windowReflex": "window.reflex*",
    "stats.bridgeWorkflows": "working patterns",
    "stats.manifestGrants": "manifest grants",
    "cap.projects.title": "Projects",
    "cap.projects.body":
      "Folders with sandbox, browser MCP, MCP servers, agent profile, preferred skills, linked utilities, widgets, and indexed files.",
    "cap.topics.title": "Topics",
    "cap.topics.body":
      "Codex threads with project profile, memory recall, and resumable work sessions.",
    "cap.apps.title": "Generated Utilities",
    "cap.apps.body":
      "Static or local server apps with manifest, storage, actions, widgets, and Reflex bridge APIs.",
    "cap.memory.title": "Memory",
    "cap.memory.body":
      "Global, project, and topic notes, plus RAG over indexed files and saved facts.",
    "cap.automations.title": "Automations",
    "cap.automations.body":
      "Manifest schedules and actions executed through the same bridge methods available to utilities.",
    "cap.mcp.title": "MCP and Skills",
    "cap.mcp.body":
      "Project-scoped MCP JSON and preferred skills are injected into new, continued, and auto-resumed topics.",
    "bridge.systemManifest": "System and Manifest",
    "bridge.agentRuntime": "Agent Runtime",
    "bridge.appDataFiles": "App Data and Files",
    "bridge.projectsTopics": "Projects and Topics",
    "bridge.browserSidecar": "Browser Sidecar",
    "bridge.nativeMacos": "Native macOS",
    "bridge.network": "Network",
    "bridge.memory": "Memory",
    "bridge.automations": "Automations",
    "bridge.appGrid": "App Grid",
    "bridge.base": "Base",
    "bridge.agent": "Agent",
    "bridge.storageIo": "Storage / IO",
    "bridge.projectsBrowser": "Projects / Browser",
    "bridge.memoryAutomationApps": "Memory / Automations / Utilities",
    "recipe.contextAgent.title": "Contextual sub-agent",
    "recipe.contextAgent.body":
      "Project cwd attaches sandbox, MCP, preferred skills, project profile, and memory/RAG.",
    "recipe.longMemory.title": "Long-term memory",
    "recipe.longMemory.body":
      "Save new facts and update known relPath entries without duplicates.",
    "recipe.capabilities.title": "Capabilities",
    "recipe.capabilities.body":
      "Add permissions and network hosts precisely, without manual manifest merging.",
    "recipe.utilityService.title": "Utility as a service",
    "recipe.utilityService.body":
      "Publish actions/widgets, create utilities, export bundles, and manage server runtime.",
    "recipe.automation.title": "Automation",
    "recipe.automation.body":
      "Schedule steps use the same bridge, except UI-only methods.",
    "recipe.healthDashboard.title": "Health dashboard",
    "recipe.healthDashboard.body":
      "Show automation status, RAG index health, and the latest error; add scheduler.read:* for a global overview.",
    "recipe.projectFiles.title": "Project files",
    "recipe.projectFiles.body":
      "Search, edit, and reindex linked project files through the bridge without leaving the utility sandbox.",
    "recipe.appRevisions.title": "Utility revisions",
    "recipe.appRevisions.body":
      "Show a generated app diff, save meaningful revisions, and revert failed edits.",
    "recipe.eventGrid.title": "Event grid",
    "recipe.eventGrid.body":
      "Connect utilities through topics, recent history, and subscriptions without direct coupling.",
    "recipe.browserSidecar.title": "Browser sidecar",
    "recipe.browserSidecar.body":
      "Enable project Browser MCP, open pages, read outlines, and fill forms.",
    "recipe.projectMcpSkills.title": "Project MCP and skills",
    "recipe.projectMcpSkills.body":
      "Update project profile, pin skills, and connect MCP servers with explicit grants.",
  },
  ru: {
    "language.auto": "Авто",
    "language.en": "Английский",
    "language.ru": "Русский",
    "nav.home": "Домой",
    "nav.apps": "Утилиты",
    "nav.memory": "Память",
    "nav.memoryWithName": "Память · {name}",
    "nav.automations": "Автоматизации",
    "nav.browser": "Браузер",
    "nav.settings": "Настройки",
    "nav.newProject": "+ Проект",
    "nav.newProjectTitle": "Новый проект",
    "nav.newPane": "+ Панель",
    "nav.newPaneTitle": "Добавить панель",
    "nav.activeProject": "Активный проект",
    "nav.noProjects": "Нет проектов",
    "nav.chooseProject": "Выбери проект",
    "nav.closeTab": "Закрыть таб",
    "nav.closePane": "Закрыть панель",
    "header.threadLabel": "потоков",
    "header.projectLabel": "проектов",
    "settings.title": "Настройки",
    "settings.capabilities": "Возможности",
    "settings.logs": "Логи и события",
    "settings.languageLabel": "Язык интерфейса",
    "settings.layerTitle": "Слой Reflex OS",
    "settings.layerBody":
      "Reflex — локальная macOS-надстройка над Codex CLI: проекты, темы, browser/MCP bridge, генерируемые утилиты, widgets, memory, RAG и запланированные автоматизации живут в одном workspace.",
    "settings.summaryLabel": "Сводка Reflex OS",
    "settings.systemMap": "Карта системы",
    "settings.bridgeTitle": "Bridge для генерируемых утилит",
    "settings.bridgeSearch": "Поиск API, helpers, permissions…",
    "settings.methodsCount": "{visible}/{total} методов",
    "settings.noMatches": "Нет совпадений.",
    "settings.recipesTitle": "Рабочие связки bridge",
    "settings.recipesCount": "{visible}/{total} связок",
    "settings.helpersTitle": "Runtime helpers",
    "settings.helpersCount": "{visible}/{total} helpers",
    "settings.helpersHint":
      "Генерируемым утилитам стоит использовать эти helpers вместо ручного postMessage; permissions и правила manifest.network всё равно применяются к базовому bridge method.",
    "settings.permissionsTitle": "Разрешения",
    "settings.grantsCount": "{visible}/{total} manifest grants",
    "settings.automationFlow": "Поток автоматизации",
    "settings.flowSchedules": "manifest.schedules",
    "settings.flowRunner": "scheduler runner",
    "settings.flowBridge": "bridge steps",
    "settings.flowHistory": "история запусков",
    "settings.automationHint":
      "Генерируемые утилиты могут обновлять собственный manifest, добавлять schedules/actions, смотреть runs и отдавать widgets или public actions другим apps.",
    "settings.copy": "Скопировать",
    "settings.copied": "Скопировано",
    "settings.allSources": "все источники",
    "settings.logSearch": "Поиск по тексту…",
    "settings.resume": "Возобновить",
    "settings.pause": "Пауза",
    "settings.clear": "Очистить",
    "settings.clearTitle": "Очистить вид; буфер бэка не трогается",
    "settings.rows": "{count} строк",
    "settings.noLogs": "Логов нет.",
    "appViewer.manifestCapabilities": "Возможности manifest",
    "appViewer.manifestPermissions": "Разрешения manifest",
    "appViewer.permissions": "Права",
    "appViewer.networkHosts": "Сетевые хосты",
    "appViewer.bridgeCatalog": "Каталог runtime bridge",
    "appViewer.bridgeSearch": "Поиск методов, helpers, связок...",
    "appViewer.methodsCount": "{visible}/{total} методов",
    "appViewer.helpersCount": "{visible}/{total} helpers",
    "appViewer.noBridgeMatches": "Нет совпадений в bridge.",
    "appViewer.copy": "Скопировать",
    "appViewer.methods": "Методы",
    "appViewer.helpers": "Хелперы",
    "appViewer.exportDialogTitle": "Сохранить .reflexapp",
    "appViewer.revertConfirm":
      "Откатиться к предыдущей версии? Несохранённые изменения будут потеряны.",
    "appViewer.restartServer": "Перезапуск",
    "appViewer.restartServerTitle": "Перезапустить сервер",
    "appViewer.serverLogsTitle": "Показать логи сервера",
    "appViewer.logs": "Логи",
    "appViewer.runtimeHelpersTitle": "Показать runtime overlay helpers",
    "appViewer.inspectorTitle":
      "Кликни по элементу в утилите и опиши, что изменить",
    "appViewer.inspector": "Инспектор",
    "appViewer.editExistingThreadTitle":
      "Открыть существующий тред для доработки и привязать к этой утилите",
    "appViewer.edit": "Править",
    "appViewer.newThreadTitle":
      "Создать новый тред для изолированных изменений",
    "appViewer.newThread": "Новый тред",
    "appViewer.exportTitle": "Экспортировать утилиту в .reflexapp файл",
    "appViewer.export": "Экспорт",
    "appViewer.actions": "Действия",
    "appViewer.public": "публичное",
    "appViewer.params": "параметры",
    "appViewer.actionParamsEditor": "Редактор параметров action",
    "appViewer.run": "Запустить",
    "appViewer.unsavedChanges": "Есть несохранённые изменения.",
    "appViewer.diffTitle": "Посмотреть diff и применить выборочно",
    "appViewer.commit": "Зафиксировать",
    "appViewer.save": "Сохранить",
    "appViewer.revert": "Откатить",
    "appViewer.reload": "Перезагрузить",
    "appViewer.saveRevision": "Сохранение ревизии",
    "appViewer.appCrashed": "App упал:",
    "appViewer.errorFixTitle":
      "Отправить ошибку Codex'у с просьбой починить",
    "appViewer.dismiss": "Скрыть",
    "appViewer.close": "Закрыть",
    "appViewer.selected": "выбрано",
    "appViewer.inspectorPlaceholder":
      "Что изменить в этом элементе? (Cmd+Enter)",
    "appViewer.applying": "Применяю...",
    "appViewer.startingServer": "Запускаю локальный сервер утилиты...",
    "appViewer.serverStartFailed": "Сервер не стартовал: {error}",
    "appViewer.serverCrashed": "Сервер упал: {error}",
    "appViewer.generationIncomplete": "Генерация не завершена",
    "appViewer.generationIncompleteBefore": "Codex ещё не записал",
    "appViewer.generationIncompleteAfter":
      "Возможно процесс прерван или сейчас в плане ждёт подтверждения.",
    "appViewer.checkAgain": "Проверить ещё раз",
    "appViewer.serverLogs": "логи сервера",
    "appViewer.clearLogsTitle": "Очистить локальный буфер логов",
    "appViewer.clear": "очистить",
    "appViewer.empty": "пусто",
    "appViewer.selectThread": "Выбери тред слева.",
    "apps.newUtility": "+ Новая утилита",
    "apps.deletedAppsTitle": "Удалённые приложения",
    "apps.trash": "Корзина",
    "apps.headerHint":
      "Утилиты общаются с агентом через мост Reflex. Опиши что хочешь, и Codex напишет.",
    "apps.trashTitle": "Корзина",
    "apps.trashEmpty": "Пусто.",
    "apps.deletedAt": "удалено {age}",
    "apps.restore": "Восстановить",
    "apps.deleteForever": "Навсегда",
    "apps.empty": "Утилит пока нет.",
    "apps.open": "Открыть",
    "apps.writingFiles": "Codex ещё пишет файлы; клик чтобы посмотреть",
    "apps.moveToTrash": "В корзину",
    "apps.creatingBadge": "создаётся...",
    "apps.newUtilityTitle": "Новая утилита Reflex",
    "apps.chooseTemplate": "Выбери шаблон под задачу.",
    "apps.linkedToProject": "Будет привязана к {name}",
    "apps.cancel": "Отмена",
    "apps.importTitle": "Импорт .reflexapp",
    "apps.importBundleTitle": "Импортировать .reflexapp бандл",
    "apps.importBundle": "Импорт .reflexapp",
    "apps.next": "Дальше ->",
    "apps.back": "<- Назад",
    "apps.describeHint":
      "Опиши, что должна делать утилита. Codex напишет файлы в фоне.",
    "apps.createShortcut": "Создать (⌘↵)",
    "apps.creating": "Создаю...",
    "apps.deleteConfirm": "Переместить \"{name}\" в корзину?",
    "apps.purgeConfirm":
      "Удалить \"{name}\" окончательно? Это действие необратимо.",
    "apps.justNow": "только что",
    "apps.minutesAgo": "{count} мин назад",
    "apps.hoursAgo": "{count} ч назад",
    "apps.daysAgo": "{count} дн назад",
    "template.blank.name": "Пустая",
    "template.blank.description": "Пустая утилита, Codex решает структуру",
    "template.blank.placeholder":
      "Например: счётчик с кнопкой сохранения в storage; виджет погоды; ...",
    "template.chat.name": "Чат-утилита",
    "template.chat.description": "Чат с агентом, стриминг ответа",
    "template.chat.placeholder":
      "Например: ассистент по моему календарю; помощник с переводом; ...",
    "template.dashboard.name": "Дашборд",
    "template.dashboard.description":
      "Данные через agent.task в виде таблицы/карточек",
    "template.dashboard.placeholder":
      "Например: статус всех проектов из ~/projects; список последних коммитов; ...",
    "template.healthDashboard.name": "Дашборд здоровья",
    "template.healthDashboard.description":
      "Операционный обзор scheduler, memory/RAG и linked apps",
    "template.healthDashboard.placeholder":
      "Например: мониторинг автоматизаций проекта, индекса памяти и server apps с компактным виджетом; ...",
    "template.form.name": "Форма",
    "template.form.description": "Поля -> Run -> результат через agent.task",
    "template.form.placeholder":
      "Например: переписать текст в нужном стиле; сгенерить regex по описанию; ...",
    "template.apiClient.name": "API-клиент",
    "template.apiClient.description": "Запросы к внешнему API через net.fetch",
    "template.apiClient.placeholder":
      "Например: показать issues из github.com/owner/repo; конвертер валют через open.er-api.com; ...",
    "template.automation.name": "Автоматизация",
    "template.automation.description":
      "Расписание, action и виджет для фоновой задачи",
    "template.automation.placeholder":
      "Например: раз в час проверять важные письма и сохранять краткую сводку; каждое утро собирать статус проектов; ...",
    "template.nodeServer.name": "Node-сервер",
    "template.nodeServer.description":
      "runtime=server: своё backend на Node.js stdlib",
    "template.nodeServer.placeholder":
      "Например: WebSocket-чат комната; sqlite-просмотрщик; превью markdown; ...",
    "stats.bridgeMethods": "Методы bridge",
    "stats.overlayHelpers": "Хелперы overlay",
    "stats.workflows": "Связки",
    "stats.permissionForms": "Формы прав",
    "stats.dispatchApi": "dispatch API",
    "stats.windowReflex": "window.reflex*",
    "stats.bridgeWorkflows": "рабочие связки",
    "stats.manifestGrants": "manifest grants",
    "cap.projects.title": "Проекты",
    "cap.projects.body":
      "Папки с sandbox, browser MCP, MCP servers, профилем агента, preferred skills, связанными утилитами, widgets и indexed files.",
    "cap.topics.title": "Топики",
    "cap.topics.body":
      "Codex threads с профилем проекта, memory recall и продолжением рабочей сессии.",
    "cap.apps.title": "Генерируемые утилиты",
    "cap.apps.body":
      "Static или local server apps с manifest, storage, actions, widgets и Reflex bridge APIs.",
    "cap.memory.title": "Память",
    "cap.memory.body":
      "Глобальные, проектные и topic notes, плюс RAG по индексированным файлам и сохранённым фактам.",
    "cap.automations.title": "Автоматизации",
    "cap.automations.body":
      "Manifest schedules и actions, которые исполняются теми же bridge methods, что доступны утилитам.",
    "cap.mcp.title": "MCP и skills",
    "cap.mcp.body":
      "Project-scoped MCP JSON и preferred skills внедряются в новые, продолженные и auto-resumed topics.",
    "bridge.systemManifest": "Система и manifest",
    "bridge.agentRuntime": "Агентный runtime",
    "bridge.appDataFiles": "Данные app и файлы",
    "bridge.projectsTopics": "Проекты и топики",
    "bridge.browserSidecar": "Браузерный sidecar",
    "bridge.nativeMacos": "Нативный macOS",
    "bridge.network": "Сеть",
    "bridge.memory": "Память",
    "bridge.automations": "Автоматизации",
    "bridge.appGrid": "Сетка apps",
    "bridge.base": "База",
    "bridge.agent": "Агент",
    "bridge.storageIo": "Хранилище / IO",
    "bridge.projectsBrowser": "Проекты / браузер",
    "bridge.memoryAutomationApps": "Память / автоматизации / утилиты",
    "recipe.contextAgent.title": "Контекстный sub-agent",
    "recipe.contextAgent.body":
      "Project cwd подключает sandbox, MCP, preferred skills, project profile и memory/RAG.",
    "recipe.longMemory.title": "Долгая память",
    "recipe.longMemory.body":
      "Сохраняй новые факты и обновляй известный relPath без дублей.",
    "recipe.capabilities.title": "Возможности",
    "recipe.capabilities.body":
      "Добавляй permissions и network hosts точечно, без ручного слияния manifest.",
    "recipe.utilityService.title": "Утилита как сервис",
    "recipe.utilityService.body":
      "Публикуй actions/widgets, создавай утилиты, экспортируй bundles и управляй server runtime.",
    "recipe.automation.title": "Автоматизация",
    "recipe.automation.body":
      "Schedule steps используют тот же bridge, кроме UI-only методов.",
    "recipe.healthDashboard.title": "Дашборд здоровья",
    "recipe.healthDashboard.body":
      "Показывай состояние автоматизаций, RAG индекса и последнюю ошибку; для общего обзора добавь scheduler.read:*.",
    "recipe.projectFiles.title": "Файлы проекта",
    "recipe.projectFiles.body":
      "Ищи, правь и переиндексируй файлы linked project через bridge, не выходя из sandbox утилиты.",
    "recipe.appRevisions.title": "Ревизии утилиты",
    "recipe.appRevisions.body":
      "Показывай diff generated app, сохраняй осмысленные ревизии и откатывай неудачные правки.",
    "recipe.eventGrid.title": "Сетка событий",
    "recipe.eventGrid.body":
      "Связывай утилиты через topics, recent history и подписки без прямых зависимостей между ними.",
    "recipe.browserSidecar.title": "Браузерный sidecar",
    "recipe.browserSidecar.body":
      "Включай project Browser MCP, открывай страницы, читай outline и заполняй формы.",
    "recipe.projectMcpSkills.title": "MCP и skills проекта",
    "recipe.projectMcpSkills.body":
      "Обновляй project profile, закрепляй skills и подключай MCP servers с явными grants.",
  },
};

const languageSettings: LanguageSetting[] = ["auto", "en", "ru"];

function readStoredLanguage(): LanguageSetting {
  try {
    const value = window.localStorage.getItem(STORAGE_KEY);
    if (languageSettings.includes(value as LanguageSetting)) {
      return value as LanguageSetting;
    }
  } catch {}
  return "auto";
}

function detectLocale(language: LanguageSetting): Locale {
  if (language === "en" || language === "ru") return language;
  if (typeof navigator !== "undefined") {
    const preferred = navigator.languages?.[0] ?? navigator.language;
    if (preferred?.toLowerCase().startsWith("ru")) return "ru";
  }
  return "en";
}

function interpolate(
  template: string,
  values?: Record<string, string | number>,
): string {
  if (!values) return template;
  return template.replace(/\{(\w+)\}/g, (match, key) =>
    Object.prototype.hasOwnProperty.call(values, key)
      ? String(values[key])
      : match,
  );
}

const I18nContext = createContext<I18nContextValue | null>(null);

export function I18nProvider({ children }: { children: ReactNode }) {
  const [language, setLanguageState] = useState<LanguageSetting>(
    readStoredLanguage,
  );
  const locale = detectLocale(language);

  useEffect(() => {
    document.documentElement.lang = locale;
    document.documentElement.dataset.locale = locale;
  }, [locale]);

  const setLanguage = useCallback((next: LanguageSetting) => {
    setLanguageState(next);
    try {
      window.localStorage.setItem(STORAGE_KEY, next);
    } catch {}
  }, []);

  const t = useCallback<Translate>(
    (key, values) =>
      interpolate(dictionaries[locale][key] ?? dictionaries.en[key] ?? key, values),
    [locale],
  );

  const value = useMemo(
    () => ({ language, locale, setLanguage, t }),
    [language, locale, setLanguage, t],
  );

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18nContextValue {
  const value = useContext(I18nContext);
  if (!value) {
    throw new Error("useI18n must be used within I18nProvider");
  }
  return value;
}
