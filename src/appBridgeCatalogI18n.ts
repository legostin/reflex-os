import { BRIDGE_RECIPE_CARDS } from "./appBridgeCatalog";
import type { Translate } from "./i18n";

export type BridgeRecipeCard = (typeof BRIDGE_RECIPE_CARDS)[number];

const BRIDGE_GROUP_TITLE_KEYS: Record<string, string> = {
  "Система и manifest": "bridge.systemManifest",
  "System and Manifest": "bridge.systemManifest",
  "Агентный runtime": "bridge.agentRuntime",
  "Agent Runtime": "bridge.agentRuntime",
  "Данные app и файлы": "bridge.appDataFiles",
  "App Data and Files": "bridge.appDataFiles",
  "Проекты и топики": "bridge.projectsTopics",
  "Projects and Topics": "bridge.projectsTopics",
  "Браузерный sidecar": "bridge.browserSidecar",
  "Browser Sidecar": "bridge.browserSidecar",
  "Нативный macOS": "bridge.nativeMacos",
  "Native macOS": "bridge.nativeMacos",
  "Сеть": "bridge.network",
  Network: "bridge.network",
  "Память": "bridge.memory",
  Memory: "bridge.memory",
  "Автоматизации": "bridge.automations",
  Automations: "bridge.automations",
  "Сетка apps": "bridge.appGrid",
  "App Grid": "bridge.appGrid",
  "База": "bridge.base",
  Base: "bridge.base",
  "Агент": "bridge.agent",
  Agent: "bridge.agent",
  "Хранилище / IO": "bridge.storageIo",
  "Storage / IO": "bridge.storageIo",
  "Проекты / браузер": "bridge.projectsBrowser",
  "Projects / Browser": "bridge.projectsBrowser",
  "Память / автоматизации / утилиты": "bridge.memoryAutomationApps",
  "Memory / Automations / Utilities": "bridge.memoryAutomationApps",
};

const RECIPE_TEXT_KEYS: Record<string, { title: string; body: string }> = {
  "Контекстный sub-agent": {
    title: "recipe.contextAgent.title",
    body: "recipe.contextAgent.body",
  },
  "Contextual sub-agent": {
    title: "recipe.contextAgent.title",
    body: "recipe.contextAgent.body",
  },
  "Долгая память": {
    title: "recipe.longMemory.title",
    body: "recipe.longMemory.body",
  },
  "Long-term memory": {
    title: "recipe.longMemory.title",
    body: "recipe.longMemory.body",
  },
  "Возможности": {
    title: "recipe.capabilities.title",
    body: "recipe.capabilities.body",
  },
  Capabilities: {
    title: "recipe.capabilities.title",
    body: "recipe.capabilities.body",
  },
  "Утилита как сервис": {
    title: "recipe.utilityService.title",
    body: "recipe.utilityService.body",
  },
  "Utility as a service": {
    title: "recipe.utilityService.title",
    body: "recipe.utilityService.body",
  },
  "Автоматизация": {
    title: "recipe.automation.title",
    body: "recipe.automation.body",
  },
  Automation: {
    title: "recipe.automation.title",
    body: "recipe.automation.body",
  },
  "Дашборд здоровья": {
    title: "recipe.healthDashboard.title",
    body: "recipe.healthDashboard.body",
  },
  "Health dashboard": {
    title: "recipe.healthDashboard.title",
    body: "recipe.healthDashboard.body",
  },
  "Файлы проекта": {
    title: "recipe.projectFiles.title",
    body: "recipe.projectFiles.body",
  },
  "Project files": {
    title: "recipe.projectFiles.title",
    body: "recipe.projectFiles.body",
  },
  "Ревизии утилиты": {
    title: "recipe.appRevisions.title",
    body: "recipe.appRevisions.body",
  },
  "Utility revisions": {
    title: "recipe.appRevisions.title",
    body: "recipe.appRevisions.body",
  },
  "Сетка событий": {
    title: "recipe.eventGrid.title",
    body: "recipe.eventGrid.body",
  },
  "Event grid": {
    title: "recipe.eventGrid.title",
    body: "recipe.eventGrid.body",
  },
  "Браузерный sidecar": {
    title: "recipe.browserSidecar.title",
    body: "recipe.browserSidecar.body",
  },
  "Browser sidecar": {
    title: "recipe.browserSidecar.title",
    body: "recipe.browserSidecar.body",
  },
  "MCP и skills проекта": {
    title: "recipe.projectMcpSkills.title",
    body: "recipe.projectMcpSkills.body",
  },
  "Project MCP and skills": {
    title: "recipe.projectMcpSkills.title",
    body: "recipe.projectMcpSkills.body",
  },
};

export function bridgeCatalogTitle(title: string, t: Translate): string {
  const key = BRIDGE_GROUP_TITLE_KEYS[title];
  return key ? t(key) : title;
}

export function bridgeRecipeTitle(
  recipe: BridgeRecipeCard,
  t: Translate,
): string {
  const key = RECIPE_TEXT_KEYS[recipe.title]?.title;
  return key ? t(key) : recipe.title;
}

export function bridgeRecipeBody(
  recipe: BridgeRecipeCard,
  t: Translate,
): string {
  const key = RECIPE_TEXT_KEYS[recipe.title]?.body;
  return key ? t(key) : recipe.body;
}
