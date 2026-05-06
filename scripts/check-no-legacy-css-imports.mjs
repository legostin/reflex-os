import { execSync } from "node:child_process";
import { readFileSync } from "node:fs";

const files = execSync('rg -l "import .*\\\\.css" src', { encoding: "utf8" })
  .split("\n")
  .filter(Boolean);
const failures = [];

for (const file of files) {
  const source = readFileSync(file, "utf8");
  for (const match of source.matchAll(/import\s+["'](.+\.css)["'];/g)) {
    if (file !== "src/main.tsx" || match[1] !== "./styles.css") {
      failures.push(`${file} imports ${match[1]}`);
    }
  }
}

if (failures.length > 0) {
  console.error(`Legacy CSS import check failed:\n- ${failures.join("\n- ")}`);
  process.exit(1);
}

console.log("Legacy CSS import check passed.");
