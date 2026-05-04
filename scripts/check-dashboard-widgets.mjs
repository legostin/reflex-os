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

const failures = [];

requireIncludes(failures, dashboardBlock, "Dashboard view spec contract is incomplete", [
  "type DashboardViewLayout =",
  "type DashboardSortMode =",
  "type DashboardValueFilter =",
  "type DashboardViewSpec =",
  "filters: DashboardValueFilter[];",
  "sort: DashboardSortMode;",
  "showMeta: boolean;",
  "function buildDashboardViewSpec(",
  "function inferDashboardFilters(",
  "function inferDashboardSort(",
  "function projectDashboardValue(",
]);

requireIncludes(failures, dashboardBlock, "Widget rendering capabilities are incomplete", [
  "function DashboardCompositeValueView(",
  "function aggregateDashboardMetrics(",
  "function aggregateDashboardTable(",
  "function DashboardWidgetSpecPreview(",
  "function matchDashboardSourcesForSpec(",
  "const saveEditedCustomWidget =",
  "const startEditCustomWidget =",
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
  "dashboard.previewFilters",
  "dashboard.previewMatches",
  "dashboard.sourceColumn",
  "dashboard.saveChanges",
  "dashboard.editWidget",
  "dashboard.filteredItemsCount",
]) {
  const count = [...i18n.matchAll(new RegExp(`"${key}"`, "g"))].length;
  if (count < 2) failures.push(`Dashboard i18n key is not translated in both locales: ${key}`);
}

requireIncludes(failures, chatCss, "Dashboard widget preview styles are incomplete", [
  ".dashboard-widget-preview",
  ".dashboard-widget-preview-chip",
  ".dashboard-custom-actions",
  ".dashboard-composite-value",
]);

if (/allActionSources\.slice\(0,\s*1\)/.test(dashboardBlock)) {
  failures.push("Custom widgets must not fall back to an arbitrary first action source.");
}

if (/JSON\.stringify|previewJsonValue/.test(valueRenderBlock)) {
  failures.push("Dashboard value renderer must not display raw JSON dumps.");
}

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

const domainLeakPattern = /\b(OpenF1|telegram|Telegram|race)\b|гонк/;
if (domainLeakPattern.test(dashboardBlock)) {
  failures.push("Dashboard widget layer contains a domain-specific example or heuristic.");
}

if (!packageJson.scripts?.["check:dashboard"]) {
  failures.push("package.json is missing scripts.check:dashboard.");
}
if (!packageJson.scripts?.build?.includes("check:dashboard")) {
  failures.push("package.json build script must run check:dashboard.");
}

if (failures.length > 0) {
  console.error(`Dashboard widget check failed:\n\n${failures.join("\n\n")}`);
  process.exit(1);
}

console.log("Dashboard widget check passed.");
