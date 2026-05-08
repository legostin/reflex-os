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

const chatThread = read("src/components/ChatThread.tsx");
const i18n = read("src/i18n.tsx");
const libRs = read("src-tauri/src/lib.rs");
const packageJson = JSON.parse(read("package.json"));

const projectScreen = sliceBetween(
  chatThread,
  "function ProjectScreen(",
  "function StatusDot(",
);
const submitQuickImpl = sliceBetween(
  libRs,
  "pub(crate) fn submit_quick_impl(",
  "fn validate_local_image_paths(",
);

const failures = [];

function requireIncludes(source, title, snippets) {
  const missing = snippets.filter((snippet) => !source.includes(snippet));
  if (missing.length > 0) {
    failures.push(`${title}:\n  - ${missing.join("\n  - ")}`);
  }
}

function requireAbsent(source, title, snippets) {
  const present = snippets.filter((snippet) => source.includes(snippet));
  if (present.length > 0) {
    failures.push(`${title}:\n  - ${present.join("\n  - ")}`);
  }
}

requireIncludes(projectScreen, "Project operating center is missing core blocks", [
  "<TopicComposer",
  "<ProjectSuggestions",
  "project.reflection",
  "startReflection",
  "parseProjectReflection",
  "project.operatingCenter",
  "project.activeFlows",
  "project.toolsAndContext",
]);

requireIncludes(chatThread, "Project reflection launch must be a bounded complex-source topic", [
  "truncateForReflection",
  '"reflection",',
]);

requireIncludes(chatThread, "Project suggestions UI is missing its agent-facing label", [
  "project.agentSuggestions",
  "project.reflectNow",
  "project.projectSummary",
  "ProjectSkillsPanel",
]);

requireIncludes(read("src/components/TopicComposer.tsx"), "Topic composer is missing /dream support", [
  'token: "/dream"',
  "topicComposer.commandDream",
]);

requireIncludes(read("src-tauri/src/project.rs"), "Project model is missing persisted reflection state", [
  "pub reflection: Option<ProjectReflection>",
  "pub struct ProjectReflection",
  "pub struct ProjectReflectionSuggestion",
]);

requireIncludes(read("src-tauri/src/lib.rs"), "Project reflection update command is missing", [
  "fn update_project_reflection(",
  "update_project_reflection,",
]);

requireIncludes(read("src-tauri/src/system_settings.rs"), "Reflection topics must use the complex request profile", [
  '"reflection" => RequestKind::Complex',
]);

requireIncludes(submitQuickImpl, "submit_quick must surface turn_start failures in the created topic", [
  "storage::append_event_oneshot(&root_for_task, &reflex_id, &stored)",
  '"stream": "error"',
  "storage::finalize_thread(&root_for_task, &reflex_id, Some(-1)",
  '"reflex://codex-end"',
]);

requireAbsent(projectScreen, "Project screen still exposes legacy dashboard/topic modal UI", [
  "<ProjectDashboard",
  "showNewTopic",
  "newTopicPrompt",
  "project-start-panel",
  "project-dashboard",
]);

for (const key of [
  "project.operatingCenter",
  "project.operatingCenterPrompt",
  "project.agentSuggestions",
  "project.activeFlows",
  "project.toolsAndContext",
  "project.startFlow",
  "project.suggestion.continueFlow.title",
  "project.suggestion.describeProject.title",
  "project.suggestion.createUtility.title",
  "project.suggestion.reviewProject.title",
  "project.suggestion.inspectUtilities.title",
]) {
  const count = [...i18n.matchAll(new RegExp(`"${key}"`, "g"))].length;
  if (count < 2) {
    failures.push(`Project operating center i18n key is not translated in both locales: ${key}`);
  }
}

if (!packageJson.scripts?.build?.includes("check:project-home")) {
  failures.push("package.json build script must include check:project-home.");
}

if (failures.length > 0) {
  console.error(failures.join("\n\n"));
  process.exit(1);
}

console.log("Project operating center checks passed.");
