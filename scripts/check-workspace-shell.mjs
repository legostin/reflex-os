import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

const root = process.cwd();
const read = (path) => readFileSync(join(root, path), "utf8");
const failures = [];
const files = [
  "src/components/workspace/WorkspaceShell.tsx",
  "src/components/workspace/WorkspaceSidebar.tsx",
  "src/components/workspace/WorkspaceMain.tsx",
];

for (const file of files) {
  if (!existsSync(join(root, file))) failures.push(`${file} missing`);
}

const shell = existsSync(join(root, files[0])) ? read(files[0]) : "";
const sidebar = existsSync(join(root, files[1])) ? read(files[1]) : "";
const main = existsSync(join(root, files[2])) ? read(files[2]) : "";
const chat = read("src/components/ChatThread.tsx");
const navTree = read("src/components/workspace/navTree.ts");
const paneTabsPath = "src/components/workspace/PaneTabs.tsx";
const paneTabs = existsSync(join(root, paneTabsPath)) ? read(paneTabsPath) : "";

if (!shell.includes("data-tauri-drag-region")) failures.push("WorkspaceShell must preserve drag regions");
if (!sidebar.includes("WorkspaceTreeNode")) failures.push("WorkspaceSidebar must render workspace tree nodes");
if (!main.includes("children")) failures.push("WorkspaceMain must render children");
if (!chat.includes("<WorkspaceShell")) failures.push("ChatThread must render WorkspaceShell");
if (chat.includes("function Header(")) failures.push("old Header function must be removed");
if (!existsSync(join(root, paneTabsPath))) failures.push(`${paneTabsPath} missing`);
if (!paneTabs.includes("export function PaneTabs")) failures.push("PaneTabs export missing");
if (chat.includes('className="tabs-row"')) failures.push("legacy tabs-row usage must be removed");
if (!sidebar.includes("iconForNode")) failures.push("WorkspaceSidebar must render menu item icons");
if (!sidebar.includes("node.icon")) failures.push("WorkspaceSidebar must prefer node icons");
if (!sidebar.includes("iconToneForNode")) failures.push("WorkspaceSidebar must color menu item icons");
if (!sidebar.includes("size-6")) failures.push("WorkspaceSidebar icons must be readable at 24px");
if (!sidebar.includes("text-[13px]")) failures.push("WorkspaceSidebar typography must use airier menu text");
for (const icon of ['icon: "home"', 'icon: "projects"', 'icon: "utilities"', 'icon: "memory"', 'icon: "settings"']) {
  if (!navTree.includes(icon)) failures.push(`navTree missing ${icon}`);
}
if (navTree.includes('id: "sections"')) failures.push("Sections must be first-level items, not nested under a Sections group");
if (!navTree.includes("const sectionNodes")) failures.push("navTree must define first-level section nodes");
if (!navTree.includes("...sectionNodes")) failures.push("navTree must return section nodes at the first level");

if (failures.length > 0) {
  console.error(`Workspace shell check failed:\n- ${failures.join("\n- ")}`);
  process.exit(1);
}

console.log("Workspace shell check passed.");
