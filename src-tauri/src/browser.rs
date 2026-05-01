use crate::logs::{self, LogLevel};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::{oneshot, Mutex};
use tokio::time::{timeout, Duration};

const REQUEST_TIMEOUT_SECS: u64 = 60;

#[derive(Default)]
pub struct BrowserSidecar {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Default)]
struct Inner {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    pending: HashMap<u64, oneshot::Sender<Result<Value, String>>>,
    next_id: u64,
}

impl BrowserSidecar {
    pub async fn ensure_started(&self, app: &AppHandle) -> Result<(), String> {
        {
            let inner = self.inner.lock().await;
            if inner.child.is_some() {
                return Ok(());
            }
        }
        let script = sidecar_script_path(app)?;
        let working = script
            .parent()
            .ok_or_else(|| "sidecar script has no parent".to_string())?
            .to_path_buf();
        let state_path = browser_state_path(app)?;
        let node_bin = resolve_node_binary()?;

        eprintln!(
            "[browser] spawning sidecar: {} {}",
            node_bin,
            script.display()
        );

        let mut child = Command::new(&node_bin)
            .arg(&script)
            .current_dir(&working)
            .env("REFLEX_BROWSER_STATE", &state_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("spawn node sidecar: {e}"))?;

        let stdin = child.stdin.take().ok_or("no sidecar stdin")?;
        let stdout = child.stdout.take().ok_or("no sidecar stdout")?;
        let stderr = child.stderr.take().ok_or("no sidecar stderr")?;

        {
            let mut inner = self.inner.lock().await;
            inner.stdin = Some(stdin);
            inner.child = Some(child);
        }

        logs::log_with(app, LogLevel::Info, "browser", "sidecar spawned");

        let inner_arc = self.inner.clone();
        let app_for_events = app.clone();
        tauri::async_runtime::spawn(async move {
            read_loop(inner_arc, stdout, app_for_events).await;
        });
        let app_for_stderr = app.clone();
        tauri::async_runtime::spawn(async move {
            stderr_loop(stderr, app_for_stderr).await;
        });
        Ok(())
    }

    pub async fn request(
        &self,
        app: &AppHandle,
        method: &str,
        params: Value,
    ) -> Result<Value, String> {
        self.ensure_started(app).await?;

        let (id, line) = {
            let mut inner = self.inner.lock().await;
            inner.next_id += 1;
            let id = inner.next_id;
            let line = serde_json::to_string(&serde_json::json!({
                "id": id,
                "method": method,
                "params": params,
            }))
            .map_err(|e| e.to_string())?;
            (id, line)
        };

        let (tx, rx) = oneshot::channel();
        {
            let mut inner = self.inner.lock().await;
            inner.pending.insert(id, tx);
            let stdin = inner
                .stdin
                .as_mut()
                .ok_or_else(|| "sidecar stdin gone".to_string())?;
            stdin
                .write_all(line.as_bytes())
                .await
                .map_err(|e| format!("write line: {e}"))?;
            stdin
                .write_all(b"\n")
                .await
                .map_err(|e| format!("write nl: {e}"))?;
            stdin.flush().await.map_err(|e| format!("flush: {e}"))?;
        }

        match timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS), rx).await {
            Ok(Ok(res)) => res,
            Ok(Err(_)) => {
                self.cleanup_pending(id).await;
                Err("sidecar channel closed".into())
            }
            Err(_) => {
                self.cleanup_pending(id).await;
                Err(format!("sidecar request timeout: {method}"))
            }
        }
    }

    async fn cleanup_pending(&self, id: u64) {
        let mut inner = self.inner.lock().await;
        inner.pending.remove(&id);
    }
}

async fn read_loop(
    inner: Arc<Mutex<Inner>>,
    stdout: ChildStdout,
    app: AppHandle,
) {
    let mut lines = BufReader::new(stdout).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                let v: Value = match serde_json::from_str(&line) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if let Some(id) = v.get("id").and_then(|x| x.as_u64()) {
                    let mut g = inner.lock().await;
                    if let Some(tx) = g.pending.remove(&id) {
                        let res = if let Some(err) = v.get("error") {
                            Err(err
                                .as_str()
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| err.to_string()))
                        } else {
                            Ok(v.get("result").cloned().unwrap_or(Value::Null))
                        };
                        let _ = tx.send(res);
                    }
                } else if let Some(event) = v.get("event").and_then(|x| x.as_str()) {
                    let payload = v.get("params").cloned().unwrap_or(Value::Null);
                    let event_name = format!("reflex://browser/{event}");
                    if let Err(e) = app.emit(&event_name, &payload) {
                        eprintln!("[browser] emit {event_name} failed: {e}");
                    }
                }
            }
            Ok(None) => {
                eprintln!("[browser] sidecar stdout closed");
                let mut g = inner.lock().await;
                g.child = None;
                g.stdin = None;
                for (_, tx) in g.pending.drain() {
                    let _ = tx.send(Err("sidecar exited".into()));
                }
                break;
            }
            Err(e) => {
                eprintln!("[browser] sidecar read err: {e}");
                break;
            }
        }
    }
}

async fn stderr_loop(stderr: ChildStderr, app: AppHandle) {
    let mut lines = BufReader::new(stderr).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                if line.trim().is_empty() {
                    continue;
                }
                let level = if line.to_lowercase().contains("error")
                    || line.to_lowercase().contains("fail")
                {
                    LogLevel::Error
                } else if line.to_lowercase().contains("warn") {
                    LogLevel::Warn
                } else {
                    LogLevel::Info
                };
                logs::log_with(&app, level, "browser-sidecar", line);
            }
            Ok(None) => {
                logs::log_with(
                    &app,
                    LogLevel::Warn,
                    "browser",
                    "sidecar stderr closed",
                );
                break;
            }
            Err(e) => {
                logs::log_with(
                    &app,
                    LogLevel::Error,
                    "browser",
                    format!("stderr read: {e}"),
                );
                break;
            }
        }
    }
}

fn sidecar_script_path(app: &AppHandle) -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("REFLEX_BROWSER_SIDECAR") {
        return Ok(PathBuf::from(p));
    }
    let mut tried = Vec::new();
    if let Ok(resource) = app.path().resource_dir() {
        let candidate = resource
            .join("sidecars")
            .join("reflex-browser-server")
            .join("server.mjs");
        if candidate.exists() {
            return Ok(candidate);
        }
        tried.push(candidate);
    }
    if let Ok(cwd) = std::env::current_dir() {
        let mut up = Some(cwd.clone());
        while let Some(dir) = up {
            let candidate = dir
                .join("sidecars")
                .join("reflex-browser-server")
                .join("server.mjs");
            if candidate.exists() {
                return Ok(candidate);
            }
            tried.push(candidate);
            up = dir.parent().map(|p| p.to_path_buf());
        }
    }
    Err(format!(
        "browser sidecar not found, tried: {}",
        tried
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

fn resolve_node_binary() -> Result<String, String> {
    if let Ok(p) = std::env::var("REFLEX_NODE_BIN") {
        if !p.is_empty() {
            return Ok(p);
        }
    }
    let candidates = [
        "/opt/homebrew/bin/node",
        "/usr/local/bin/node",
        "/usr/bin/node",
    ];
    for c in candidates.iter() {
        if std::path::Path::new(c).exists() {
            return Ok((*c).to_string());
        }
    }
    if let Ok(out) = std::process::Command::new("/bin/bash")
        .args(["-lc", "command -v node"])
        .output()
    {
        let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !path.is_empty() && std::path::Path::new(&path).exists() {
            return Ok(path);
        }
    }
    Err("node binary not found (tried REFLEX_NODE_BIN env, common paths, and login shell)".into())
}

fn browser_state_path(app: &AppHandle) -> Result<PathBuf, String> {
    let base = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir: {e}"))?;
    let dir = base.join("browser");
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;
    Ok(dir.join("storageState.json"))
}

#[tauri::command]
pub async fn browser_init(
    app: AppHandle,
    headless: Option<bool>,
) -> Result<Value, String> {
    let s = app.state::<BrowserSidecar>();
    s.request(
        &app,
        "browser.init",
        serde_json::json!({ "headless": headless.unwrap_or(false) }),
    )
    .await
}

#[tauri::command]
pub async fn browser_shutdown(app: AppHandle) -> Result<Value, String> {
    let s = app.state::<BrowserSidecar>();
    s.request(&app, "browser.shutdown", Value::Null).await
}

#[tauri::command]
pub async fn browser_tabs_list(app: AppHandle) -> Result<Value, String> {
    let s = app.state::<BrowserSidecar>();
    s.request(&app, "tabs.list", Value::Null).await
}

#[tauri::command]
pub async fn browser_tab_open(
    app: AppHandle,
    url: Option<String>,
) -> Result<Value, String> {
    let s = app.state::<BrowserSidecar>();
    s.request(&app, "tabs.open", serde_json::json!({ "url": url }))
        .await
}

#[tauri::command]
pub async fn browser_tab_close(
    app: AppHandle,
    tab_id: String,
) -> Result<Value, String> {
    let s = app.state::<BrowserSidecar>();
    s.request(&app, "tabs.close", serde_json::json!({ "tab_id": tab_id }))
        .await
}

#[tauri::command]
pub async fn browser_navigate(
    app: AppHandle,
    tab_id: String,
    url: String,
) -> Result<Value, String> {
    let s = app.state::<BrowserSidecar>();
    s.request(
        &app,
        "page.navigate",
        serde_json::json!({ "tab_id": tab_id, "url": url }),
    )
    .await
}

#[tauri::command]
pub async fn browser_back(app: AppHandle, tab_id: String) -> Result<Value, String> {
    let s = app.state::<BrowserSidecar>();
    s.request(&app, "page.back", serde_json::json!({ "tab_id": tab_id }))
        .await
}

#[tauri::command]
pub async fn browser_forward(app: AppHandle, tab_id: String) -> Result<Value, String> {
    let s = app.state::<BrowserSidecar>();
    s.request(&app, "page.forward", serde_json::json!({ "tab_id": tab_id }))
        .await
}

#[tauri::command]
pub async fn browser_reload(app: AppHandle, tab_id: String) -> Result<Value, String> {
    let s = app.state::<BrowserSidecar>();
    s.request(&app, "page.reload", serde_json::json!({ "tab_id": tab_id }))
        .await
}

#[tauri::command]
pub async fn browser_current_url(
    app: AppHandle,
    tab_id: String,
) -> Result<Value, String> {
    let s = app.state::<BrowserSidecar>();
    s.request(
        &app,
        "page.current_url",
        serde_json::json!({ "tab_id": tab_id }),
    )
    .await
}

#[tauri::command]
pub async fn browser_read_text(
    app: AppHandle,
    tab_id: String,
) -> Result<Value, String> {
    let s = app.state::<BrowserSidecar>();
    s.request(
        &app,
        "page.read_text",
        serde_json::json!({ "tab_id": tab_id }),
    )
    .await
}

#[tauri::command]
pub async fn browser_read_outline(
    app: AppHandle,
    tab_id: String,
) -> Result<Value, String> {
    let s = app.state::<BrowserSidecar>();
    s.request(
        &app,
        "page.read_outline",
        serde_json::json!({ "tab_id": tab_id }),
    )
    .await
}

#[tauri::command]
pub async fn browser_click_text(
    app: AppHandle,
    tab_id: String,
    text: String,
    exact: Option<bool>,
) -> Result<Value, String> {
    let s = app.state::<BrowserSidecar>();
    s.request(
        &app,
        "page.click_text",
        serde_json::json!({ "tab_id": tab_id, "text": text, "exact": exact.unwrap_or(false) }),
    )
    .await
}

#[tauri::command]
pub async fn browser_click_selector(
    app: AppHandle,
    tab_id: String,
    selector: String,
) -> Result<Value, String> {
    let s = app.state::<BrowserSidecar>();
    s.request(
        &app,
        "page.click_selector",
        serde_json::json!({ "tab_id": tab_id, "selector": selector }),
    )
    .await
}

#[tauri::command]
pub async fn browser_fill(
    app: AppHandle,
    tab_id: String,
    selector: String,
    value: String,
) -> Result<Value, String> {
    let s = app.state::<BrowserSidecar>();
    s.request(
        &app,
        "page.fill",
        serde_json::json!({ "tab_id": tab_id, "selector": selector, "value": value }),
    )
    .await
}

#[tauri::command]
pub async fn browser_scroll(
    app: AppHandle,
    tab_id: String,
    dx: Option<i64>,
    dy: Option<i64>,
) -> Result<Value, String> {
    let s = app.state::<BrowserSidecar>();
    s.request(
        &app,
        "page.scroll",
        serde_json::json!({ "tab_id": tab_id, "dx": dx, "dy": dy }),
    )
    .await
}

#[tauri::command]
pub async fn browser_wait_for(
    app: AppHandle,
    tab_id: String,
    selector: String,
    timeout_ms: Option<u64>,
) -> Result<Value, String> {
    let s = app.state::<BrowserSidecar>();
    s.request(
        &app,
        "page.wait_for",
        serde_json::json!({ "tab_id": tab_id, "selector": selector, "timeout": timeout_ms }),
    )
    .await
}

#[tauri::command]
pub async fn browser_screenshot(
    app: AppHandle,
    tab_id: String,
    full_page: Option<bool>,
) -> Result<Value, String> {
    let s = app.state::<BrowserSidecar>();
    s.request(
        &app,
        "page.screenshot",
        serde_json::json!({ "tab_id": tab_id, "full_page": full_page.unwrap_or(false) }),
    )
    .await
}

#[tauri::command]
pub async fn browser_state_save(app: AppHandle) -> Result<Value, String> {
    let s = app.state::<BrowserSidecar>();
    s.request(&app, "state.save", Value::Null).await
}

#[tauri::command]
pub async fn browser_screencast_start(
    app: AppHandle,
    tab_id: String,
    quality: Option<u32>,
    max_width: Option<u32>,
    max_height: Option<u32>,
    every_nth_frame: Option<u32>,
) -> Result<Value, String> {
    let s = app.state::<BrowserSidecar>();
    s.request(
        &app,
        "screencast.start",
        serde_json::json!({
            "tab_id": tab_id,
            "quality": quality,
            "max_width": max_width,
            "max_height": max_height,
            "every_nth_frame": every_nth_frame,
        }),
    )
    .await
}

#[tauri::command]
pub async fn browser_screencast_stop(
    app: AppHandle,
    tab_id: String,
) -> Result<Value, String> {
    let s = app.state::<BrowserSidecar>();
    s.request(&app, "screencast.stop", serde_json::json!({ "tab_id": tab_id }))
        .await
}

#[tauri::command]
pub async fn browser_set_viewport(
    app: AppHandle,
    tab_id: String,
    width: u32,
    height: u32,
) -> Result<Value, String> {
    let s = app.state::<BrowserSidecar>();
    s.request(
        &app,
        "page.set_viewport",
        serde_json::json!({ "tab_id": tab_id, "width": width, "height": height }),
    )
    .await
}

#[tauri::command]
pub async fn browser_mouse_move(
    app: AppHandle,
    tab_id: String,
    x: f64,
    y: f64,
    steps: Option<u32>,
) -> Result<Value, String> {
    let s = app.state::<BrowserSidecar>();
    s.request(
        &app,
        "page.mouse_move",
        serde_json::json!({ "tab_id": tab_id, "x": x, "y": y, "steps": steps }),
    )
    .await
}

#[tauri::command]
pub async fn browser_mouse_down(
    app: AppHandle,
    tab_id: String,
    button: Option<String>,
    click_count: Option<u32>,
) -> Result<Value, String> {
    let s = app.state::<BrowserSidecar>();
    s.request(
        &app,
        "page.mouse_down",
        serde_json::json!({ "tab_id": tab_id, "button": button, "click_count": click_count }),
    )
    .await
}

#[tauri::command]
pub async fn browser_mouse_up(
    app: AppHandle,
    tab_id: String,
    button: Option<String>,
    click_count: Option<u32>,
) -> Result<Value, String> {
    let s = app.state::<BrowserSidecar>();
    s.request(
        &app,
        "page.mouse_up",
        serde_json::json!({ "tab_id": tab_id, "button": button, "click_count": click_count }),
    )
    .await
}

#[tauri::command]
pub async fn browser_mouse_click(
    app: AppHandle,
    tab_id: String,
    x: f64,
    y: f64,
    button: Option<String>,
    click_count: Option<u32>,
) -> Result<Value, String> {
    let s = app.state::<BrowserSidecar>();
    s.request(
        &app,
        "page.mouse_click",
        serde_json::json!({
            "tab_id": tab_id,
            "x": x,
            "y": y,
            "button": button,
            "click_count": click_count,
        }),
    )
    .await
}

#[tauri::command]
pub async fn browser_mouse_wheel(
    app: AppHandle,
    tab_id: String,
    dx: f64,
    dy: f64,
) -> Result<Value, String> {
    let s = app.state::<BrowserSidecar>();
    s.request(
        &app,
        "page.mouse_wheel",
        serde_json::json!({ "tab_id": tab_id, "dx": dx, "dy": dy }),
    )
    .await
}

#[tauri::command]
pub async fn browser_keyboard_type(
    app: AppHandle,
    tab_id: String,
    text: String,
    delay: Option<u32>,
) -> Result<Value, String> {
    let s = app.state::<BrowserSidecar>();
    s.request(
        &app,
        "page.keyboard_type",
        serde_json::json!({ "tab_id": tab_id, "text": text, "delay": delay }),
    )
    .await
}

#[tauri::command]
pub async fn browser_keyboard_press(
    app: AppHandle,
    tab_id: String,
    key: String,
    delay: Option<u32>,
) -> Result<Value, String> {
    let s = app.state::<BrowserSidecar>();
    s.request(
        &app,
        "page.keyboard_press",
        serde_json::json!({ "tab_id": tab_id, "key": key, "delay": delay }),
    )
    .await
}
