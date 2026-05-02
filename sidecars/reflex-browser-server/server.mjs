import { chromium } from "playwright";
import { mkdirSync, existsSync, writeFileSync, readFileSync, unlinkSync } from "node:fs";
import { dirname } from "node:path";
import { homedir } from "node:os";
import readline from "node:readline";
import net from "node:net";

const STATE_PATH =
  process.env.REFLEX_BROWSER_STATE ||
  `${homedir()}/Library/Application Support/reflex-os/browser/storageState.json`;
const SOCK_PATH =
  process.env.REFLEX_BROWSER_SOCK ||
  `${homedir()}/Library/Application Support/reflex-os/browser/sock`;

let browser = null;
let context = null;
let headlessMode = false;
const tabs = new Map();
let nextTabSeq = 0;
const screencasts = new Map();
let activeTabId = null;

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
    void stopScreencast(id);
    sendEvent("tabs.closed", { tab_id: id });
  });
}

async function startScreencast(tabId, opts = {}) {
  const page = getTab(tabId);
  if (screencasts.has(tabId)) return { ok: true, already: true };
  const cdp = await page.context().newCDPSession(page);
  let frameCount = 0;
  cdp.on("Page.screencastFrame", async (frame) => {
    frameCount += 1;
    if (frameCount <= 2) {
      logErr(`screencast frame #${frameCount} for ${tabId} (${frame.data.length} b64 chars)`);
    }
    sendEvent("screencast.frame", {
      tab_id: tabId,
      jpeg_b64: frame.data,
      metadata: frame.metadata,
    });
    try {
      await cdp.send("Page.screencastFrameAck", {
        sessionId: frame.sessionId,
      });
    } catch {}
  });
  await cdp.send("Page.startScreencast", {
    format: opts.format ?? "jpeg",
    quality: opts.quality ?? 60,
    maxWidth: opts.max_width ?? 1280,
    maxHeight: opts.max_height ?? 720,
    everyNthFrame: opts.every_nth_frame ?? 1,
  });
  screencasts.set(tabId, cdp);
  void pushManualFrame(tabId).catch(() => {});
  return { ok: true };
}

async function pushManualFrame(tabId) {
  const page = tabs.get(tabId);
  if (!page) return;
  try {
    const buf = await page.screenshot({ type: "jpeg", quality: 60 });
    sendEvent("screencast.frame", {
      tab_id: tabId,
      jpeg_b64: buf.toString("base64"),
      metadata: {
        deviceWidth: 1280,
        deviceHeight: 720,
        pageScaleFactor: 1,
      },
    });
  } catch (e) {
    logErr("manual frame failed", e?.message || e);
  }
}

async function stopScreencast(tabId) {
  const cdp = screencasts.get(tabId);
  if (!cdp) return { ok: true, already: true };
  try {
    await cdp.send("Page.stopScreencast");
  } catch {}
  try {
    await cdp.detach();
  } catch {}
  screencasts.delete(tabId);
  return { ok: true };
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

function resolveTabId(tab_id) {
  if (tab_id) return tab_id;
  if (activeTabId && tabs.has(activeTabId)) return activeTabId;
  const first = tabs.keys().next().value;
  return first ?? null;
}

async function handle(msg, sendFn) {
  const { id, method, params = {} } = msg;
  if (id === undefined) return;
  if (params && params.tab_id === undefined && tabHungryMethods.has(method)) {
    const fallback = resolveTabId(undefined);
    if (fallback) params.tab_id = fallback;
  }
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
        activeTabId = tabId;
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
        if (activeTabId === params.tab_id) {
          activeTabId = tabs.keys().next().value ?? null;
        }
        result = { ok: true };
        break;
      }
      case "tabs.set_active": {
        if (!tabs.has(params.tab_id)) {
          throw new Error(`tab not found: ${params.tab_id}`);
        }
        activeTabId = params.tab_id;
        result = { ok: true, tab_id: activeTabId };
        break;
      }
      case "tabs.get_active": {
        result = { tab_id: activeTabId };
        break;
      }
      case "page.navigate": {
        const p = getTab(params.tab_id);
        await p.goto(params.url, {
          waitUntil: params.wait_until ?? "domcontentloaded",
          timeout: params.timeout ?? 30000,
        });
        result = { url: p.url() };
        if (screencasts.has(params.tab_id)) {
          void pushManualFrame(params.tab_id).catch(() => {});
        }
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
      case "log.push": {
        const tag = params.source || "browser-sidecar";
        const lvl = params.level ? `[${params.level}]` : "";
        logErr(`[${tag}]${lvl} ${params.message}`);
        result = { ok: true };
        break;
      }
      case "screencast.start": {
        result = await startScreencast(params.tab_id, params);
        break;
      }
      case "screencast.stop": {
        result = await stopScreencast(params.tab_id);
        break;
      }
      case "page.set_viewport": {
        const p = getTab(params.tab_id);
        await p.setViewportSize({
          width: params.width ?? 1280,
          height: params.height ?? 720,
        });
        result = { ok: true };
        break;
      }
      case "page.mouse_move": {
        const p = getTab(params.tab_id);
        await p.mouse.move(params.x, params.y, {
          steps: params.steps ?? 1,
        });
        result = { ok: true };
        break;
      }
      case "page.mouse_down": {
        const p = getTab(params.tab_id);
        await p.mouse.down({
          button: params.button ?? "left",
          clickCount: params.click_count ?? 1,
        });
        result = { ok: true };
        break;
      }
      case "page.mouse_up": {
        const p = getTab(params.tab_id);
        await p.mouse.up({
          button: params.button ?? "left",
          clickCount: params.click_count ?? 1,
        });
        result = { ok: true };
        break;
      }
      case "page.mouse_click": {
        const p = getTab(params.tab_id);
        await p.mouse.click(params.x, params.y, {
          button: params.button ?? "left",
          clickCount: params.click_count ?? 1,
          delay: params.delay ?? 0,
        });
        result = { ok: true };
        break;
      }
      case "page.mouse_wheel": {
        const p = getTab(params.tab_id);
        await p.mouse.wheel(params.dx ?? 0, params.dy ?? 0);
        result = { ok: true };
        break;
      }
      case "page.keyboard_type": {
        const p = getTab(params.tab_id);
        await p.keyboard.type(params.text, { delay: params.delay ?? 0 });
        result = { ok: true };
        break;
      }
      case "page.keyboard_press": {
        const p = getTab(params.tab_id);
        await p.keyboard.press(params.key, { delay: params.delay ?? 0 });
        result = { ok: true };
        break;
      }
      default:
        throw new Error(`unknown method: ${method}`);
    }
    sendFn({ id, result });
  } catch (e) {
    sendFn({ id, error: String((e && e.message) || e) });
  }
}

const tabHungryMethods = new Set([
  "page.navigate",
  "page.back",
  "page.forward",
  "page.reload",
  "page.current_url",
  "page.read_text",
  "page.read_outline",
  "page.click_text",
  "page.click_selector",
  "page.fill",
  "page.scroll",
  "page.wait_for",
  "page.screenshot",
  "page.set_viewport",
  "page.mouse_move",
  "page.mouse_down",
  "page.mouse_up",
  "page.mouse_click",
  "page.mouse_wheel",
  "page.keyboard_type",
  "page.keyboard_press",
  "screencast.start",
  "screencast.stop",
  "tabs.close",
]);

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
  Promise.resolve(handle(msg, send)).catch((e) => logErr("handle", e));
});

mkdirSync(dirname(SOCK_PATH), { recursive: true });
try {
  unlinkSync(SOCK_PATH);
} catch {}
const sockServer = net.createServer((socket) => {
  let buf = "";
  const sendToSock = (obj) => {
    try {
      socket.write(JSON.stringify(obj) + "\n");
    } catch {}
  };
  socket.on("data", (chunk) => {
    buf += chunk.toString("utf-8");
    let nl;
    while ((nl = buf.indexOf("\n")) >= 0) {
      const line = buf.slice(0, nl);
      buf = buf.slice(nl + 1);
      if (!line.trim()) continue;
      let msg;
      try {
        msg = JSON.parse(line);
      } catch (e) {
        logErr("sock parse", e);
        continue;
      }
      Promise.resolve(handle(msg, sendToSock)).catch((e) =>
        logErr("sock handle", e),
      );
    }
  });
  socket.on("error", () => {});
});
sockServer.listen(SOCK_PATH, () => {
  logErr(`mcp socket listening at ${SOCK_PATH}`);
});

async function shutdown() {
  try {
    for (const id of [...screencasts.keys()]) {
      await stopScreencast(id);
    }
    await saveState();
    if (browser) await browser.close();
    try {
      sockServer.close();
    } catch {}
    try {
      unlinkSync(SOCK_PATH);
    } catch {}
  } catch (e) {
    logErr("shutdown", e);
  }
  process.exit(0);
}
process.on("SIGTERM", shutdown);
process.on("SIGINT", shutdown);

sendEvent("ready", { pid: process.pid });
