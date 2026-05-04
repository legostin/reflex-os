import { readFileSync } from "node:fs";
import { join } from "node:path";

const root = process.cwd();
const read = (path) => readFileSync(join(root, path), "utf8");

function sliceBetween(source, start, end) {
  const startIndex = source.indexOf(start);
  if (startIndex < 0) throw new Error(`Missing marker: ${start}`);
  const endIndex = source.indexOf(end, startIndex + start.length);
  if (endIndex < 0) throw new Error(`Missing marker: ${end}`);
  return source.slice(startIndex, endIndex);
}

function requireIncludes(failures, source, title, snippets) {
  const missing = snippets.filter((snippet) => !source.includes(snippet));
  if (missing.length > 0) {
    failures.push(`${title}:\n  - ${missing.join("\n  - ")}`);
  }
}

function requirePattern(failures, source, title, pattern) {
  if (!pattern.test(source)) failures.push(title);
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

const chatThread = read("src/components/ChatThread.tsx");
const chatCss = read("src/components/ChatThread.css");
const i18n = read("src/i18n.tsx");
const packageJson = JSON.parse(read("package.json"));

const dashboardBlock = sliceBetween(
  chatThread,
  "type DashboardRecord = {",
  "function ProjectScreen(",
);
const valueRenderBlock = sliceBetween(
  chatThread,
  "function DashboardValueView({",
  "function ProjectDashboard(",
);
const recordSearchBlock = sliceBetween(
  chatThread,
  "function dashboardSafeSearchText(",
  "function dashboardSourceScoreForSpec(",
);

const failures = [];

requireIncludes(failures, dashboardBlock, "Dashboard view spec contract is incomplete", [
  "type DashboardViewLayout =",
  "type DashboardSortMode =",
  "type DashboardWidgetSize =",
  "type DashboardValueFilter =",
  "type DashboardViewSpec =",
  "type DashboardSourceBlueprint =",
  "sourceKey?: string;",
  "filters: DashboardValueFilter[];",
  "sort: DashboardSortMode;",
  "size: DashboardWidgetSize;",
  "showMeta: boolean;",
  "function buildDashboardViewSpec(",
  "function inferDashboardSize(",
  "function nextDashboardWidgetSize(",
  "function nextDashboardWidgetLayout(",
  "function nextDashboardWidgetSort(",
  "DASHBOARD_WIDGET_SIZE_ORDER",
  "DASHBOARD_WIDGET_LAYOUT_ORDER",
  "DASHBOARD_WIDGET_SORT_ORDER",
  "function inferDashboardFilters(",
  "function inferDashboardSort(",
  "function projectDashboardValue(",
]);

requireIncludes(failures, dashboardBlock, "Widget rendering capabilities are incomplete", [
  "function DashboardCompositeValueView(",
  "function aggregateDashboardMetrics(",
  "function aggregateDashboardTable(",
  "function DashboardWidgetSpecPreview(",
  "function DashboardSourceBlueprintView(",
  "function customDashboardWidgetFromSource(",
  "function matchDashboardSourcesForSpec(",
  "function matchDashboardSourcesForWidget(",
  "function dashboardSourceMatchedTokensForSpec(",
  "function dashboardSafeSearchText(",
  "function buildDashboardSourceBlueprint(",
  "dashboard.widgetMeta",
  "const saveEditedCustomWidget =",
  "const startEditCustomWidget =",
  "const moveCustomWidget =",
  "const cycleCustomWidgetSize =",
  "const cycleCustomWidgetLayout =",
  "const cycleCustomWidgetSort =",
  "const pinActionAsWidget =",
  "function buildDashboardWidgetTaskPrompt(",
  "function buildDashboardWidgetRepairPrompt(",
  "const createWidgetSourceTask =",
  "const createWidgetRepairTask =",
  "const createActionSourceRepairTask =",
  "dashboard.noSourceHint",
  "dashboard.sourceBlueprintTitle",
  "dashboard.repairSourceTask",
  "dashboard.createSourceTask",
]);

for (const filterId of [
  "failed",
  "open",
  "closed",
  "running",
  "pending",
  "blocked",
  "disabled",
  "ready",
  "stale",
]) {
  requireIncludes(
    failures,
    dashboardBlock,
    `Dashboard filter definition missing: ${filterId}`,
    [`id: "${filterId}"`],
  );
  requireIncludes(
    failures,
    i18n,
    `Dashboard filter i18n missing: ${filterId}`,
    [`"dashboard.filter.${filterId}"`],
  );
}

for (const key of [
  "dashboard.previewTitle",
  "dashboard.previewSort",
  "dashboard.previewSize",
  "dashboard.previewFilters",
  "dashboard.previewMatches",
  "dashboard.previewMatchSignals",
  "dashboard.previewMatchScore",
  "dashboard.sourceColumn",
  "dashboard.widgetMeta",
  "dashboard.pinWidget",
  "dashboard.pinnedWidget",
  "dashboard.saveChanges",
  "dashboard.editWidget",
  "dashboard.moveWidgetUp",
  "dashboard.moveWidgetDown",
  "dashboard.cycleWidgetSize",
  "dashboard.cycleWidgetLayout",
  "dashboard.cycleWidgetSort",
  "dashboard.filteredItemsCount",
  "dashboard.noSourceHint",
  "dashboard.sourceBlueprintTitle",
  "dashboard.sourceBlueprintKind",
  "dashboard.sourceBlueprintFields",
  "dashboard.createSourceTask",
  "dashboard.creatingSourceTask",
  "dashboard.repairSourceHint",
  "dashboard.repairSourceTask",
  "dashboard.repairingSourceTask",
  "dashboard.size.compact",
  "dashboard.size.normal",
  "dashboard.size.wide",
  "dashboard.size.full",
]) {
  const count = [...i18n.matchAll(new RegExp(`"${escapeRegExp(key)}"`, "g"))].length;
  if (count < 2) failures.push(`Dashboard i18n key is not translated in both locales: ${key}`);
}

requireIncludes(failures, chatCss, "Dashboard widget preview styles are incomplete", [
  ".dashboard-widget-preview",
  ".dashboard-widget-preview-chip",
  ".dashboard-widget-preview-chip-signals",
  ".dashboard-action-card-actions",
  ".dashboard-widget-wide",
  ".dashboard-widget-full",
  ".dashboard-custom-actions",
  ".dashboard-composite-value",
  ".dashboard-empty-hint",
  ".dashboard-source-blueprint",
  ".dashboard-source-blueprint-chip",
  ".dashboard-widget-preview-missing",
  ".dashboard-value-empty-action",
]);

if (/allActionSources\.slice\(0,\s*1\)/.test(dashboardBlock)) {
  failures.push("Custom widgets must not fall back to an arbitrary first action source.");
}

if (/JSON\.stringify|previewJsonValue/.test(valueRenderBlock)) {
  failures.push("Dashboard value renderer must not display raw JSON dumps.");
}

if (/JSON\.stringify/.test(recordSearchBlock)) {
  failures.push("Dashboard source matching must not stringify raw records.");
}
requirePattern(
  failures,
  recordSearchBlock,
  "Dashboard source matching must skip secret-bearing keys.",
  /DASHBOARD_SECRET_KEY_PATTERNS/,
);
requirePattern(
  failures,
  dashboardBlock,
  "Dashboard source matches must preserve the matched signals for explainability.",
  /matchedTokens: string\[\][\s\S]+dashboardSourceMatchedTokensForSpec/,
);

requirePattern(
  failures,
  dashboardBlock,
  "Dashboard action cards must be pinnable as editable custom widgets.",
  /sourceKey: dashboardSourceKey\(source\)|sourceKey,\s*[\s\S]+spec: dashboardSpecForSource\(source\)/,
);

requirePattern(
  failures,
  dashboardBlock,
  "Pinned dashboard widgets must preserve source affinity even with weak matching.",
  /widget\.sourceKey[\s\S]+pinnedSource[\s\S]+matchedTokens/,
);

requirePattern(
  failures,
  dashboardBlock,
  "Dashboard table aggregation must keep source context for multi-source tables.",
  /aggregateDashboardTable[\s\S]+dashboard\.sourceColumn/,
);

requirePattern(
  failures,
  dashboardBlock,
  "Custom widget creation must persist a compiled view spec.",
  /const widget: CustomDashboardWidget =[\s\S]+spec: buildDashboardViewSpec\(prompt\)/,
);

requirePattern(
  failures,
  dashboardBlock,
  "Dashboard source task prompt must consider open-source/API wrappers.",
  /open-source repository[\s\S]+wrapper target/,
);

requirePattern(
  failures,
  dashboardBlock,
  "Dashboard source task prompt must require safe public no-arg actions.",
  /public: true[\s\S]+no required params[\s\S]+safe to run automatically/,
);

requirePattern(
  failures,
  dashboardBlock,
  "Dashboard source task prompt must include bridge or MCP integration work.",
  /bridge\/MCP layer/,
);

requirePattern(
  failures,
  dashboardBlock,
  "Dashboard source task prompt must include the inferred source contract.",
  /Recommended source contract[\s\S]+Expected fields/,
);

requirePattern(
  failures,
  dashboardBlock,
  "Dashboard widget preview must show the inferred source contract when no source matches.",
  /missingSourceBlueprint[\s\S]+DashboardSourceBlueprintView/,
);

requirePattern(
  failures,
  dashboardBlock,
  "Dashboard widgets must offer a repair task for matched but unusable sources.",
  /emptyAction[\s\S]+createWidgetRepairTask/,
);

requirePattern(
  failures,
  dashboardBlock,
  "Dashboard action cards must offer a repair task for unusable public action output.",
  /DashboardValueView[\s\S]+emptyAction[\s\S]+createActionSourceRepairTask/,
);

requirePattern(
  failures,
  dashboardBlock,
  "Dashboard repair prompt must target reusable utility output instead of UI heuristics.",
  /Repair the matched utility integration[\s\S]+Fix the reusable utility output/,
);

const domainLeakPattern = /\b(OpenF1|telegram|Telegram|race)\b|гонк/;
if (domainLeakPattern.test(dashboardBlock)) {
  failures.push("Dashboard widget layer contains a domain-specific example or heuristic.");
}

if (!packageJson.scripts?.["check:dashboard"]) {
  failures.push("package.json is missing scripts.check:dashboard.");
}
if (!packageJson.scripts?.["check:dashboard:fixtures"]) {
  failures.push("package.json is missing scripts.check:dashboard:fixtures.");
}
if (!packageJson.scripts?.build?.includes("check:dashboard")) {
  failures.push("package.json build script must run check:dashboard.");
}
if (!packageJson.scripts?.build?.includes("check:dashboard:fixtures")) {
  failures.push("package.json build script must run check:dashboard:fixtures.");
}

if (failures.length > 0) {
  console.error(`Dashboard widget check failed:\n\n${failures.join("\n\n")}`);
  process.exit(1);
}

console.log("Dashboard widget check passed.");
