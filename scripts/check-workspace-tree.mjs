import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

const root = process.cwd();
const read = (path) => readFileSync(join(root, path), "utf8");
const failures = [];

const typesPath = "src/components/workspace/workspaceTypes.ts";
const treePath = "src/components/workspace/navTree.ts";
const types = existsSync(join(root, typesPath)) ? read(typesPath) : "";
const tree = existsSync(join(root, treePath)) ? read(treePath) : "";
const chat = read("src/components/ChatThread.tsx");

for (const name of ["Route", "Project", "ProjectFolder", "Thread", "AppManifest", "AppFolder"]) {
  if (!types.includes(`export type ${name}`)) failures.push(`${name} not exported from workspaceTypes.ts`);
}
for (const token of ["WorkspaceTreeNode", "buildWorkspaceTree", "sectionNodes", "projects", "utilities", "topics", "files"]) {
  if (!tree.includes(token)) failures.push(`${token} missing from navTree.ts`);
}
if (!chat.includes('from "./workspace/workspaceTypes"')) failures.push("ChatThread must import shared workspace types");

if (failures.length > 0) {
  console.error(`Workspace tree check failed:\n- ${failures.join("\n- ")}`);
  process.exit(1);
}

console.log("Workspace tree check passed.");
