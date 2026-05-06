import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

const root = process.cwd();
const path = "src/components/ui/primitives.tsx";
const failures = [];
const source = existsSync(join(root, path)) ? readFileSync(join(root, path), "utf8") : "";

for (const name of ["cx", "Button", "IconButton", "Badge", "Panel", "TextInput", "ModalFrame"]) {
  if (!source.includes(`export function ${name}`) && !source.includes(`export const ${name}`)) {
    failures.push(`${name} missing from ${path}`);
  }
}
if (!source.includes("focus-visible:outline-none")) failures.push("focus-visible styling missing");
if (!source.includes("rounded-md")) failures.push("8px-or-less radius baseline missing");

if (failures.length > 0) {
  console.error(`UI primitive check failed:\n- ${failures.join("\n- ")}`);
  process.exit(1);
}

console.log("UI primitive check passed.");
