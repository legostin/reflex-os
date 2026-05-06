import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

const root = process.cwd();
const read = (path) => readFileSync(join(root, path), "utf8");
const failures = [];

const pkg = JSON.parse(read("package.json"));
const vite = read("vite.config.ts");
const main = read("src/main.tsx");
const app = read("src/App.tsx");
const stylesPath = "src/styles.css";
const styles = existsSync(join(root, stylesPath)) ? read(stylesPath) : "";

if (!pkg.devDependencies?.tailwindcss) failures.push("tailwindcss missing from devDependencies");
if (!pkg.devDependencies?.["@tailwindcss/vite"]) failures.push("@tailwindcss/vite missing from devDependencies");
if (!pkg.scripts?.["check:tailwind"]) failures.push("check:tailwind script missing");
if (!vite.includes('import tailwindcss from "@tailwindcss/vite"')) failures.push("vite config must import @tailwindcss/vite");
if (!vite.includes("tailwindcss()")) failures.push("vite plugins must include tailwindcss()");
if (!main.includes('import "./styles.css";')) failures.push("main.tsx must import src/styles.css");
if (app.includes('import "./App.css";')) failures.push("App.tsx must not import App.css");
if (!styles.includes('@import "tailwindcss";')) failures.push("src/styles.css must import tailwindcss");
if (!styles.includes("--color-reflex-bg")) failures.push("src/styles.css must define Reflex theme tokens");

if (failures.length > 0) {
  console.error(`Tailwind setup check failed:\n- ${failures.join("\n- ")}`);
  process.exit(1);
}

console.log("Tailwind setup check passed.");
