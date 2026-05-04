import { readFileSync } from "node:fs";
import { join } from "node:path";
import ts from "typescript";

const root = process.cwd();
const read = (path) => readFileSync(join(root, path), "utf8");

function sliceBetween(source, start, end) {
  const startIndex = source.indexOf(start);
  if (startIndex < 0) throw new Error(`Missing marker: ${start}`);
  const endIndex = source.indexOf(end, startIndex + start.length);
  if (endIndex < 0) throw new Error(`Missing marker: ${end}`);
  return source.slice(startIndex, endIndex);
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

const chatThread = read("src/components/ChatThread.tsx");
const projectionCore = sliceBetween(
  chatThread,
  "function dashboardTokens(input: string): string[]",
  "function dashboardCountLabel(",
);

const prelude = `
function isJsonObject(value: unknown): value is Record<string, any> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}

function previewJsonValue(value: unknown): string {
  if (value == null) return "null";
  if (typeof value === "string") return value.slice(0, 240);
  try {
    return JSON.stringify(value, null, 2).slice(0, 240);
  } catch {
    return String(value).slice(0, 240);
  }
}

function unwrapActionResult(value: any): unknown {
  if (isJsonObject(value) && "result" in value) return value.result;
  return value;
}

function normalizeDashboardValue(value: unknown): unknown {
  let result = unwrapActionResult(value);
  if (isJsonObject(result)) {
    const keys = Object.keys(result);
    if (keys.length === 1 && keys[0] === "value") {
      result = result.value;
    }
  }
  return result;
}

function dashboardSourceKey(source: any): string {
  return \`\${source.appId}::\${source.action.id}\`;
}
`;

const source = `
const __dashboardFixture = (() => {
${prelude}
${projectionCore}
return {
  buildDashboardViewSpec,
  buildDashboardSourceBlueprint,
  dashboardRecordSearchText,
  dashboardSourceScoreForSpec,
  matchDashboardSourcesForSpec,
  matchDashboardSourcesForWidget,
  projectDashboardValue,
};
})();
`;

const js = ts.transpileModule(source, {
  compilerOptions: {
    module: ts.ModuleKind.ESNext,
    target: ts.ScriptTarget.ES2020,
  },
}).outputText;

const dashboard = new Function(`${js}; return __dashboardFixture;`)();

const failedJobsSpec = dashboard.buildDashboardViewSpec("failed jobs count");
assert(failedJobsSpec.layout === "metric", "failed jobs count should infer metric layout");
assert(failedJobsSpec.size === "compact", "metric widgets should infer compact size");
assert(
  failedJobsSpec.filters.some((filter) => filter.id === "failed"),
  "failed jobs count should infer failed filter",
);
const failedJobsBlueprint = dashboard.buildDashboardSourceBlueprint(failedJobsSpec);
assert(failedJobsBlueprint.resultKind === "metric", "failed jobs blueprint should be metric");
assert(
  failedJobsBlueprint.actionId.includes("failed"),
  "failed jobs blueprint action id should carry failed signal",
);
assert(
  failedJobsBlueprint.fields.includes("value"),
  "metric blueprint should require a value field",
);
const failedJobsProjection = dashboard.projectDashboardValue(
  {
    jobs: [
      { title: "Deploy", status: "failed", updated_at: "2026-05-04T10:00:00Z" },
      { title: "Backup", status: "succeeded", updated_at: "2026-05-04T11:00:00Z" },
    ],
  },
  failedJobsSpec,
);
assert(failedJobsProjection.lists[0]?.count === 1, "failed jobs should filter to one item");
assert(failedJobsProjection.lists[0]?.totalCount === 2, "failed jobs should preserve total count");
assert(failedJobsProjection.metrics[0]?.value === "1", "failed jobs metric should show filtered count");

const openTasksSpec = dashboard.buildDashboardViewSpec("wide open tasks table");
assert(openTasksSpec.layout === "table", "open tasks table should infer table layout");
assert(openTasksSpec.size === "wide", "wide table should infer wide size");
assert(
  openTasksSpec.filters.some((filter) => filter.id === "open"),
  "open tasks table should infer open filter",
);
const openTasksBlueprint = dashboard.buildDashboardSourceBlueprint(openTasksSpec);
assert(openTasksBlueprint.resultKind === "table", "open tasks blueprint should be table");
assert(
  openTasksBlueprint.fields.includes("items[].title"),
  "table blueprint should require item titles",
);
assert(
  openTasksBlueprint.fields.includes("status"),
  "open tasks blueprint should include filter status hints",
);
const taskSource = {
  appId: "tasks",
  appName: "Task Utility",
  action: {
    id: "tasks_overview",
    name: "Tasks overview",
    description: "Open tasks dashboard table",
  },
};
const openTasksMatches = dashboard.matchDashboardSourcesForSpec(openTasksSpec, [
  taskSource,
], {}, 4);
assert(openTasksMatches[0]?.matchedTokens.includes("open"), "matches should explain open signal");
assert(openTasksMatches[0]?.matchedTokens.includes("tasks"), "matches should explain tasks signal");
assert(
  openTasksMatches[0]?.score === openTasksMatches[0]?.matchedTokens.length,
  "match score should equal matched signal count",
);
const pinnedWeakMatches = dashboard.matchDashboardSourcesForWidget(
  {
    id: "pinned",
    title: "Pinned",
    prompt: "unrelated metric",
    createdAtMs: 0,
    sourceKey: "tasks::tasks_overview",
  },
  dashboard.buildDashboardViewSpec("unrelated metric"),
  [taskSource],
  {},
  3,
);
assert(
  pinnedWeakMatches[0]?.key === "tasks::tasks_overview",
  "pinned widgets should preserve their source even when matching is weak",
);
const openTasksProjection = dashboard.projectDashboardValue(
  {
    tasks: [
      { title: "Review PR", status: "open", priority: 2 },
      { title: "Ship release", status: "closed", priority: 5 },
    ],
  },
  openTasksSpec,
);
assert(openTasksProjection.table, "open tasks table should produce a projected table");
assert(openTasksProjection.table.count === 1, "open tasks table should filter to one row");
assert(openTasksProjection.table.totalCount === 2, "open tasks table should preserve total rows");
assert(
  openTasksProjection.table.rows.some((row) => Object.values(row).includes("Review PR")),
  "open tasks table should include the open task title",
);

const withoutOwnerSpec = dashboard.buildDashboardViewSpec("summary without owner");
assert(
  withoutOwnerSpec.excludeKeys.includes("owner"),
  "without owner should infer owner exclusion",
);
assert(
  !withoutOwnerSpec.includeTokens.includes("owner"),
  "excluded owner field should not remain a source matching token",
);
const withoutOwnerProjection = dashboard.projectDashboardValue(
  { owner: "Alice", status: "ok" },
  withoutOwnerSpec,
);
assert(
  !withoutOwnerProjection.rows.some((row) => row.value.includes("Alice")),
  "excluded owner field should not render",
);
const withoutRussianOwnerSpec = dashboard.buildDashboardViewSpec("summary без владельца");
assert(
  withoutRussianOwnerSpec.excludeKeys.includes("owner"),
  "Russian owner exclusion should map to owner key",
);
assert(
  !withoutRussianOwnerSpec.excludeKeys.includes("владельца"),
  "Russian owner exclusion should not keep localized key noise",
);

const latestErrorsSpec = dashboard.buildDashboardViewSpec("latest errors list");
assert(latestErrorsSpec.layout === "list", "latest errors list should infer list layout");
assert(latestErrorsSpec.sort === "latest", "latest errors list should infer latest sort");
assert(
  latestErrorsSpec.filters.some((filter) => filter.id === "failed"),
  "latest errors list should infer failed/error filter",
);
const latestOneErrorSpec = dashboard.buildDashboardViewSpec("latest 1 errors list");
assert(latestOneErrorSpec.maxItems === 1, "latest 1 errors list should infer max item count");
const latestErrorsProjection = dashboard.projectDashboardValue(
  {
    events: [
      { title: "Old crash", status: "error", updated_at: "2026-05-04T09:00:00Z" },
      { title: "New crash", status: "error", updated_at: "2026-05-04T12:00:00Z" },
      { title: "Heartbeat", status: "ok", updated_at: "2026-05-04T13:00:00Z" },
    ],
  },
  latestErrorsSpec,
);
assert(latestErrorsProjection.lists[0]?.count === 2, "latest errors should keep only error events");
assert(
  latestErrorsProjection.lists[0]?.items[0]?.includes("New crash"),
  "latest errors should sort matching events newest first",
);
const latestOneErrorProjection = dashboard.projectDashboardValue(
  {
    events: [
      { title: "Old crash", status: "error", updated_at: "2026-05-04T09:00:00Z" },
      { title: "New crash", status: "error", updated_at: "2026-05-04T12:00:00Z" },
    ],
  },
  latestOneErrorSpec,
);
assert(
  latestOneErrorProjection.lists[0]?.items.length === 1,
  "latest 1 errors should limit rendered list items",
);

const safeRecord = {
  value: {
    access_token: "failed-secret-token",
    auth: { refresh_token: "error-secret" },
    jobs: [{ title: "Healthy job", status: "ok" }],
  },
};
const safeText = dashboard.dashboardRecordSearchText(safeRecord);
assert(!safeText.includes("failed-secret-token"), "record search text must not include access token values");
assert(!safeText.includes("refresh_token"), "record search text must not include secret keys");
const safeScore = dashboard.dashboardSourceScoreForSpec(failedJobsSpec, {
  appId: "safe",
  appName: "Safe Utility",
  action: { id: "jobs_status", name: "Jobs status", description: "Healthy job status" },
}, safeRecord);
assert(safeScore === 1, "secret-only failed tokens must not inflate source score");

console.log("Dashboard widget fixture check passed.");
