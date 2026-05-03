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

function setFromMatches(source, regex, group = 1) {
  return new Set([...source.matchAll(regex)].map((match) => match[group]));
}

function sorted(values) {
  return [...values].sort((a, b) => a.localeCompare(b));
}

function difference(left, right) {
  return sorted(left).filter((value) => !right.has(value));
}

function recordDiff(failures, title, missing) {
  if (missing.length > 0) {
    failures.push(`${title}:\n  - ${missing.join("\n  - ")}`);
  }
}

const catalog = read("src/appBridgeCatalog.ts");
const appsRs = read("src-tauri/src/apps.rs");
const dispatch = read("src-tauri/src/apps_dispatch.rs");
const libRs = read("src-tauri/src/lib.rs");
const readme = read("README.md");

const apiBlock = sliceBetween(
  catalog,
  "export const BRIDGE_API_GROUPS",
  "export const BRIDGE_HELPER_GROUPS",
);
const helperBlock = catalog.slice(
  catalog.indexOf("export const BRIDGE_HELPER_GROUPS"),
);
const readmeOverlayBlock = sliceBetween(
  readme,
  "The injected runtime overlay provides:",
  "## App Bridge API",
);
const readmeApiBlock = sliceBetween(
  readme,
  "## App Bridge API",
  "## Manifest Automation",
);
const dispatchBlock = sliceBetween(dispatch, "match method {", "other => Err");
const promptMethodBlock = sliceBetween(
  libRs,
  "AVAILABLE METHODS:",
  "Permissions for apps.invoke",
);
const promptHelperBlock = sliceBetween(
  libRs,
  "The iframe runtime overlay already provides helpers",
  "Permissions for apps.invoke",
);
const runtimeCatalogMethodBlock = sliceBetween(
  dispatch,
  "let methods = vec![",
  "let helpers = vec![",
);
const runtimeCatalogHelperBlock = sliceBetween(
  dispatch,
  "let helpers = vec![",
  "Ok(serde_json::json!",
);

const catalogMethods = setFromMatches(
  apiBlock,
  /"([a-z][A-Za-z0-9_]*(?:\.[A-Za-z0-9_]+)+)"/g,
);
const readmeMethods = setFromMatches(
  readmeApiBlock,
  /\b([a-z][A-Za-z0-9_]*(?:\.[A-Za-z0-9_]+)+)\b/g,
);
const promptMethods = setFromMatches(
  promptMethodBlock,
  /\b([a-z][A-Za-z0-9_]*(?:\.[A-Za-z0-9_]+)+)\b/g,
);
const catalogHelpers = setFromMatches(helperBlock, /"(reflex[A-Za-z0-9_]+)"/g);
const overlayHelpers = setFromMatches(
  appsRs,
  /window\.(reflex[A-Za-z0-9_]+)\s*=\s*(?:function|reflexInvokeRaw)/g,
);
const readmeHelpers = setFromMatches(
  readmeOverlayBlock,
  /window\.(reflex[A-Za-z0-9_]+)/g,
);
const promptHelpers = setFromMatches(promptHelperBlock, /\b(reflex[A-Za-z0-9_]+)\b/g);
const runtimeCatalogMethods = setFromMatches(
  runtimeCatalogMethodBlock,
  /"([a-z][A-Za-z0-9_]*(?:\.[A-Za-z0-9_]+)+)"/g,
);
const runtimeCatalogHelpers = setFromMatches(
  runtimeCatalogHelperBlock,
  /"(reflex[A-Za-z0-9_]+)"/g,
);

const dispatchMethods = new Set();
for (const arm of dispatchBlock.matchAll(
  /^\s*(?:"[^"]+"\s*(?:\|\s*"[^"]+"\s*)*)=>/gm,
)) {
  for (const method of arm[0].matchAll(/"([^"]+)"/g)) {
    dispatchMethods.add(method[1]);
  }
}

const dispatchAliasesOrInternal = new Set([
  "browser.click_selector",
  "browser.click_text",
  "browser.read_outline",
  "browser.read_text",
  "browser.current_url",
  "browser.set_active",
  "browser.tab.open",
  "browser.tab.close",
  "browser.tabsList",
  "browser.wait_for",
  "clipboard.read_text",
  "clipboard.write_text",
  "apps.commit_partial",
  "events.clearSubscriptions",
  "integration.learn_visible",
  "mcp.list",
  "memory.forget_path",
  "memory.index_path",
  "memory.path_status",
  "memory.path_status_batch",
  "system.open_path",
  "system.open_panel",
  "system.open_url",
  "system.reveal_path",
  "threads.open",
  "scheduler.run_detail",
  "scheduler.run_now",
  "scheduler.set_paused",
  "threads.list",
]);

const publicDispatchMethods = new Set(
  sorted(dispatchMethods).filter((method) => !dispatchAliasesOrInternal.has(method)),
);

const failures = [];

recordDiff(
  failures,
  "Catalog API methods missing from apps_dispatch.rs",
  difference(catalogMethods, dispatchMethods),
);
recordDiff(
  failures,
  "Public dispatch methods missing from src/appBridgeCatalog.ts",
  difference(publicDispatchMethods, catalogMethods),
);
recordDiff(
  failures,
  "Catalog API methods missing from README App Bridge API section",
  difference(catalogMethods, readmeMethods),
);
recordDiff(
  failures,
  "Catalog API methods missing from app creation prompt method docs",
  difference(catalogMethods, promptMethods),
);
recordDiff(
  failures,
  "Catalog API methods missing from bridge.catalog runtime payload",
  difference(catalogMethods, runtimeCatalogMethods),
);
recordDiff(
  failures,
  "bridge.catalog runtime methods missing from src/appBridgeCatalog.ts",
  difference(runtimeCatalogMethods, catalogMethods),
);
recordDiff(
  failures,
  "Catalog helpers missing from runtime overlay",
  difference(catalogHelpers, overlayHelpers),
);
recordDiff(
  failures,
  "Runtime overlay helpers missing from src/appBridgeCatalog.ts",
  difference(overlayHelpers, catalogHelpers),
);
recordDiff(
  failures,
  "Catalog helpers missing from README overlay list",
  difference(catalogHelpers, readmeHelpers),
);
recordDiff(
  failures,
  "README overlay helpers missing from src/appBridgeCatalog.ts",
  difference(readmeHelpers, catalogHelpers),
);
recordDiff(
  failures,
  "Catalog helpers missing from app creation prompt",
  difference(catalogHelpers, promptHelpers),
);
recordDiff(
  failures,
  "App creation prompt helpers missing from src/appBridgeCatalog.ts",
  difference(promptHelpers, catalogHelpers),
);
recordDiff(
  failures,
  "Catalog helpers missing from bridge.catalog runtime payload",
  difference(catalogHelpers, runtimeCatalogHelpers),
);
recordDiff(
  failures,
  "bridge.catalog runtime helpers missing from src/appBridgeCatalog.ts",
  difference(runtimeCatalogHelpers, catalogHelpers),
);

if (failures.length > 0) {
  console.error(failures.join("\n\n"));
  process.exit(1);
}

console.log(
  `Bridge catalog check passed (${catalogMethods.size} methods, ${catalogHelpers.size} helpers).`,
);
