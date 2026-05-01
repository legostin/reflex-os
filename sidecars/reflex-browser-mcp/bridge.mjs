// MCP stdio server that proxies to the long-lived Reflex browser sidecar
// over a Unix socket. Codex spawns one of these per thread when the
// reflex_browser MCP server is enabled in the project config.

import net from "node:net";
import readline from "node:readline";
import { homedir } from "node:os";

const SOCK_PATH =
  process.env.REFLEX_BROWSER_SOCK ||
  `${homedir()}/Library/Application Support/reflex-os/browser/sock`;

const PROTOCOL_VERSION = "2024-11-05";
const SERVER_NAME = "reflex-browser";
const SERVER_VERSION = "0.1.0";

function logErr(...args) {
  console.error("[reflex-browser-mcp]", ...args);
}

function writeMcp(obj) {
  process.stdout.write(JSON.stringify(obj) + "\n");
}

const TOOLS = [
  {
    name: "browser_list_tabs",
    description: "List all open browser tabs (id, url, title).",
    inputSchema: { type: "object", properties: {} },
    sidecar: { method: "tabs.list" },
  },
  {
    name: "browser_open_tab",
    description: "Open a new browser tab. Optionally navigates to url. Returns the new tab_id and becomes the active tab.",
    inputSchema: {
      type: "object",
      properties: {
        url: { type: "string", description: "URL to load (optional)" },
      },
    },
    sidecar: { method: "tabs.open" },
  },
  {
    name: "browser_close_tab",
    description: "Close a browser tab by id.",
    inputSchema: {
      type: "object",
      properties: {
        tab_id: { type: "string" },
      },
      required: ["tab_id"],
    },
    sidecar: { method: "tabs.close" },
  },
  {
    name: "browser_switch_tab",
    description: "Set the active tab — subsequent tools without tab_id will target this one.",
    inputSchema: {
      type: "object",
      properties: { tab_id: { type: "string" } },
      required: ["tab_id"],
    },
    sidecar: { method: "tabs.set_active" },
  },
  {
    name: "browser_navigate",
    description: "Navigate the active (or given) tab to a URL.",
    inputSchema: {
      type: "object",
      properties: {
        tab_id: { type: "string" },
        url: { type: "string" },
        wait_until: {
          type: "string",
          enum: ["load", "domcontentloaded", "networkidle", "commit"],
          description: "Default: domcontentloaded",
        },
      },
      required: ["url"],
    },
    sidecar: { method: "page.navigate" },
  },
  {
    name: "browser_back",
    description: "Go back in history.",
    inputSchema: { type: "object", properties: { tab_id: { type: "string" } } },
    sidecar: { method: "page.back" },
  },
  {
    name: "browser_forward",
    description: "Go forward in history.",
    inputSchema: { type: "object", properties: { tab_id: { type: "string" } } },
    sidecar: { method: "page.forward" },
  },
  {
    name: "browser_reload",
    description: "Reload the current page.",
    inputSchema: { type: "object", properties: { tab_id: { type: "string" } } },
    sidecar: { method: "page.reload" },
  },
  {
    name: "browser_current_url",
    description: "Get current URL and title.",
    inputSchema: { type: "object", properties: { tab_id: { type: "string" } } },
    sidecar: { method: "page.current_url" },
  },
  {
    name: "browser_read_text",
    description: "Return the page's visible innerText (truncated to ~50KB).",
    inputSchema: { type: "object", properties: { tab_id: { type: "string" } } },
    sidecar: { method: "page.read_text" },
  },
  {
    name: "browser_read_outline",
    description: "Return up to 250 interactive/structural elements (headings, links, buttons, inputs) with their tag, role and visible text.",
    inputSchema: { type: "object", properties: { tab_id: { type: "string" } } },
    sidecar: { method: "page.read_outline" },
  },
  {
    name: "browser_click_text",
    description: "Click the first element containing the given text. More robust than CSS selectors.",
    inputSchema: {
      type: "object",
      properties: {
        tab_id: { type: "string" },
        text: { type: "string" },
        exact: { type: "boolean", description: "Default: false (substring match)" },
        timeout: { type: "number", description: "ms; default 5000" },
      },
      required: ["text"],
    },
    sidecar: { method: "page.click_text" },
  },
  {
    name: "browser_click_selector",
    description: "Click an element by CSS selector.",
    inputSchema: {
      type: "object",
      properties: {
        tab_id: { type: "string" },
        selector: { type: "string" },
        timeout: { type: "number" },
      },
      required: ["selector"],
    },
    sidecar: { method: "page.click_selector" },
  },
  {
    name: "browser_fill",
    description: "Fill a form input matched by CSS selector.",
    inputSchema: {
      type: "object",
      properties: {
        tab_id: { type: "string" },
        selector: { type: "string" },
        value: { type: "string" },
        timeout: { type: "number" },
      },
      required: ["selector", "value"],
    },
    sidecar: { method: "page.fill" },
  },
  {
    name: "browser_scroll",
    description: "Scroll the page by pixel deltas (positive dy = down).",
    inputSchema: {
      type: "object",
      properties: {
        tab_id: { type: "string" },
        dx: { type: "number" },
        dy: { type: "number" },
      },
    },
    sidecar: { method: "page.scroll" },
  },
  {
    name: "browser_wait_for",
    description: "Wait until an element matching the CSS selector appears.",
    inputSchema: {
      type: "object",
      properties: {
        tab_id: { type: "string" },
        selector: { type: "string" },
        timeout: { type: "number", description: "ms; default 10000" },
      },
      required: ["selector"],
    },
    sidecar: { method: "page.wait_for" },
  },
  {
    name: "browser_keyboard_press",
    description: "Press a single key (Enter, Tab, Escape, ArrowLeft, etc).",
    inputSchema: {
      type: "object",
      properties: {
        tab_id: { type: "string" },
        key: { type: "string" },
      },
      required: ["key"],
    },
    sidecar: { method: "page.keyboard_press" },
  },
  {
    name: "browser_keyboard_type",
    description: "Type a sequence of characters.",
    inputSchema: {
      type: "object",
      properties: {
        tab_id: { type: "string" },
        text: { type: "string" },
        delay: { type: "number" },
      },
      required: ["text"],
    },
    sidecar: { method: "page.keyboard_type" },
  },
  {
    name: "browser_screenshot",
    description: "Capture a JPEG screenshot of the page. Returns base64 data.",
    inputSchema: {
      type: "object",
      properties: {
        tab_id: { type: "string" },
        full_page: { type: "boolean" },
      },
    },
    sidecar: { method: "page.screenshot" },
    formatResult: (r) =>
      r && r.jpeg_b64
        ? [
            {
              type: "image",
              data: r.jpeg_b64,
              mimeType: "image/jpeg",
            },
          ]
        : null,
  },
];

const TOOLS_BY_NAME = new Map(TOOLS.map((t) => [t.name, t]));

let socket = null;
let nextSidecarId = 0;
const pending = new Map();
let sockBuf = "";

async function ensureSocket() {
  if (socket && !socket.destroyed) return socket;
  return new Promise((resolve, reject) => {
    const s = net.createConnection({ path: SOCK_PATH }, () => {
      socket = s;
      resolve(s);
    });
    s.setEncoding("utf-8");
    s.on("data", (chunk) => {
      sockBuf += chunk;
      let nl;
      while ((nl = sockBuf.indexOf("\n")) >= 0) {
        const line = sockBuf.slice(0, nl);
        sockBuf = sockBuf.slice(nl + 1);
        if (!line.trim()) continue;
        try {
          const msg = JSON.parse(line);
          if (msg.id !== undefined && pending.has(msg.id)) {
            const p = pending.get(msg.id);
            pending.delete(msg.id);
            if (msg.error) p.reject(new Error(msg.error));
            else p.resolve(msg.result);
          }
        } catch (e) {
          logErr("sock parse", e);
        }
      }
    });
    s.on("error", (e) => {
      socket = null;
      reject(e);
      for (const [id, p] of pending) {
        p.reject(e);
        pending.delete(id);
      }
    });
    s.on("close", () => {
      socket = null;
      for (const [id, p] of pending) {
        p.reject(new Error("sidecar socket closed"));
        pending.delete(id);
      }
    });
  });
}

async function sidecarRequest(method, params) {
  const s = await ensureSocket();
  nextSidecarId += 1;
  const id = nextSidecarId;
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    const line = JSON.stringify({ id, method, params: params || {} }) + "\n";
    s.write(line);
    setTimeout(() => {
      if (pending.has(id)) {
        pending.delete(id);
        reject(new Error(`sidecar timeout: ${method}`));
      }
    }, 60_000);
  });
}

function ok(id, result) {
  writeMcp({ jsonrpc: "2.0", id, result });
}

function err(id, code, message) {
  writeMcp({ jsonrpc: "2.0", id, error: { code, message } });
}

async function handleMcp(msg) {
  const { id, method, params } = msg;
  switch (method) {
    case "initialize": {
      ok(id, {
        protocolVersion: PROTOCOL_VERSION,
        capabilities: { tools: {} },
        serverInfo: { name: SERVER_NAME, version: SERVER_VERSION },
      });
      return;
    }
    case "notifications/initialized":
      return;
    case "tools/list": {
      const tools = TOOLS.map((t) => ({
        name: t.name,
        description: t.description,
        inputSchema: t.inputSchema,
      }));
      ok(id, { tools });
      return;
    }
    case "tools/call": {
      const name = params?.name;
      const tool = TOOLS_BY_NAME.get(name);
      if (!tool) {
        err(id, -32601, `tool not found: ${name}`);
        return;
      }
      try {
        const result = await sidecarRequest(
          tool.sidecar.method,
          params?.arguments ?? {},
        );
        const content = tool.formatResult
          ? tool.formatResult(result)
          : [
              {
                type: "text",
                text:
                  result === undefined
                    ? "ok"
                    : typeof result === "string"
                      ? result
                      : JSON.stringify(result, null, 2),
              },
            ];
        ok(id, {
          content: content || [
            { type: "text", text: JSON.stringify(result, null, 2) },
          ],
        });
      } catch (e) {
        ok(id, {
          isError: true,
          content: [
            {
              type: "text",
              text: `error: ${e?.message || e}`,
            },
          ],
        });
      }
      return;
    }
    case "ping":
      ok(id, {});
      return;
    default:
      err(id, -32601, `method not implemented: ${method}`);
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
  Promise.resolve(handleMcp(msg)).catch((e) => {
    if (msg && msg.id !== undefined) err(msg.id, -32603, String(e));
    logErr("handle", e);
  });
});

process.on("SIGTERM", () => process.exit(0));
process.on("SIGINT", () => process.exit(0));
