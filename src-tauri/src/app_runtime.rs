use std::collections::HashMap;
use std::net::TcpListener;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::time::sleep;

use crate::apps;

const LOG_BUFFER_LIMIT: usize = 500;

#[derive(Clone, serde::Serialize)]
pub struct LogLine {
    pub seq: u64,
    pub stream: String, // "stdout" | "stderr" | "system"
    pub line: String,
    pub ts_ms: u128,
}

pub struct ServerEntry {
    pub child: Child,
    pub port: u16,
    pub ref_count: usize,
    pub logs: Vec<LogLine>,
    pub log_seq: u64,
    pub exit_code: Option<i32>,
}

#[derive(Default)]
pub struct AppRuntimes {
    pub servers: Arc<Mutex<HashMap<String, ServerEntry>>>,
}

#[derive(serde::Serialize)]
pub struct ServerStatus {
    pub running: bool,
    pub port: Option<u16>,
    pub exit_code: Option<i32>,
}

#[derive(serde::Serialize)]
pub struct LogsSnapshot {
    pub lines: Vec<LogLine>,
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn pick_free_port() -> std::io::Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

async fn wait_port_open(port: u16, timeout_ms: u64) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let addr = format!("127.0.0.1:{port}");
    loop {
        if let Ok(stream) =
            std::net::TcpStream::connect_timeout(&addr.parse().unwrap(), Duration::from_millis(200))
        {
            drop(stream);
            return Ok(());
        }
        if Instant::now() > deadline {
            return Err(format!(
                "server didn't start listening on :{port} within {timeout_ms}ms"
            ));
        }
        sleep(Duration::from_millis(150)).await;
    }
}

fn append_log(
    entry: &mut ServerEntry,
    stream: &str,
    line: String,
) -> LogLine {
    entry.log_seq += 1;
    let log = LogLine {
        seq: entry.log_seq,
        stream: stream.into(),
        line,
        ts_ms: now_ms(),
    };
    entry.logs.push(log.clone());
    if entry.logs.len() > LOG_BUFFER_LIMIT {
        let drop_n = entry.logs.len() - LOG_BUFFER_LIMIT;
        entry.logs.drain(0..drop_n);
    }
    log
}

async fn push_log(
    runtimes: &AppRuntimes,
    app_handle: &AppHandle,
    app_id: &str,
    stream: &str,
    line: String,
) {
    let log = {
        let mut map = runtimes.servers.lock().await;
        match map.get_mut(app_id) {
            Some(entry) => Some(append_log(entry, stream, line)),
            None => None,
        }
    };
    if let Some(log) = log {
        let _ = app_handle.emit(
            "reflex://app-server-log",
            &serde_json::json!({
                "app_id": app_id,
                "stream": log.stream,
                "seq": log.seq,
                "line": log.line,
                "ts_ms": log.ts_ms,
            }),
        );
    }
}

fn spawn_reader<R>(
    runtimes: Arc<Mutex<HashMap<String, ServerEntry>>>,
    app_handle: AppHandle,
    app_id: String,
    stream_label: &'static str,
    reader: R,
) where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    let runtimes_for_task = AppRuntimes { servers: runtimes };
    tauri::async_runtime::spawn(async move {
        let mut buf = BufReader::new(reader).lines();
        while let Ok(Some(line)) = buf.next_line().await {
            push_log(
                &runtimes_for_task,
                &app_handle,
                &app_id,
                stream_label,
                line,
            )
            .await;
        }
    });
}

/// Spawn the server child. Returns a fully-populated ServerEntry (not yet inserted).
async fn spawn_child(
    runtimes_arc: Arc<Mutex<HashMap<String, ServerEntry>>>,
    app: &AppHandle,
    app_id: &str,
) -> Result<ServerEntry, String> {
    let manifest = apps::read_manifest(app, app_id).map_err(|e| e.to_string())?;
    let server_cfg = manifest
        .server
        .ok_or_else(|| "manifest.server is missing".to_string())?;
    if server_cfg.command.is_empty() {
        return Err("manifest.server.command is empty".into());
    }
    let dir = apps::app_dir(app, app_id).map_err(|e| e.to_string())?;
    let port = pick_free_port().map_err(|e| format!("pick_free_port: {e}"))?;

    let mut cmd = Command::new(&server_cfg.command[0]);
    cmd.args(&server_cfg.command[1..])
        .current_dir(&dir)
        .env("REFLEX_PORT", port.to_string())
        .env("PORT", port.to_string())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("spawn {} failed: {e}", server_cfg.command[0]))?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    if let Some(out) = stdout {
        spawn_reader(
            runtimes_arc.clone(),
            app.clone(),
            app_id.to_string(),
            "stdout",
            out,
        );
    }
    if let Some(err) = stderr {
        spawn_reader(
            runtimes_arc.clone(),
            app.clone(),
            app_id.to_string(),
            "stderr",
            err,
        );
    }

    // Note: on failure here, ServerEntry is dropped → child dropped → kill_on_drop kicks in.
    let timeout = server_cfg.ready_timeout_ms.unwrap_or(15_000);
    if let Err(e) = wait_port_open(port, timeout).await {
        return Err(e);
    }

    Ok(ServerEntry {
        child,
        port,
        ref_count: 0,
        logs: vec![],
        log_seq: 0,
        exit_code: None,
    })
}

/// Start (or reuse) a server-runtime app. Returns port. Increments refcount.
pub async fn start(
    runtimes: &AppRuntimes,
    app: &AppHandle,
    app_id: &str,
) -> Result<u16, String> {
    {
        let mut map = runtimes.servers.lock().await;
        if let Some(entry) = map.get_mut(app_id) {
            let alive = matches!(entry.child.try_wait(), Ok(None));
            if alive {
                entry.ref_count += 1;
                return Ok(entry.port);
            } else {
                if let Ok(Some(s)) = entry.child.try_wait() {
                    entry.exit_code = s.code();
                }
                map.remove(app_id);
            }
        }
    }

    let mut entry = spawn_child(runtimes.servers.clone(), app, app_id).await?;
    entry.ref_count = 1;
    let port = entry.port;
    let init = LogLine {
        seq: 0,
        stream: "system".into(),
        line: format!("[reflex] server started on :{port}"),
        ts_ms: now_ms(),
    };
    entry.logs.push(init);
    {
        let mut map = runtimes.servers.lock().await;
        map.insert(app_id.to_string(), entry);
    }
    Ok(port)
}

/// Start a server-runtime app without incrementing refcount when it is already alive.
/// This is intended for explicit app-management controls rather than mounted viewers.
pub async fn ensure_started(
    runtimes: &AppRuntimes,
    app: &AppHandle,
    app_id: &str,
) -> Result<u16, String> {
    {
        let mut map = runtimes.servers.lock().await;
        if let Some(entry) = map.get_mut(app_id) {
            let alive = matches!(entry.child.try_wait(), Ok(None));
            if alive {
                return Ok(entry.port);
            } else {
                if let Ok(Some(s)) = entry.child.try_wait() {
                    entry.exit_code = s.code();
                }
                map.remove(app_id);
            }
        }
    }

    let mut entry = spawn_child(runtimes.servers.clone(), app, app_id).await?;
    entry.ref_count = 1;
    let port = entry.port;
    let init = LogLine {
        seq: 0,
        stream: "system".into(),
        line: format!("[reflex] server started on :{port}"),
        ts_ms: now_ms(),
    };
    entry.logs.push(init);
    {
        let mut map = runtimes.servers.lock().await;
        map.insert(app_id.to_string(), entry);
    }
    Ok(port)
}

/// Decrement refcount; if 0, kill the child.
pub async fn stop(runtimes: &AppRuntimes, app_id: &str) {
    let mut map = runtimes.servers.lock().await;
    let kill = match map.get_mut(app_id) {
        Some(entry) => {
            entry.ref_count = entry.ref_count.saturating_sub(1);
            entry.ref_count == 0
        }
        None => false,
    };
    if kill {
        if let Some(mut entry) = map.remove(app_id) {
            let _ = entry.child.kill().await;
        }
    }
}

/// Force kill the running server (regardless of refcount) and start a fresh one.
/// Used after revise to pick up new files. Refcount is preserved.
pub async fn restart(
    runtimes: &AppRuntimes,
    app: &AppHandle,
    app_id: &str,
) -> Result<u16, String> {
    let preserved_ref_count = {
        let mut map = runtimes.servers.lock().await;
        if let Some(mut entry) = map.remove(app_id) {
            let _ = entry.child.kill().await;
            entry.ref_count.max(1)
        } else {
            1
        }
    };

    let mut entry = spawn_child(runtimes.servers.clone(), app, app_id).await?;
    entry.ref_count = preserved_ref_count;
    let port = entry.port;
    let init = LogLine {
        seq: 0,
        stream: "system".into(),
        line: format!("[reflex] server restarted on :{port}"),
        ts_ms: now_ms(),
    };
    entry.logs.push(init);
    {
        let mut map = runtimes.servers.lock().await;
        map.insert(app_id.to_string(), entry);
    }
    Ok(port)
}

pub async fn status(runtimes: &AppRuntimes, app_id: &str) -> ServerStatus {
    let mut map = runtimes.servers.lock().await;
    match map.get_mut(app_id) {
        Some(entry) => match entry.child.try_wait() {
            Ok(None) => ServerStatus {
                running: true,
                port: Some(entry.port),
                exit_code: None,
            },
            Ok(Some(code)) => {
                let ec = code.code();
                entry.exit_code = ec;
                ServerStatus {
                    running: false,
                    port: Some(entry.port),
                    exit_code: ec,
                }
            }
            Err(_) => ServerStatus {
                running: false,
                port: Some(entry.port),
                exit_code: None,
            },
        },
        None => ServerStatus {
            running: false,
            port: None,
            exit_code: None,
        },
    }
}

pub async fn running_port(runtimes: &AppRuntimes, app_id: &str) -> Option<u16> {
    let mut map = runtimes.servers.lock().await;
    match map.get_mut(app_id) {
        Some(entry) => match entry.child.try_wait() {
            Ok(None) => Some(entry.port),
            Ok(Some(code)) => {
                entry.exit_code = code.code();
                None
            }
            Err(_) => None,
        },
        None => None,
    }
}

pub async fn logs(runtimes: &AppRuntimes, app_id: &str) -> LogsSnapshot {
    let map = runtimes.servers.lock().await;
    LogsSnapshot {
        lines: map
            .get(app_id)
            .map(|e| e.logs.clone())
            .unwrap_or_default(),
    }
}
