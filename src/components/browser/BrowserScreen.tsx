import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./browser.css";

interface TabSummary {
  tab_id: string;
  url: string;
  title: string;
}

const HOME_URL = "about:blank";

export function BrowserScreen() {
  const [tabs, setTabs] = useState<TabSummary[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [urlDraft, setUrlDraft] = useState("");
  const [snapshot, setSnapshot] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const initRef = useRef(false);

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

  async function openInitialTab() {
    setBusy(true);
    setError(null);
    try {
      await invoke("browser_init", { headless: false });
      const res = await invoke<{ tab_id: string; url: string }>(
        "browser_tab_open",
        { url: HOME_URL },
      );
      setActiveId(res.tab_id);
      await refreshTabs();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    if (initRef.current) return;
    initRef.current = true;
    void openInitialTab();
  }, []);

  useEffect(() => {
    const subs: Promise<() => void>[] = [];
    subs.push(
      listen("reflex://browser/tabs.opened", () => void refreshTabs()),
    );
    subs.push(
      listen("reflex://browser/tabs.closed", () => void refreshTabs()),
    );
    subs.push(
      listen<{ tab_id: string; url: string }>(
        "reflex://browser/tabs.navigated",
        (ev) => {
          if (!ev.payload?.tab_id) return;
          setTabs((prev) =>
            prev.map((t) =>
              t.tab_id === ev.payload.tab_id
                ? { ...t, url: ev.payload.url }
                : t,
            ),
          );
          if (ev.payload.tab_id === activeId) {
            setUrlDraft(ev.payload.url);
          }
        },
      ),
    );
    return () => {
      subs.forEach((p) => p.then((u) => u()));
    };
  }, [activeId]);

  useEffect(() => {
    if (!activeId) return;
    let alive = true;
    void (async () => {
      try {
        const r = await invoke<{ url: string; title: string }>(
          "browser_current_url",
          { tabId: activeId },
        );
        if (alive) setUrlDraft(r.url);
      } catch {}
    })();
    return () => {
      alive = false;
    };
  }, [activeId]);

  async function newTab() {
    setBusy(true);
    setError(null);
    try {
      const res = await invoke<{ tab_id: string }>("browser_tab_open", {
        url: HOME_URL,
      });
      setActiveId(res.tab_id);
      await refreshTabs();
      setSnapshot("");
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
      await loadSnapshot(activeId);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function back() {
    if (!activeId) return;
    setBusy(true);
    try {
      await invoke("browser_back", { tabId: activeId });
      await loadSnapshot(activeId);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function forward() {
    if (!activeId) return;
    setBusy(true);
    try {
      await invoke("browser_forward", { tabId: activeId });
      await loadSnapshot(activeId);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function reload() {
    if (!activeId) return;
    setBusy(true);
    try {
      await invoke("browser_reload", { tabId: activeId });
      await loadSnapshot(activeId);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function loadSnapshot(id: string) {
    setLoading(true);
    try {
      const r = await invoke<{ text: string }>("browser_read_text", {
        tabId: id,
      });
      setSnapshot(r.text || "");
    } catch (e) {
      setSnapshot("");
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="browser-root">
      <header className="browser-header">
        <div className="browser-tab-bar">
          {tabs.map((t) => (
            <button
              key={t.tab_id}
              className={`browser-tab ${activeId === t.tab_id ? "active" : ""}`}
              onClick={() => {
                setActiveId(t.tab_id);
                setUrlDraft(t.url);
              }}
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
      <main className="browser-main">
        <div className="browser-snapshot-toolbar">
          <button
            onClick={() => activeId && void loadSnapshot(activeId)}
            disabled={!activeId || loading}
          >
            {loading ? "Читаю…" : "Обновить snapshot"}
          </button>
          <span className="browser-hint">
            Шаг 1: Chromium открыт отдельным окном (headed). На шаге 2 он
            переедет в эту панель.
          </span>
        </div>
        {snapshot ? (
          <pre className="browser-snapshot">{snapshot}</pre>
        ) : (
          <div className="browser-empty">
            Открой страницу через URL bar, потом нажми «Обновить snapshot».
          </div>
        )}
      </main>
    </div>
  );
}

export default BrowserScreen;
