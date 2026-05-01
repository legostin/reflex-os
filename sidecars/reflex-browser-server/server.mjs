import { chromium } from "playwright";
import { mkdirSync, existsSync, writeFileSync, readFileSync } from "node:fs";
import { dirname } from "node:path";
import { homedir } from "node:os";
import readline from "node:readline";

const STATE_PATH =
  process.env.REFLEX_BROWSER_STATE ||
  `${homedir()}/Library/Application Support/reflex-os/browser/storageState.json`;

let browser = null;
let context = null;
let headlessMode = false;
const tabs = new Map();
let nextTabSeq = 0;

function send(obj) {
  process.stdout.write(JSON.stringify(obj) + "\n");
}

function sendEvent(event, params) {
  send({ event, params });
}

function logErr(...args) {
  console.error("[browser-sidecar]", ...args);
}

async function ensureBrowser({ headless = true } = {}) {
  if (browser && headless === headlessMode) return;
  if (browser) {
    try {
      await saveState();
      await browser.close();
    } catch (e) {
      logErr("close on relaunch", e);
    }
    browser = null;
    context = null;
    tabs.clear();
  }
  mkdirSync(dirname(STATE_PATH), { recursive: true });
  let storageState;
  if (existsSync(STATE_PATH)) {
    try {
      storageState = JSON.parse(readFileSync(STATE_PATH, "utf-8"));
    } catch (e) {
      logErr("storageState read failed", e);
    }
  }
  browser = await chromium.launch({ headless });
  headlessMode = headless;
  context = await browser.newContext({
    storageState,
    viewport: { width: 1280, height: 720 },
  });
  context.on("page", (page) => {
    if ([...tabs.values()].includes(page)) return;
    const id = makeTabId();
    tabs.set(id, page);
    attachPage(id, page);
    sendEvent("tabs.opened", { tab_id: id, url: page.url() });
  });
  sendEvent("browser.ready", { headless });
}

function makeTabId() {
  nextTabSeq += 1;
  return `t_${Date.now().toString(36)}_${nextTabSeq}`;
}

function attachPage(id, page) {
  page.on("framenavigated", (frame) => {
    if (frame === page.mainFrame()) {
      sendEvent("tabs.navigated", { tab_id: id, url: page.url() });
    }
  });
  page.on("close", () => {
    tabs.delete(id);
    sendEvent("tabs.closed", { tab_id: id });
  });
}

async function saveState() {
  if (!context) return;
  try {
    const state = await context.storageState();
    writeFileSync(STATE_PATH, JSON.stringify(state));
  } catch (e) {
    logErr("saveState err", e);
  }
}

function getTab(tabId) {
  const p = tabs.get(tabId);
  if (!p) {
    throw new Error(`tab not found: ${tabId}`);
  }
  return p;
}

async function summarizeTabs() {
  const out = [];
  for (const [id, p] of tabs) {
    let title = "";
    try {
      title = await p.title();
    } catch {}
    out.push({ tab_id: id, url: p.url(), title });
  }
  return out;
}

async function handle(msg) {
  const { id, method, params = {} } = msg;
  if (id === undefined) return;
  try {
    let result;
    switch (method) {
      case "browser.init": {
        await ensureBrowser({ headless: params.headless ?? false });
        result = { ok: true, headless: headlessMode };
        break;
      }
      case "browser.shutdown": {
        await saveState();
        if (browser) await browser.close();
        browser = null;
        context = null;
        tabs.clear();
        result = { ok: true };
        break;
      }
      case "tabs.list": {
        result = await summarizeTabs();
        break;
      }
      case "tabs.open": {
        await ensureBrowser({ headless: headlessMode });
        const page = await context.newPage();
        const tabId = makeTabId();
        tabs.set(tabId, page);
        attachPage(tabId, page);
        if (params.url) {
          try {
            await page.goto(params.url, {
              waitUntil: "domcontentloaded",
              timeout: params.timeout ?? 30000,
            });
          } catch (e) {
            logErr("initial nav failed", e);
          }
        }
        result = { tab_id: tabId, url: page.url() };
        break;
      }
      case "tabs.close": {
        const p = getTab(params.tab_id);
        await p.close();
        tabs.delete(params.tab_id);
        result = { ok: true };
        break;
      }
      case "page.navigate": {
        const p = getTab(params.tab_id);
        await p.goto(params.url, {
          waitUntil: params.wait_until ?? "domcontentloaded",
          timeout: params.timeout ?? 30000,
        });
        result = { url: p.url() };
        break;
      }
      case "page.back": {
        const p = getTab(params.tab_id);
        await p.goBack({ waitUntil: "domcontentloaded" });
        result = { url: p.url() };
        break;
      }
      case "page.forward": {
        const p = getTab(params.tab_id);
        await p.goForward({ waitUntil: "domcontentloaded" });
        result = { url: p.url() };
        break;
      }
      case "page.reload": {
        const p = getTab(params.tab_id);
        await p.reload({ waitUntil: "domcontentloaded" });
        result = { url: p.url() };
        break;
      }
      case "page.current_url": {
        const p = getTab(params.tab_id);
        result = { url: p.url(), title: await p.title() };
        break;
      }
      case "page.read_text": {
        const p = getTab(params.tab_id);
        const text = await p.evaluate(() => {
          const t = (document.body && document.body.innerText) || "";
          return t.length > 50000 ? t.slice(0, 50000) : t;
        });
        result = { text };
        break;
      }
      case "page.read_outline": {
        const p = getTab(params.tab_id);
        const outline = await p.evaluate(() => {
          const out = [];
          const sel = "h1,h2,h3,h4,h5,h6,a,button,[role='button'],input,textarea";
          document.querySelectorAll(sel).forEach((el) => {
            const tag = el.tagName.toLowerCase();
            const text = (el.innerText || el.value || el.placeholder || "")
              .toString()
              .trim()
              .replace(/\s+/g, " ")
              .slice(0, 200);
            if (!text) return;
            const role = el.getAttribute("role") || tag;
            out.push({ tag, role, text });
          });
          return out.slice(0, 250);
        });
        result = { outline };
        break;
      }
      case "page.click_text": {
        const p = getTab(params.tab_id);
        const locator = p.getByText(params.text, {
          exact: params.exact ?? false,
        });
        await locator
          .first()
          .click({ timeout: params.timeout ?? 5000 });
        result = { ok: true };
        break;
      }
      case "page.click_selector": {
        const p = getTab(params.tab_id);
        await p.click(params.selector, {
          timeout: params.timeout ?? 5000,
        });
        result = { ok: true };
        break;
      }
      case "page.fill": {
        const p = getTab(params.tab_id);
        await p.fill(params.selector, params.value, {
          timeout: params.timeout ?? 5000,
        });
        result = { ok: true };
        break;
      }
      case "page.scroll": {
        const p = getTab(params.tab_id);
        await p.evaluate(
          ({ dx, dy }) => window.scrollBy(dx || 0, dy || 0),
          { dx: params.dx ?? 0, dy: params.dy ?? 0 },
        );
        result = { ok: true };
        break;
      }
      case "page.wait_for": {
        const p = getTab(params.tab_id);
        await p.waitForSelector(params.selector, {
          timeout: params.timeout ?? 10000,
        });
        result = { ok: true };
        break;
      }
      case "page.screenshot": {
        const p = getTab(params.tab_id);
        const buf = await p.screenshot({
          type: "jpeg",
          quality: params.quality ?? 70,
          fullPage: !!params.full_page,
        });
        result = { jpeg_b64: buf.toString("base64") };
        break;
      }
      case "state.save": {
        await saveState();
        result = { ok: true };
        break;
      }
      default:
        throw new Error(`unknown method: ${method}`);
    }
    send({ id, result });
  } catch (e) {
    send({ id, error: String((e && e.message) || e) });
  }
}

const rl = readline.createInterface({ input: process.stdin });
rl.on("line", (line) => {
  if (!line.trim()) return;
  let msg;
  try {
    msg = JSON.parse(line);
  } catch (e) {
    logErr("parse", e);
    return;
  }
  Promise.resolve(handle(msg)).catch((e) => logErr("handle", e));
});

async function shutdown() {
  try {
    await saveState();
    if (browser) await browser.close();
  } catch (e) {
    logErr("shutdown", e);
  }
  process.exit(0);
}
process.on("SIGTERM", shutdown);
process.on("SIGINT", shutdown);

sendEvent("ready", { pid: process.pid });
