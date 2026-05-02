use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command as TokioCommand};
use tokio::sync::{mpsc, oneshot};

use crate::storage;

const CODEX_EVENT: &str = "reflex://codex-event";
const CODEX_END_EVENT: &str = "reflex://codex-end";

#[derive(Default)]
struct Inner {
    next_id: Mutex<u64>,
    pending: Mutex<HashMap<u64, oneshot::Sender<Result<Value, Value>>>>,
    stdin: tokio::sync::Mutex<Option<ChildStdin>>,
    threads: Mutex<HashMap<String, ReflexThread>>,
    questions: Mutex<HashMap<String, PendingQuestion>>,
    /// app_thread_id → oneshot waiter for turn/completed (used by scratch tasks).
    turn_waits: Mutex<HashMap<String, oneshot::Sender<Value>>>,
    /// app_thread_id → bus that streams agent message deltas to a listener (used by agent.stream).
    stream_listeners: Mutex<HashMap<String, mpsc::UnboundedSender<StreamEvent>>>,
}

#[derive(Clone, Debug)]
pub enum StreamEvent {
    Delta(String),
    Done(Option<String>),
}

#[derive(Clone)]
pub struct PendingQuestion {
    pub request_id: Value,
    pub method: String,
    pub params: Value,
    pub reflex_thread_id: Option<String>,
}

#[derive(Clone)]
pub struct ReflexThread {
    pub reflex_id: String,
    pub project_root: PathBuf,
    pub seq: u64,
    pub current_turn_id: Option<String>,
}

#[derive(Clone)]
pub struct AppServerClient {
    inner: Arc<Inner>,
}

pub struct AppServerHandle {
    inner: tokio::sync::Mutex<Option<AppServerClient>>,
    notify: tokio::sync::Notify,
}

impl Default for AppServerHandle {
    fn default() -> Self {
        Self {
            inner: tokio::sync::Mutex::new(None),
            notify: tokio::sync::Notify::new(),
        }
    }
}

impl AppServerHandle {
    pub async fn set(&self, client: AppServerClient) {
        *self.inner.lock().await = Some(client);
        self.notify.notify_waiters();
    }

    pub async fn wait(&self) -> AppServerClient {
        loop {
            {
                let guard = self.inner.lock().await;
                if let Some(c) = guard.as_ref() {
                    return c.clone();
                }
            }
            self.notify.notified().await;
        }
    }
}

#[derive(Serialize)]
struct ClientInfo {
    name: &'static str,
    version: &'static str,
    title: Option<&'static str>,
}

#[derive(Serialize)]
struct InitializeParams {
    #[serde(rename = "clientInfo")]
    client_info: ClientInfo,
    capabilities: Value,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct InitializeResult {
    #[serde(default, rename = "userAgent")]
    pub user_agent: Option<String>,
    #[serde(default, rename = "codexHome")]
    pub codex_home: Option<String>,
}

impl AppServerClient {
    pub async fn start(app: AppHandle) -> std::io::Result<Self> {
        eprintln!("[app-server] spawning codex app-server");
        let mut child = TokioCommand::new("codex")
            .args(["app-server", "--listen", "stdio://"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;

        let stdin = child.stdin.take().expect("stdin piped");
        let stdout = child.stdout.take().expect("stdout piped");
        let stderr = child.stderr.take().expect("stderr piped");

        let inner = Arc::new(Inner::default());
        *inner.stdin.lock().await = Some(stdin);

        {
            let inner = inner.clone();
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let mut lines = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    handle_line(&inner, &app, &line).await;
                }
                eprintln!("[app-server] stdout closed");
            });
        }
        tauri::async_runtime::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                eprintln!("[app-server stderr] {line}");
            }
        });
        tauri::async_runtime::spawn(async move {
            match child.wait().await {
                Ok(status) => eprintln!("[app-server] exited: {status}"),
                Err(e) => eprintln!("[app-server] wait err: {e}"),
            }
        });

        Ok(Self { inner })
    }

    pub async fn request(&self, method: &str, params: Value) -> Result<Value, Value> {
        let id = {
            let mut n = self.inner.next_id.lock().unwrap();
            *n += 1;
            *n
        };
        let (tx, rx) = oneshot::channel();
        self.inner.pending.lock().unwrap().insert(id, tx);
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let line = format!("{req}\n");
        eprintln!("[app-server ->] {req}");
        let mut guard = self.inner.stdin.lock().await;
        if let Some(stdin) = guard.as_mut() {
            if let Err(e) = stdin.write_all(line.as_bytes()).await {
                self.inner.pending.lock().unwrap().remove(&id);
                return Err(serde_json::json!({"code": -32000, "message": format!("write failed: {e}")}));
            }
            let _ = stdin.flush().await;
        } else {
            self.inner.pending.lock().unwrap().remove(&id);
            return Err(serde_json::json!({"code": -32000, "message": "stdin closed"}));
        }
        drop(guard);
        rx.await
            .unwrap_or_else(|_| Err(serde_json::json!({"code": -32000, "message": "client closed"})))
    }

    pub async fn initialize(&self) -> Result<InitializeResult, Value> {
        let params = InitializeParams {
            client_info: ClientInfo {
                name: "reflex",
                version: env!("CARGO_PKG_VERSION"),
                title: Some("Reflex"),
            },
            capabilities: serde_json::json!({}),
        };
        let result = self
            .request("initialize", serde_json::to_value(&params).unwrap())
            .await?;
        serde_json::from_value(result).map_err(|e| {
            serde_json::json!({"code": -32000, "message": format!("decode initialize: {e}")})
        })
    }

    pub async fn thread_start(
        &self,
        cwd: &PathBuf,
        sandbox: &str,
        mcp_servers: Option<&Value>,
    ) -> Result<String, Value> {
        let mut params = serde_json::json!({
            "cwd": cwd.to_string_lossy(),
            "sandbox": sandbox,
            "approvalPolicy": "on-request",
            "experimentalRawEvents": false,
            "persistExtendedHistory": false,
        });
        if let Some(mcp) = mcp_servers {
            params["config"] = serde_json::json!({"mcp_servers": mcp});
        }
        let result = self.request("thread/start", params).await?;
        // Try common shapes: {threadId} or {thread: {id}}
        if let Some(s) = result.get("threadId").and_then(|v| v.as_str()) {
            return Ok(s.to_string());
        }
        if let Some(s) = result
            .get("thread")
            .and_then(|t| t.get("id"))
            .and_then(|v| v.as_str())
        {
            return Ok(s.to_string());
        }
        Err(serde_json::json!({
            "code": -32000,
            "message": format!("missing threadId in result: {result}")
        }))
    }

    pub async fn thread_resume(
        &self,
        thread_id: &str,
        sandbox: &str,
        mcp_servers: Option<&Value>,
    ) -> Result<Value, Value> {
        let mut params = serde_json::json!({
            "threadId": thread_id,
            "persistExtendedHistory": false,
            "excludeTurns": true,
            "approvalPolicy": "on-request",
            "sandbox": sandbox,
        });
        if let Some(mcp) = mcp_servers {
            params["config"] = serde_json::json!({"mcp_servers": mcp});
        }
        self.request("thread/resume", params).await
    }

    pub async fn turn_start(&self, thread_id: &str, prompt: &str) -> Result<Value, Value> {
        self.turn_start_with_local_images(thread_id, prompt, &[])
            .await
    }

    pub async fn turn_start_with_local_images(
        &self,
        thread_id: &str,
        prompt: &str,
        local_images: &[String],
    ) -> Result<Value, Value> {
        let mut input = vec![serde_json::json!({
            "type": "text",
            "text": prompt,
            "text_elements": [],
        })];
        for path in local_images {
            input.push(serde_json::json!({
                "type": "localImage",
                "path": path,
            }));
        }
        let params = serde_json::json!({
            "threadId": thread_id,
            "input": input,
        });
        self.request("turn/start", params).await
    }

    pub async fn turn_interrupt(&self, thread_id: &str, turn_id: &str) -> Result<Value, Value> {
        let params = serde_json::json!({
            "threadId": thread_id,
            "turnId": turn_id,
        });
        self.request("turn/interrupt", params).await
    }

    pub fn register_thread(
        &self,
        app_thread_id: String,
        reflex_id: String,
        project_root: PathBuf,
        initial_seq: u64,
    ) {
        let mut map = self.inner.threads.lock().unwrap();
        map.insert(
            app_thread_id,
            ReflexThread {
                reflex_id,
                project_root,
                seq: initial_seq,
                current_turn_id: None,
            },
        );
    }

    /// Wait for `turn/completed` for the given app_thread_id. Returns the `turn` object.
    pub async fn wait_for_turn(&self, app_thread_id: &str) -> Option<Value> {
        let (tx, rx) = oneshot::channel();
        {
            let mut waits = self.inner.turn_waits.lock().unwrap();
            waits.insert(app_thread_id.to_string(), tx);
        }
        rx.await.ok()
    }

    /// Subscribe to streaming agent message deltas for the given app_thread_id.
    /// Returns a receiver that yields StreamEvent::Delta and finally StreamEvent::Done.
    pub fn subscribe_stream(
        &self,
        app_thread_id: &str,
    ) -> mpsc::UnboundedReceiver<StreamEvent> {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut listeners = self.inner.stream_listeners.lock().unwrap();
        listeners.insert(app_thread_id.to_string(), tx);
        rx
    }

    pub fn unsubscribe_stream(&self, app_thread_id: &str) {
        let mut listeners = self.inner.stream_listeners.lock().unwrap();
        listeners.remove(app_thread_id);
    }

    pub async fn send_response(&self, id: Value, result: Value) -> Result<(), String> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        });
        let line = format!("{payload}\n");
        eprintln!("[app-server -> response] {payload}");
        let mut guard = self.inner.stdin.lock().await;
        if let Some(stdin) = guard.as_mut() {
            stdin
                .write_all(line.as_bytes())
                .await
                .map_err(|e| e.to_string())?;
            stdin.flush().await.map_err(|e| e.to_string())?;
            Ok(())
        } else {
            Err("stdin closed".into())
        }
    }

    pub fn take_question(&self, question_key: &str) -> Option<PendingQuestion> {
        self.inner.questions.lock().unwrap().remove(question_key)
    }

    pub fn current_turn_id(&self, reflex_id: &str) -> Option<(String, String)> {
        let map = self.inner.threads.lock().unwrap();
        for (app_id, rt) in map.iter() {
            if rt.reflex_id == reflex_id {
                if let Some(tid) = &rt.current_turn_id {
                    return Some((app_id.clone(), tid.clone()));
                }
            }
        }
        None
    }
}

fn extract_thread_id(method: &str, params: &Value) -> Option<String> {
    if let Some(s) = params.get("threadId").and_then(|v| v.as_str()) {
        return Some(s.to_string());
    }
    if method == "thread/started" {
        if let Some(s) = params
            .get("thread")
            .and_then(|t| t.get("id"))
            .and_then(|v| v.as_str())
        {
            return Some(s.to_string());
        }
    }
    None
}

fn translate_notification(method: &str, params: &Value) -> Option<Value> {
    match method {
        "thread/started" => Some(serde_json::json!({
            "type": "thread.started",
            "thread_id": extract_thread_id(method, params),
        })),
        "turn/started" => Some(serde_json::json!({
            "type": "turn.started",
            "turn_id": params.get("turnId").cloned(),
        })),
        "turn/completed" => Some(serde_json::json!({
            "type": "turn.completed",
            "turn": params.get("turn").cloned(),
        })),
        "item/started" => Some(serde_json::json!({
            "type": "item.started",
            "item": params.get("item").cloned(),
        })),
        "item/completed" => Some(serde_json::json!({
            "type": "item.completed",
            "item": params.get("item").cloned(),
        })),
        "item/agentMessage/delta" => Some(serde_json::json!({
            "type": "item.agentMessage.delta",
            "delta": params.get("delta").cloned(),
            "itemId": params.get("itemId").cloned(),
        })),
        "item/commandExecution/outputDelta"
        | "command/exec/outputDelta"
        | "item/commandExecution/terminalInteraction"
        | "item/fileChange/outputDelta"
        | "item/fileChange/patchUpdated" => Some(serde_json::json!({
            "type": method,
            "params": params,
        })),
        "thread/tokenUsage/updated" | "turn/diff/updated" | "turn/plan/updated" => {
            Some(serde_json::json!({
                "type": method,
                "params": params,
            }))
        }
        _ => None,
    }
}

async fn handle_line(inner: &Arc<Inner>, app: &AppHandle, line: &str) {
    eprintln!("[app-server <-] {line}");
    let v: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[app-server] parse err: {e} line={line}");
            return;
        }
    };

    let has_method = v.get("method").is_some();
    let has_result_or_error = v.get("result").is_some() || v.get("error").is_some();

    if has_result_or_error {
        if let Some(id) = v.get("id").and_then(|v| v.as_u64()) {
            let tx = inner.pending.lock().unwrap().remove(&id);
            if let Some(tx) = tx {
                let res = if let Some(err) = v.get("error") {
                    Err(err.clone())
                } else {
                    Ok(v.get("result").cloned().unwrap_or(Value::Null))
                };
                let _ = tx.send(res);
            } else {
                eprintln!("[app-server] response with unknown id={id}");
            }
        }
        return;
    }

    if !has_method {
        return;
    }

    let method = v
        .get("method")
        .and_then(|m| m.as_str())
        .unwrap_or("")
        .to_string();
    let id = v.get("id").cloned();
    let params = v.get("params").cloned().unwrap_or(Value::Null);

    if let Some(req_id) = id.clone() {
        // server -> client request (approvals etc)
        // find which thread this question relates to
        let app_thread_id = params
            .get("threadId")
            .or_else(|| params.get("conversationId"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let reflex_thread_id = if let Some(tid) = &app_thread_id {
            inner
                .threads
                .lock()
                .unwrap()
                .get(tid)
                .map(|t| t.reflex_id.clone())
        } else {
            None
        };

        let key = req_id.to_string();
        inner.questions.lock().unwrap().insert(
            key.clone(),
            PendingQuestion {
                request_id: req_id.clone(),
                method: method.clone(),
                params: params.clone(),
                reflex_thread_id: reflex_thread_id.clone(),
            },
        );

        let payload = serde_json::json!({
            "question_id": key,
            "method": method,
            "params": params,
            "thread_id": reflex_thread_id,
        });
        let _ = app.emit("reflex://thread-question", &payload);

        // also push notification
        use tauri_plugin_notification::NotificationExt;
        let label = reflex_thread_id.unwrap_or_else(|| "Reflex".into());
        let _ = app
            .notification()
            .builder()
            .title("Reflex — agent asks")
            .body(format!("{} · {}", label, method))
            .show();

        return;
    }

    // notification — route to thread if applicable
    let app_thread_id = extract_thread_id(&method, &params);
    let mut reflex_thread: Option<ReflexThread> = None;
    if let Some(tid) = &app_thread_id {
        let map = inner.threads.lock().unwrap();
        reflex_thread = map.get(tid).cloned();
    }

    // Scratch hooks (agent.task / agent.stream) — work even when thread is not registered in `threads`.
    if let Some(tid) = &app_thread_id {
        if method == "item/agentMessage/delta" {
            if let Some(delta) = params.get("delta").and_then(|v| v.as_str()) {
                let listener = {
                    let listeners = inner.stream_listeners.lock().unwrap();
                    listeners.get(tid).cloned()
                };
                if let Some(tx) = listener {
                    let _ = tx.send(StreamEvent::Delta(delta.to_string()));
                }
            }
        }
        if method == "turn/completed" {
            // Notify task waiters
            let waiter = {
                let mut waits = inner.turn_waits.lock().unwrap();
                waits.remove(tid)
            };
            if let Some(tx) = waiter {
                let turn_obj = params.get("turn").cloned().unwrap_or(Value::Null);
                let _ = tx.send(turn_obj);
            }
            // Notify stream listeners
            let listener = {
                let mut listeners = inner.stream_listeners.lock().unwrap();
                listeners.remove(tid)
            };
            if let Some(tx) = listener {
                let last = params
                    .get("turn")
                    .and_then(|t| {
                        t.get("lastAgentMessage")
                            .or_else(|| t.get("last_agent_message"))
                    })
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let _ = tx.send(StreamEvent::Done(last));
            }
        }
    }

    // Track current turn id on turn/started, clear on turn/completed
    if method == "turn/started" {
        if let (Some(tid), Some(turn)) = (
            &app_thread_id,
            params.get("turnId").and_then(|v| v.as_str()),
        ) {
            let mut map = inner.threads.lock().unwrap();
            if let Some(rt) = map.get_mut(tid) {
                rt.current_turn_id = Some(turn.to_string());
            }
        }
    }

    let translated = translate_notification(&method, &params);

    if let (Some(rt), Some(payload)) = (reflex_thread, translated) {
        // assign next seq
        let seq = {
            let mut map = inner.threads.lock().unwrap();
            if let Some(thread) = map.get_mut(app_thread_id.as_deref().unwrap_or("")) {
                thread.seq += 1;
                thread.seq
            } else {
                rt.seq + 1
            }
        };
        let raw = payload.to_string();
        let ts_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let stored = storage::StoredEvent {
            seq,
            stream: "stdout".to_string(),
            ts_ms,
            raw: raw.clone(),
        };
        let _ = storage::append_event_oneshot(&rt.project_root, &rt.reflex_id, &stored);
        let _ = app.emit(
            CODEX_EVENT,
            &serde_json::json!({
                "thread_id": rt.reflex_id,
                "seq": seq,
                "raw": raw,
                "stream": "stdout",
            }),
        );

        // turn/completed → also emit codex-end + finalize meta
        if method == "turn/completed" {
            // clear current turn id
            {
                let mut map = inner.threads.lock().unwrap();
                if let Some(thread) = map.get_mut(app_thread_id.as_deref().unwrap_or("")) {
                    thread.current_turn_id = None;
                }
            }
            // figure out exit_code: success unless error notif came earlier (we emit 0)
            let exit_code = Some(0);
            let session_id = app_thread_id.clone();
            let _ = storage::finalize_thread(&rt.project_root, &rt.reflex_id, exit_code, session_id);
            crate::codex::notify_thread_done_external(
                app,
                &rt.project_root,
                &rt.reflex_id,
                exit_code,
                false,
            );
            let _ = app.emit(
                CODEX_END_EVENT,
                &serde_json::json!({
                    "thread_id": rt.reflex_id,
                    "exit_code": exit_code,
                }),
            );
        }
    } else if method == "error" {
        // error notification — emit as stderr-like event (no thread context required)
        let raw = serde_json::to_string(&params).unwrap_or_default();
        let _ = app.emit(
            CODEX_EVENT,
            &serde_json::json!({
                "thread_id": app_thread_id.unwrap_or_default(),
                "seq": 0,
                "raw": raw,
                "stream": "error",
            }),
        );
    } else {
        // pass through unknown notifications as a separate channel for debugging
        let _ = app.emit(
            "reflex://app-server-notification",
            &serde_json::json!({"method": method, "params": params}),
        );
    }
}
