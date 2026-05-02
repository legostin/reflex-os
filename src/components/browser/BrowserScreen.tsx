import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./browser.css";

interface TabSummary {
  tab_id: string;
  url: string;
  title: string;
}

interface FramePayload {
  tab_id: string;
  jpeg_b64: string;
  metadata?: {
    deviceWidth?: number;
    deviceHeight?: number;
    pageScaleFactor?: number;
    offsetTop?: number;
    scrollOffsetX?: number;
    scrollOffsetY?: number;
    timestamp?: number;
  };
}

const HOME_URL = "about:blank";
const VIEWPORT_W = 1280;
const VIEWPORT_H = 720;

interface TabFrame {
  src: string;
  meta?: FramePayload["metadata"];
}

export interface BrowserScreenProps {
  projectId: string | null;
  projectName: string | null;
  onStartChat: (
    prompt: string,
    tabs: { url: string; title: string }[],
  ) => Promise<void> | void;
}

export function BrowserScreen({
  projectId,
  projectName,
  onStartChat,
}: BrowserScreenProps) {
  const [tabs, setTabs] = useState<TabSummary[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [urlDraft, setUrlDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [framesByTab, setFramesByTab] = useState<Record<string, TabFrame>>({});
  const [chatDraft, setChatDraft] = useState("");
  const [chatSubmitting, setChatSubmitting] = useState(false);
  const initRef = useRef(false);
  const stageRef = useRef<HTMLDivElement>(null);
  const imgRef = useRef<HTMLImageElement>(null);
  const activeIdRef = useRef<string | null>(null);
  useEffect(() => {
    activeIdRef.current = activeId;
  }, [activeId]);
  const frameSrc = activeId ? framesByTab[activeId]?.src ?? "" : "";
  const frameMeta = activeId ? framesByTab[activeId]?.meta : undefined;

  async function refreshTabs() {
    try {
      const list = await invoke<TabSummary[]>("browser_tabs_list");
      setTabs(list);
      if (!list.find((t) => t.tab_id === activeId) && list.length > 0) {
        setActiveId(list[0].tab_id);
      }
    } catch (e) {
      setError(String(e));
    }
  }

  async function bootstrap() {
    setBusy(true);
    setError(null);
    try {
      await invoke("browser_init", { headless: true, projectId });
      const list = await invoke<TabSummary[]>("browser_tabs_list");
      let firstId: string;
      if (list.length === 0) {
        const res = await invoke<{ tab_id: string; url: string }>(
          "browser_tab_open",
          { url: HOME_URL },
        );
        firstId = res.tab_id;
      } else {
        firstId = list[0].tab_id;
      }
      await invoke("browser_set_viewport", {
        tabId: firstId,
        width: VIEWPORT_W,
        height: VIEWPORT_H,
      });
      setActiveId(firstId);
      await invoke("browser_screencast_start", {
        tabId: firstId,
        quality: 60,
        maxWidth: VIEWPORT_W,
        maxHeight: VIEWPORT_H,
        everyNthFrame: 1,
      });
      await refreshTabs();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function submitChat() {
    const prompt = chatDraft.trim();
    if (!prompt || chatSubmitting) return;
    if (!projectId) {
      setError("Активный проект не выбран — выбери проект в шапке.");
      return;
    }
    setChatSubmitting(true);
    setError(null);
    try {
      const list = await invoke<TabSummary[]>("browser_tabs_list");
      const snapshot = list.map((t) => ({
        url: t.url,
        title: t.title,
      }));
      await onStartChat(prompt, snapshot);
      setChatDraft("");
    } catch (e) {
      setError(String(e));
    } finally {
      setChatSubmitting(false);
    }
  }

  useEffect(() => {
    if (initRef.current) return;
    initRef.current = true;
    void bootstrap();
  }, []);

  useEffect(() => {
    setFramesByTab((prev) => {
      const ids = new Set(tabs.map((t) => t.tab_id));
      const next: Record<string, TabFrame> = {};
      for (const [k, v] of Object.entries(prev)) {
        if (ids.has(k)) next[k] = v;
      }
      return next;
    });
  }, [tabs]);

  useEffect(() => {
    const subs: Promise<() => void>[] = [];
    subs.push(
      listen<{ tab_id: string; url: string }>(
        "reflex://browser/tabs-opened",
        (ev) => {
          void refreshTabs();
          const payload = ev.payload;
          if (!payload?.tab_id) return;
          if (payload.url && payload.url !== HOME_URL) {
            void switchActive(payload.tab_id);
          }
        },
      ),
    );
    subs.push(
      listen("reflex://browser/tabs-closed", () => void refreshTabs()),
    );
    subs.push(
      listen<{ tab_id: string; url: string }>(
        "reflex://browser/tabs-navigated",
        (ev) => {
          if (!ev.payload?.tab_id) return;
          setTabs((prev) =>
            prev.map((t) =>
              t.tab_id === ev.payload.tab_id
                ? { ...t, url: ev.payload.url }
                : t,
            ),
          );
          setUrlDraft((cur) =>
            ev.payload.tab_id === activeId ? ev.payload.url : cur,
          );
        },
      ),
    );
    subs.push(
      listen<FramePayload>(
        "reflex://browser/screencast-frame",
        (ev) => {
          if (!ev.payload) return;
          setFramesByTab((prev) => ({
            ...prev,
            [ev.payload.tab_id]: {
              src: `data:image/jpeg;base64,${ev.payload.jpeg_b64}`,
              meta: ev.payload.metadata ?? prev[ev.payload.tab_id]?.meta,
            },
          }));
        },
      ),
    );
    return () => {
      subs.forEach((p) => p.then((u) => u()));
    };
  }, []);

  async function switchActive(id: string) {
    const prev = activeIdRef.current;
    if (id === prev) return;
    if (prev) {
      try {
        await invoke("browser_screencast_stop", { tabId: prev });
      } catch {}
    }
    setActiveId(id);
    activeIdRef.current = id;
    try {
      await invoke("browser_set_active_tab", { tabId: id });
      await invoke("browser_set_viewport", {
        tabId: id,
        width: VIEWPORT_W,
        height: VIEWPORT_H,
      });
      await invoke("browser_screencast_start", {
        tabId: id,
        quality: 60,
        maxWidth: VIEWPORT_W,
        maxHeight: VIEWPORT_H,
        everyNthFrame: 1,
      });
      const r = await invoke<{ url: string }>("browser_current_url", {
        tabId: id,
      });
      setUrlDraft(r.url);
    } catch (e) {
      setError(String(e));
    }
  }

  async function newTab() {
    setBusy(true);
    setError(null);
    try {
      if (activeId) {
        try {
          await invoke("browser_screencast_stop", { tabId: activeId });
        } catch {}
      }
      const res = await invoke<{ tab_id: string }>("browser_tab_open", {
        url: HOME_URL,
      });
      await invoke("browser_set_viewport", {
        tabId: res.tab_id,
        width: VIEWPORT_W,
        height: VIEWPORT_H,
      });
        setActiveId(res.tab_id);
      await invoke("browser_screencast_start", {
        tabId: res.tab_id,
        quality: 60,
        maxWidth: VIEWPORT_W,
        maxHeight: VIEWPORT_H,
        everyNthFrame: 1,
      });
      await refreshTabs();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function closeTab(id: string) {
    setBusy(true);
    setError(null);
    try {
      try {
        await invoke("browser_screencast_stop", { tabId: id });
      } catch {}
      await invoke("browser_tab_close", { tabId: id });
      await refreshTabs();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function go() {
    if (!activeId) return;
    let url = urlDraft.trim();
    if (!url) return;
    if (!/^[a-z]+:\/\//i.test(url) && !url.startsWith("about:")) {
      if (url.includes(".") && !url.includes(" ")) {
        url = `https://${url}`;
      } else {
        url = `https://www.google.com/search?q=${encodeURIComponent(url)}`;
      }
    }
    setBusy(true);
    setError(null);
    try {
      await invoke("browser_navigate", { tabId: activeId, url });
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function back() {
    if (!activeId) return;
    try {
      await invoke("browser_back", { tabId: activeId });
    } catch (e) {
      setError(String(e));
    }
  }

  async function forward() {
    if (!activeId) return;
    try {
      await invoke("browser_forward", { tabId: activeId });
    } catch (e) {
      setError(String(e));
    }
  }

  async function reload() {
    if (!activeId) return;
    try {
      await invoke("browser_reload", { tabId: activeId });
    } catch (e) {
      setError(String(e));
    }
  }

  function pageCoords(ev: { clientX: number; clientY: number }): {
    x: number;
    y: number;
  } | null {
    const img = imgRef.current;
    if (!img) return null;
    const rect = img.getBoundingClientRect();
    if (rect.width === 0 || rect.height === 0) return null;
    const xLocal = ev.clientX - rect.left;
    const yLocal = ev.clientY - rect.top;
    const w = frameMeta?.deviceWidth ?? VIEWPORT_W;
    const h = frameMeta?.deviceHeight ?? VIEWPORT_H;
    return {
      x: Math.max(0, Math.min(w, (xLocal / rect.width) * w)),
      y: Math.max(0, Math.min(h, (yLocal / rect.height) * h)),
    };
  }

  async function onStageClick(ev: React.MouseEvent) {
    if (!activeId) return;
    const c = pageCoords(ev);
    if (!c) return;
    stageRef.current?.focus();
    try {
      await invoke("browser_mouse_click", {
        tabId: activeId,
        x: c.x,
        y: c.y,
        button: "left",
        clickCount: ev.detail || 1,
      });
    } catch (e) {
      setError(String(e));
    }
  }

  async function onStageWheel(ev: React.WheelEvent) {
    if (!activeId) return;
    ev.preventDefault();
    try {
      await invoke("browser_mouse_wheel", {
        tabId: activeId,
        dx: ev.deltaX,
        dy: ev.deltaY,
      });
    } catch {}
  }

  const SPECIAL_KEYS = new Set([
    "Enter",
    "Backspace",
    "Tab",
    "Escape",
    "ArrowLeft",
    "ArrowRight",
    "ArrowUp",
    "ArrowDown",
    "Home",
    "End",
    "PageUp",
    "PageDown",
    "Delete",
  ]);

  async function onStageKeyDown(ev: React.KeyboardEvent) {
    if (!activeId) return;
    if (ev.metaKey || ev.ctrlKey || ev.altKey) {
      return;
    }
    if (SPECIAL_KEYS.has(ev.key)) {
      ev.preventDefault();
      try {
        await invoke("browser_keyboard_press", {
          tabId: activeId,
          key: ev.key,
        });
      } catch (e) {
        setError(String(e));
      }
      return;
    }
    if (ev.key.length === 1) {
      ev.preventDefault();
      try {
        await invoke("browser_keyboard_type", {
          tabId: activeId,
          text: ev.key,
        });
      } catch (e) {
        setError(String(e));
      }
    }
  }

  if (!projectId) {
    return (
      <div className="browser-root">
        <div className="browser-empty">
          Чтобы открыть браузер, выбери активный проект в шапке.
        </div>
      </div>
    );
  }

  return (
    <div className="browser-root">
      <header className="browser-header">
        <div className="browser-project-line">
          <span className="browser-project-label">Проект:</span>
          <span className="browser-project-name">
            {projectName ?? projectId}
          </span>
        </div>
        <div className="browser-tab-bar">
          {tabs.map((t) => (
            <button
              key={t.tab_id}
              className={`browser-tab ${activeId === t.tab_id ? "active" : ""}`}
              onClick={() => void switchActive(t.tab_id)}
              title={t.url}
            >
              <span className="browser-tab-title">
                {t.title || t.url || "blank"}
              </span>
              <span
                className="browser-tab-close"
                onClick={(ev) => {
                  ev.stopPropagation();
                  void closeTab(t.tab_id);
                }}
              >
                ✕
              </span>
            </button>
          ))}
          <button
            className="browser-tab browser-tab-new"
            onClick={() => void newTab()}
            disabled={busy}
            title="Новая вкладка"
          >
            +
          </button>
        </div>
        <div className="browser-url-bar">
          <button onClick={() => void back()} disabled={busy} title="Назад">
            ◀
          </button>
          <button
            onClick={() => void forward()}
            disabled={busy}
            title="Вперёд"
          >
            ▶
          </button>
          <button
            onClick={() => void reload()}
            disabled={busy}
            title="Обновить"
          >
            ↻
          </button>
          <input
            className="browser-url-input"
            value={urlDraft}
            placeholder="URL или поисковый запрос"
            onChange={(e) => setUrlDraft(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                void go();
              }
            }}
            disabled={busy}
          />
          <button
            onClick={() => void go()}
            disabled={busy || !urlDraft.trim()}
          >
            Go
          </button>
        </div>
      </header>
      {error && <div className="browser-error">{error}</div>}
      <div
        ref={stageRef}
        className="browser-stage"
        tabIndex={0}
        onClick={onStageClick}
        onWheel={onStageWheel}
        onKeyDown={onStageKeyDown}
      >
        {frameSrc ? (
          <img
            ref={imgRef}
            src={frameSrc}
            alt=""
            className="browser-frame"
            draggable={false}
          />
        ) : (
          <div className="browser-empty">
            {busy ? "Запускаю Chromium…" : "Нет картинки. Открой страницу."}
          </div>
        )}
      </div>
      <footer className="browser-chat-bar">
        <input
          className="browser-chat-input"
          value={chatDraft}
          placeholder={
            tabs.length === 0
              ? "Открой страницу и начни чат с агентом…"
              : `Запустить чат по ${tabs.length} вкладк${tabs.length === 1 ? "е" : "ам"}…`
          }
          onChange={(e) => setChatDraft(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              void submitChat();
            }
          }}
          disabled={chatSubmitting}
        />
        <button
          className="browser-chat-send"
          onClick={() => void submitChat()}
          disabled={chatSubmitting || !chatDraft.trim()}
          title="Создать чат с контекстом текущих вкладок"
        >
          {chatSubmitting ? "…" : "Чат"}
        </button>
      </footer>
    </div>
  );
}

export default BrowserScreen;
