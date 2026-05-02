use crate::app_bus::{self, AppBusBridge};
use crate::app_server;
use crate::apps;
use crate::memory::agents::recall::{self, RecallRequest};
use crate::memory::files;
use crate::memory::rag;
use crate::memory::schema::{MemoryKind, MemoryScope, ScopeRoots};
use crate::memory::store::{self, ListFilter, SaveRequest};
use crate::{memory, project};
use crate::scheduler;
use crate::QuickContext;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager};

pub async fn dispatch_app_method(
    app: &AppHandle,
    app_id: &str,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    eprintln!("[reflex] dispatch app={app_id} method={method}");
    match method {
        "system.context" => system_context(app, app_id),
        "agent.ask" => {
            let prompt = params
                .get("prompt")
                .and_then(|v| v.as_str())
                .ok_or("missing prompt")?;
            let answer = crate::ask_agent_oneshot(app, prompt)
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({ "answer": answer }))
        }
        "agent.startTopic" => {
            let prompt = params
                .get("prompt")
                .and_then(|v| v.as_str())
                .ok_or("missing prompt")?;
            let project_id = params
                .get("projectId")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let ctx = QuickContext::default();
            let thread_id = crate::submit_quick_impl(
                app.clone(),
                prompt.into(),
                ctx,
                project_id,
                None,
                None,
                None,
            )?;
            Ok(serde_json::json!({ "threadId": thread_id }))
        }
        "storage.get" => {
            let key = params
                .get("key")
                .and_then(|v| v.as_str())
                .ok_or("missing key")?;
            let store = apps::read_storage(app, app_id).map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "value": store.get(key).cloned().unwrap_or(serde_json::Value::Null),
            }))
        }
        "storage.set" => {
            let key = params
                .get("key")
                .and_then(|v| v.as_str())
                .ok_or("missing key")?;
            let value = params
                .get("value")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            let mut store = apps::read_storage(app, app_id).map_err(|e| e.to_string())?;
            if let Some(obj) = store.as_object_mut() {
                obj.insert(key.to_string(), value);
            }
            apps::write_storage(app, app_id, &store).map_err(|e| e.to_string())?;
            Ok(serde_json::json!({ "ok": true }))
        }
        "fs.read" => {
            let path = params
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or("missing path")?;
            let bytes = apps::read_app_file(app, app_id, path).map_err(|e| e.to_string())?;
            let text = String::from_utf8(bytes).map_err(|e| e.to_string())?;
            Ok(serde_json::json!({ "content": text }))
        }
        "fs.write" => {
            let path = params
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or("missing path")?;
            let content = params
                .get("content")
                .and_then(|v| v.as_str())
                .ok_or("missing content")?;
            apps::write_app_file(app, app_id, path, content.as_bytes())
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({ "ok": true }))
        }
        "notify.show" => {
            let title = params
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Reflex App");
            let body = params
                .get("body")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            use tauri_plugin_notification::NotificationExt;
            app.notification()
                .builder()
                .title(title)
                .body(body)
                .show()
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({ "ok": true }))
        }
        "dialog.openDirectory" => {
            use tauri_plugin_dialog::DialogExt;
            let title = params
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Выбор папки")
                .to_string();
            let mut builder = app.dialog().file().set_title(&title);
            if let Some(default_path) = params.get("defaultPath").and_then(|v| v.as_str()) {
                builder = builder.set_directory(std::path::PathBuf::from(default_path));
            }
            let (tx, rx) = tokio::sync::oneshot::channel();
            builder.pick_folder(move |path| {
                let _ = tx.send(path);
            });
            let picked = rx.await.map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "path": picked.map(|p| p.to_string()),
            }))
        }
        "dialog.openFile" => {
            use tauri_plugin_dialog::DialogExt;
            let title = params
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Выбор файла")
                .to_string();
            let multiple = params
                .get("multiple")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let mut builder = app.dialog().file().set_title(&title);
            if let Some(default_path) = params.get("defaultPath").and_then(|v| v.as_str()) {
                builder = builder.set_directory(std::path::PathBuf::from(default_path));
            }
            if let Some(filters) = params.get("filters").and_then(|v| v.as_array()) {
                for f in filters {
                    let name = f.get("name").and_then(|v| v.as_str()).unwrap_or("filter");
                    let exts: Vec<&str> = f
                        .get("extensions")
                        .and_then(|v| v.as_array())
                        .map(|a| a.iter().filter_map(|s| s.as_str()).collect())
                        .unwrap_or_default();
                    builder = builder.add_filter(name, &exts);
                }
            }
            if multiple {
                let (tx, rx) = tokio::sync::oneshot::channel();
                builder.pick_files(move |paths| {
                    let _ = tx.send(paths);
                });
                let picked = rx.await.map_err(|e| e.to_string())?;
                let paths: Vec<String> = picked
                    .unwrap_or_default()
                    .into_iter()
                    .map(|p| p.to_string())
                    .collect();
                Ok(serde_json::json!({ "paths": paths }))
            } else {
                let (tx, rx) = tokio::sync::oneshot::channel();
                builder.pick_file(move |path| {
                    let _ = tx.send(path);
                });
                let picked = rx.await.map_err(|e| e.to_string())?;
                Ok(serde_json::json!({
                    "path": picked.map(|p| p.to_string()),
                }))
            }
        }
        "dialog.saveFile" => {
            use tauri_plugin_dialog::DialogExt;
            let title = params
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Сохранить как")
                .to_string();
            let mut builder = app.dialog().file().set_title(&title);
            if let Some(default_path) = params.get("defaultPath").and_then(|v| v.as_str()) {
                let pb = std::path::PathBuf::from(default_path);
                if let Some(parent) = pb.parent() {
                    if !parent.as_os_str().is_empty() {
                        builder = builder.set_directory(parent);
                    }
                }
                if let Some(name) = pb.file_name().and_then(|n| n.to_str()) {
                    builder = builder.set_file_name(name);
                }
            }
            if let Some(filters) = params.get("filters").and_then(|v| v.as_array()) {
                for f in filters {
                    let name = f.get("name").and_then(|v| v.as_str()).unwrap_or("filter");
                    let exts: Vec<&str> = f
                        .get("extensions")
                        .and_then(|v| v.as_array())
                        .map(|a| a.iter().filter_map(|s| s.as_str()).collect())
                        .unwrap_or_default();
                    builder = builder.add_filter(name, &exts);
                }
            }
            let (tx, rx) = tokio::sync::oneshot::channel();
            builder.save_file(move |path| {
                let _ = tx.send(path);
            });
            let picked = rx.await.map_err(|e| e.to_string())?;
            let path_str = picked.as_ref().map(|p| p.to_string());
            if let (Some(p), Some(content)) =
                (picked.as_ref(), params.get("content").and_then(|v| v.as_str()))
            {
                if let Some(fs_path) = p.as_path() {
                    std::fs::write(fs_path, content).map_err(|e| e.to_string())?;
                } else {
                    return Err("save target not a local path".into());
                }
            }
            Ok(serde_json::json!({ "path": path_str }))
        }
        "agent.stream" => {
            let prompt = params
                .get("prompt")
                .and_then(|v| v.as_str())
                .ok_or("missing prompt")?
                .to_string();
            let sandbox = params
                .get("sandbox")
                .and_then(|v| v.as_str())
                .unwrap_or("read-only")
                .to_string();
            let cwd_str = params.get("cwd").and_then(|v| v.as_str());
            let cwd_path = match cwd_str {
                Some(p) => PathBuf::from(p),
                None => apps::app_dir(app, app_id).map_err(|e| e.to_string())?,
            };

            let handle = app.state::<app_server::AppServerHandle>();
            let server = handle.wait().await;
            let app_thread_id = server
                .thread_start(&cwd_path, &sandbox, None)
                .await
                .map_err(|e| format!("thread_start: {e}"))?;

            let mut rx = server.subscribe_stream(&app_thread_id);
            let stream_id = format!("s_{}_{}", app_id, crate::uuid_like());
            let app_emit = app.clone();
            let stream_id_for_task = stream_id.clone();
            let app_id_for_task = app_id.to_string();
            let app_thread_id_for_task = app_thread_id.clone();
            let server_for_task = server.clone();
            tauri::async_runtime::spawn(async move {
                while let Some(ev) = rx.recv().await {
                    match ev {
                        app_server::StreamEvent::Delta(token) => {
                            let _ = app_emit.emit(
                                "reflex://app-stream-token",
                                &serde_json::json!({
                                    "stream_id": stream_id_for_task,
                                    "app_id": app_id_for_task,
                                    "token": token,
                                }),
                            );
                        }
                        app_server::StreamEvent::Done(full) => {
                            let _ = app_emit.emit(
                                "reflex://app-stream-done",
                                &serde_json::json!({
                                    "stream_id": stream_id_for_task,
                                    "app_id": app_id_for_task,
                                    "result": full,
                                }),
                            );
                            break;
                        }
                    }
                }
                server_for_task.unsubscribe_stream(&app_thread_id_for_task);
            });

            let server_kick = server.clone();
            let thread_for_kick = app_thread_id.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = server_kick.turn_start(&thread_for_kick, &prompt).await {
                    eprintln!("[reflex] agent.stream turn_start failed: {e:?}");
                }
            });

            Ok(serde_json::json!({
                "streamId": stream_id,
                "threadId": app_thread_id,
            }))
        }
        "agent.streamAbort" => {
            let stream_thread = params
                .get("threadId")
                .and_then(|v| v.as_str())
                .ok_or("missing threadId")?;
            let handle = app.state::<app_server::AppServerHandle>();
            let server = handle.wait().await;
            server.unsubscribe_stream(stream_thread);
            Ok(serde_json::json!({ "ok": true }))
        }
        "agent.task" => {
            let prompt = params
                .get("prompt")
                .and_then(|v| v.as_str())
                .ok_or("missing prompt")?;
            let sandbox = params
                .get("sandbox")
                .and_then(|v| v.as_str())
                .unwrap_or("read-only")
                .to_string();
            let cwd_str = params.get("cwd").and_then(|v| v.as_str());
            let cwd_path = match cwd_str {
                Some(p) => PathBuf::from(p),
                None => apps::app_dir(app, app_id).map_err(|e| e.to_string())?,
            };

            let handle = app.state::<app_server::AppServerHandle>();
            let server = handle.wait().await;
            let app_thread_id = server
                .thread_start(&cwd_path, &sandbox, None)
                .await
                .map_err(|e| format!("thread_start: {e}"))?;
            let _ = server.turn_start(&app_thread_id, prompt).await;
            let turn = server.wait_for_turn(&app_thread_id).await;
            let result_text = turn
                .as_ref()
                .and_then(|t| {
                    t.get("lastAgentMessage")
                        .or_else(|| t.get("last_agent_message"))
                })
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            Ok(serde_json::json!({
                "threadId": app_thread_id,
                "result": result_text,
            }))
        }
        "net.fetch" => {
            let url_str = params
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or("missing url")?;
            let parsed_url = reqwest::Url::parse(url_str).map_err(|e| format!("invalid url: {e}"))?;
            let host = parsed_url
                .host_str()
                .ok_or("url has no host")?
                .to_string();

            let manifest = apps::read_manifest(app, app_id).map_err(|e| e.to_string())?;
            let policy = manifest
                .network
                .ok_or_else(|| "manifest.network is missing — declare allowed_hosts".to_string())?;
            if !policy.allows_host(&host) {
                return Err(format!(
                    "host '{host}' not in manifest.network.allowed_hosts"
                ));
            }

            let method_str = params
                .get("method")
                .and_then(|v| v.as_str())
                .unwrap_or("GET")
                .to_uppercase();
            let m = reqwest::Method::from_bytes(method_str.as_bytes())
                .map_err(|e| format!("invalid method: {e}"))?;
            let timeout_ms = params
                .get("timeoutMs")
                .and_then(|v| v.as_u64())
                .unwrap_or(30_000);

            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_millis(timeout_ms))
                .build()
                .map_err(|e| format!("client build: {e}"))?;
            let mut req = client.request(m, parsed_url);

            if let Some(headers) = params.get("headers").and_then(|v| v.as_object()) {
                let mut header_map = reqwest::header::HeaderMap::new();
                for (k, v) in headers {
                    let val = v.as_str().ok_or("header value must be string")?;
                    let name = reqwest::header::HeaderName::from_bytes(k.as_bytes())
                        .map_err(|e| format!("invalid header {k}: {e}"))?;
                    let value = reqwest::header::HeaderValue::from_str(val)
                        .map_err(|e| format!("invalid header value for {k}: {e}"))?;
                    header_map.insert(name, value);
                }
                req = req.headers(header_map);
            }

            if let Some(body) = params.get("body") {
                if let Some(s) = body.as_str() {
                    req = req.body(s.to_string());
                } else if !body.is_null() {
                    let json = serde_json::to_string(body).map_err(|e| e.to_string())?;
                    req = req.header("content-type", "application/json").body(json);
                }
            }

            let resp = req.send().await.map_err(|e| format!("fetch failed: {e}"))?;
            let status_code = resp.status().as_u16();
            let mut headers_out = serde_json::Map::new();
            for (k, v) in resp.headers().iter() {
                if let Ok(s) = v.to_str() {
                    headers_out.insert(k.as_str().to_string(), serde_json::Value::String(s.into()));
                }
            }
            let bytes = resp.bytes().await.map_err(|e| format!("read body: {e}"))?;
            const MAX_BODY: usize = 10 * 1024 * 1024;
            if bytes.len() > MAX_BODY {
                return Err(format!(
                    "response too large: {} bytes (limit 10MB)",
                    bytes.len()
                ));
            }
            let (body_value, encoding) = match std::str::from_utf8(&bytes) {
                Ok(s) => (serde_json::Value::String(s.to_string()), "utf8"),
                Err(_) => {
                    use base64::Engine;
                    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                    (serde_json::Value::String(b64), "base64")
                }
            };
            Ok(serde_json::json!({
                "status": status_code,
                "headers": headers_out,
                "body": body_value,
                "encoding": encoding,
            }))
        }
        "events.emit" => {
            let topic = params
                .get("topic")
                .and_then(|v| v.as_str())
                .ok_or("missing topic")?;
            let payload = params.get("payload").cloned().unwrap_or(serde_json::Value::Null);
            let bus = app.state::<memory::MemoryState>().bus.clone();
            app_bus::emit_event(&bus, app_id, topic, payload).await?;
            Ok(serde_json::json!({ "ok": true }))
        }
        "events.subscribe" => {
            let topics = parse_topics(&params)?;
            let bridge: AppBusBridge =
                app.state::<AppBusBridge>().inner().clone();
            bridge.subscribe(app_id, &topics);
            Ok(serde_json::json!({ "ok": true, "topics": topics }))
        }
        "events.unsubscribe" => {
            let topics = parse_topics(&params)?;
            let bridge: AppBusBridge =
                app.state::<AppBusBridge>().inner().clone();
            bridge.unsubscribe(app_id, &topics);
            Ok(serde_json::json!({ "ok": true }))
        }
        "events.clearSubscriptions" => {
            let bridge: AppBusBridge =
                app.state::<AppBusBridge>().inner().clone();
            bridge.clear(app_id);
            Ok(serde_json::json!({ "ok": true }))
        }
        "apps.invoke" => {
            let target_id = params
                .get("app_id")
                .or_else(|| params.get("appId"))
                .and_then(|v| v.as_str())
                .ok_or("missing app_id")?
                .to_string();
            let action_id = params
                .get("action_id")
                .or_else(|| params.get("actionId"))
                .and_then(|v| v.as_str())
                .ok_or("missing action_id")?
                .to_string();
            let action_params = params
                .get("params")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            invoke_app_action(app, app_id, &target_id, &action_id, action_params).await
        }
        "apps.list_actions" => {
            let target_id = params
                .get("app_id")
                .or_else(|| params.get("appId"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let include_steps = params
                .get("include_steps")
                .or_else(|| params.get("includeSteps"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            list_actions(app, target_id.as_deref(), include_steps)
        }
        "memory.save" => memory_save_for_app(app, app_id, params).await,
        "memory.list" => memory_list_for_app(app, app_id, params),
        "memory.delete" => memory_delete_for_app(app, app_id, params),
        "memory.search" => memory_search_for_app(app, app_id, params).await,
        "memory.recall" => memory_recall_for_app(app, app_id, params).await,
        "memory.indexPath" | "memory.index_path" => {
            memory_index_path_for_app(app, app_id, params).await
        }
        "memory.pathStatus" | "memory.path_status" => {
            memory_path_status_for_app(app, app_id, params)
        }
        "memory.forgetPath" | "memory.forget_path" => {
            memory_forget_path_for_app(app, app_id, params).await
        }
        other => Err(format!("unknown method: {other}")),
    }
}

fn system_context(app: &AppHandle, app_id: &str) -> Result<serde_json::Value, String> {
    let app_root = apps::app_dir(app, app_id).map_err(|e| e.to_string())?;
    let manifest = apps::read_manifest(app, app_id).ok();
    let app_project = project::find_project_for(&app_root);
    let linked_projects = linked_projects_for_app(app, app_id)?;
    Ok(serde_json::json!({
        "app_id": app_id,
        "app_root": app_root.to_string_lossy(),
        "manifest": manifest,
        "app_project": app_project,
        "linked_projects": linked_projects,
        "memory_defaults": {
            "project_id": default_memory_project(app, app_id)?.map(|p| p.id),
            "scope": "project",
        },
    }))
}

fn linked_projects_for_app(
    app: &AppHandle,
    app_id: &str,
) -> Result<Vec<project::Project>, String> {
    let app_root = apps::app_dir(app, app_id).map_err(|e| e.to_string())?;
    let app_root_canon = app_root.canonicalize().ok();
    let mut out = Vec::new();
    for p in project::list_registered(app).map_err(|e| e.to_string())? {
        if !p.apps.iter().any(|id| id == app_id) {
            continue;
        }
        if let Some(root) = &app_root_canon {
            if PathBuf::from(&p.root)
                .canonicalize()
                .map(|candidate| candidate == *root)
                .unwrap_or(false)
            {
                continue;
            }
        }
        out.push(p);
    }
    Ok(out)
}

fn default_memory_project(
    app: &AppHandle,
    app_id: &str,
) -> Result<Option<project::Project>, String> {
    let linked = linked_projects_for_app(app, app_id)?;
    if linked.len() == 1 {
        return Ok(linked.into_iter().next());
    }
    let app_root = apps::app_dir(app, app_id).map_err(|e| e.to_string())?;
    Ok(project::find_project_for(&app_root))
}

fn app_has_permission(app: &AppHandle, app_id: &str, permission: &str) -> bool {
    let manifest = match apps::read_manifest(app, app_id) {
        Ok(m) => m,
        Err(_) => return false,
    };
    manifest.permissions.iter().any(|p| {
        p == "*"
            || p == permission
            || (permission.starts_with("memory.") && p == "memory:*")
            || (permission.starts_with("memory.global.") && p == "memory.global")
            || (permission.starts_with("memory.project.") && p == "memory.project:*")
    })
}

fn ensure_global_memory_permission(
    app: &AppHandle,
    app_id: &str,
    write: bool,
) -> Result<(), String> {
    let specific = if write {
        "memory.global.write"
    } else {
        "memory.global.read"
    };
    if app_has_permission(app, app_id, specific) {
        Ok(())
    } else {
        Err(format!(
            "permission denied: global memory requires manifest.permissions entry '{specific}'"
        ))
    }
}

#[derive(Clone)]
struct MemoryTarget {
    root: PathBuf,
    thread_id: Option<String>,
}

fn resolve_memory_target(
    app: &AppHandle,
    app_id: &str,
    params: &serde_json::Value,
) -> Result<MemoryTarget, String> {
    let thread_id = params
        .get("thread_id")
        .or_else(|| params.get("threadId"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    if let Some(project_id) = params
        .get("project_id")
        .or_else(|| params.get("projectId"))
        .and_then(|v| v.as_str())
    {
        let project = project::get_by_id(app, project_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("project not found: {project_id}"))?;
        ensure_project_memory_access(app, app_id, &project)?;
        return Ok(MemoryTarget {
            root: PathBuf::from(&project.root),
            thread_id,
        });
    }

    if let Some(project_root) = params
        .get("project_root")
        .or_else(|| params.get("projectRoot"))
        .and_then(|v| v.as_str())
    {
        let root = PathBuf::from(project_root);
        let matched = project::list_registered(app)
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|p| same_path(Path::new(&p.root), &root));
        if let Some(project) = matched {
            ensure_project_memory_access(app, app_id, &project)?;
            return Ok(MemoryTarget {
                root: PathBuf::from(&project.root),
                thread_id,
            });
        }
        let app_root = apps::app_dir(app, app_id).map_err(|e| e.to_string())?;
        if !same_path(&app_root, &root) && !app_has_permission(app, app_id, "memory.project:*") {
            return Err(
                "permission denied: project_root must be this app root, a linked project, or require memory.project:*"
                    .into(),
            );
        }
        return Ok(MemoryTarget {
            root,
            thread_id,
        });
    }

    if let Some(project) = default_memory_project(app, app_id)? {
        return Ok(MemoryTarget {
            root: PathBuf::from(&project.root),
            thread_id,
        });
    }

    Ok(MemoryTarget {
        root: apps::app_dir(app, app_id).map_err(|e| e.to_string())?,
        thread_id,
    })
}

fn ensure_project_memory_access(
    app: &AppHandle,
    app_id: &str,
    target: &project::Project,
) -> Result<(), String> {
    let app_root = apps::app_dir(app, app_id).map_err(|e| e.to_string())?;
    if same_path(&app_root, Path::new(&target.root)) {
        return Ok(());
    }
    if target.apps.iter().any(|id| id == app_id) {
        return Ok(());
    }
    if app_has_permission(app, app_id, "memory.project:*") {
        return Ok(());
    }
    Err(format!(
        "permission denied: app '{app_id}' is not linked to project '{}'",
        target.id
    ))
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn scope_roots(target: &MemoryTarget) -> Result<ScopeRoots, String> {
    let home = std::env::var("HOME")
        .map(PathBuf::from)
        .map_err(|e| format!("HOME not set: {e}"))?;
    Ok(ScopeRoots::resolve(
        &home,
        Some(&target.root),
        target.thread_id.as_deref(),
    ))
}

fn parse_scope(
    params: &serde_json::Value,
    default_scope: MemoryScope,
) -> Result<MemoryScope, String> {
    match params.get("scope") {
        Some(v) => serde_json::from_value(v.clone()).map_err(|e| e.to_string()),
        None => Ok(default_scope),
    }
}

fn parse_kind(
    params: &serde_json::Value,
    default_kind: MemoryKind,
) -> Result<MemoryKind, String> {
    match params.get("kind").or_else(|| params.get("type")) {
        Some(v) => serde_json::from_value(v.clone()).map_err(|e| e.to_string()),
        None => Ok(default_kind),
    }
}

fn parse_list_filter(params: &serde_json::Value) -> Result<ListFilter, String> {
    let Some(value) = params.get("filter") else {
        return Ok(ListFilter::default());
    };
    if value.is_null() {
        return Ok(ListFilter::default());
    }
    #[derive(serde::Deserialize, Default)]
    struct RawFilter {
        kind: Option<MemoryKind>,
        tag: Option<String>,
        query: Option<String>,
    }
    let raw: RawFilter = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
    Ok(ListFilter {
        kind: raw.kind,
        tag: raw.tag,
        query: raw.query,
    })
}

async fn memory_save_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let scope = parse_scope(&params, MemoryScope::Project)?;
    if scope == MemoryScope::Global {
        ensure_global_memory_permission(app, app_id, true)?;
    }
    let target = resolve_memory_target(app, app_id, &params)?;
    let roots = scope_roots(&target)?;
    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or("missing name")?
        .trim()
        .to_string();
    if name.is_empty() {
        return Err("name must be non-empty".into());
    }
    let body = params
        .get("body")
        .and_then(|v| v.as_str())
        .ok_or("missing body")?
        .to_string();
    let description = params
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let tags = params
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let source = params
        .get("source")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| Some(format!("app:{app_id}")));
    let req = SaveRequest {
        scope,
        kind: parse_kind(&params, MemoryKind::Fact)?,
        name,
        description,
        body: body.clone(),
        rel_path: None,
        tags,
        source,
    };
    let note = store::save(&roots, req).map_err(|e| e.to_string())?;
    if scope != MemoryScope::Global {
        let doc_id = format!("memory:{}", note.rel_path.display());
        let root = target.root.clone();
        tokio::spawn(async move {
            let _ = rag::index_text(&root, &doc_id, "memory", &body).await;
        });
    }
    Ok(serde_json::to_value(note).unwrap_or(serde_json::Value::Null))
}

fn memory_list_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let scope = parse_scope(&params, MemoryScope::Project)?;
    if scope == MemoryScope::Global {
        ensure_global_memory_permission(app, app_id, false)?;
    }
    let target = resolve_memory_target(app, app_id, &params)?;
    let roots = scope_roots(&target)?;
    let filter = parse_list_filter(&params)?;
    let notes = store::list(&roots, scope, &filter).map_err(|e| e.to_string())?;
    Ok(serde_json::to_value(notes).unwrap_or(serde_json::Value::Array(vec![])))
}

fn memory_delete_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let scope = parse_scope(&params, MemoryScope::Project)?;
    if scope == MemoryScope::Global {
        ensure_global_memory_permission(app, app_id, true)?;
    }
    let rel_path = params
        .get("rel_path")
        .or_else(|| params.get("relPath"))
        .and_then(|v| v.as_str())
        .ok_or("missing rel_path")?;
    let target = resolve_memory_target(app, app_id, &params)?;
    let roots = scope_roots(&target)?;
    store::delete(&roots, scope, Path::new(rel_path)).map_err(|e| e.to_string())?;
    if scope != MemoryScope::Global {
        let doc_id = format!("memory:{rel_path}");
        let root = target.root.clone();
        tokio::spawn(async move {
            let _ = rag::forget(&root, &doc_id).await;
        });
    }
    Ok(serde_json::json!({ "ok": true }))
}

async fn memory_search_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let query = params
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or("missing query")?;
    let limit = params
        .get("limit")
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .unwrap_or(8);
    let target = resolve_memory_target(app, app_id, &params)?;
    let hits = rag::search(&target.root, query, limit)
        .await
        .map_err(|e| e.to_string())?;
    Ok(serde_json::to_value(hits).unwrap_or(serde_json::Value::Array(vec![])))
}

async fn memory_recall_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let query = params
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or("missing query")?;
    let target = resolve_memory_target(app, app_id, &params)?;
    let thread_id = target
        .thread_id
        .clone()
        .unwrap_or_else(|| format!("app:{app_id}"));
    let req = RecallRequest {
        project_root: target.root.to_string_lossy().into_owned(),
        thread_id,
        query: query.to_string(),
        max_notes: params
            .get("max_notes")
            .or_else(|| params.get("maxNotes"))
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(8),
        max_rag: params
            .get("max_rag")
            .or_else(|| params.get("maxRag"))
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(6),
    };
    let result = recall::recall(req).await.map_err(|e| e.to_string())?;
    Ok(serde_json::to_value(result).unwrap_or(serde_json::Value::Null))
}

fn resolve_project_path(target: &MemoryTarget, raw: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(raw);
    let candidate = if path.is_absolute() {
        path
    } else {
        target.root.join(path)
    };
    let root = target
        .root
        .canonicalize()
        .map_err(|e| format!("canonicalize project root: {e}"))?;
    let canonical = if candidate.exists() {
        candidate
            .canonicalize()
            .map_err(|e| format!("canonicalize path: {e}"))?
    } else {
        let parent = candidate
            .parent()
            .ok_or_else(|| "path has no parent".to_string())?
            .canonicalize()
            .map_err(|e| format!("canonicalize parent: {e}"))?;
        parent.join(candidate.file_name().unwrap_or_default())
    };
    if !canonical.starts_with(&root) {
        return Err("path must be inside the selected project root".into());
    }
    Ok(canonical)
}

async fn memory_index_path_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let raw_path = params
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or("missing path")?;
    let target = resolve_memory_target(app, app_id, &params)?;
    let path = resolve_project_path(&target, raw_path)?;
    let outcome = files::index_path(&target.root, &path)
        .await
        .map_err(|e| e.to_string())?;
    Ok(serde_json::to_value(outcome).unwrap_or(serde_json::Value::Null))
}

fn memory_path_status_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let raw_path = params
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or("missing path")?;
    let target = resolve_memory_target(app, app_id, &params)?;
    let path = resolve_project_path(&target, raw_path)?;
    let status = files::status(&target.root, &path).map_err(|e| e.to_string())?;
    Ok(serde_json::to_value(status).unwrap_or(serde_json::Value::Null))
}

async fn memory_forget_path_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let raw_path = params
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or("missing path")?;
    let target = resolve_memory_target(app, app_id, &params)?;
    let path = resolve_project_path(&target, raw_path)?;
    let removed = files::forget_path(&target.root, &path)
        .await
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "forgotten": removed }))
}

fn parse_topics(params: &serde_json::Value) -> Result<Vec<String>, String> {
    let arr = params
        .get("topics")
        .and_then(|v| v.as_array())
        .ok_or("missing topics array")?;
    let mut out = Vec::with_capacity(arr.len());
    for t in arr {
        if let Some(s) = t.as_str() {
            out.push(s.to_string());
        }
    }
    if out.is_empty() {
        return Err("topics must be non-empty array of strings".into());
    }
    Ok(out)
}

async fn invoke_app_action(
    app: &AppHandle,
    caller_id: &str,
    target_id: &str,
    action_id: &str,
    action_params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let target_manifest = apps::read_manifest(app, target_id).map_err(|e| e.to_string())?;
    let action = target_manifest
        .actions
        .iter()
        .find(|a| a.id == action_id)
        .cloned()
        .ok_or_else(|| format!("action not found: {target_id}::{action_id}"))?;

    let allowed = action.public || caller_has_invoke_permission(app, caller_id, target_id, action_id);
    if !allowed {
        return Err(format!(
            "permission denied: {target_id}::{action_id} is not public and caller '{caller_id}' lacks 'apps.invoke:{target_id}' permission"
        ));
    }

    let handle: scheduler::SchedulerHandle = app
        .state::<scheduler::SchedulerHandle>()
        .inner()
        .clone();
    let fut = scheduler::runner::run_workflow(
        app.clone(),
        handle,
        target_id.to_string(),
        scheduler::runner::WorkflowCaller::InterApp {
            from: caller_id.to_string(),
            action_id: action_id.to_string(),
        },
        action.steps.clone(),
        Some(action_params),
    );
    let record = Box::pin(fut).await;

    if record.status == "ok" {
        Ok(serde_json::json!({
            "ok": true,
            "run_id": record.run_id,
            "result": scheduler::runner::last_step_value(&record),
        }))
    } else {
        Err(record.error.clone().unwrap_or_else(|| "workflow failed".into()))
    }
}

fn caller_has_invoke_permission(
    app: &AppHandle,
    caller_id: &str,
    target_id: &str,
    action_id: &str,
) -> bool {
    let manifest = match apps::read_manifest(app, caller_id) {
        Ok(m) => m,
        Err(_) => return false,
    };
    let needed = [
        "apps.invoke:*".to_string(),
        format!("apps.invoke:{target_id}"),
        format!("apps.invoke:{target_id}::{action_id}"),
    ];
    manifest.permissions.iter().any(|p| needed.contains(p))
}

fn list_actions(
    app: &AppHandle,
    target_id: Option<&str>,
    include_steps: bool,
) -> Result<serde_json::Value, String> {
    let listings = apps::list_apps(app).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for l in listings {
        if let Some(t) = target_id {
            if l.manifest.id != t {
                continue;
            }
        }
        let mut actions_json = Vec::with_capacity(l.manifest.actions.len());
        for a in &l.manifest.actions {
            let mut item = serde_json::json!({
                "id": a.id,
                "name": a.name,
                "description": a.description,
                "public": a.public,
                "steps_count": a.steps.len(),
            });
            if include_steps {
                item["steps"] = serde_json::to_value(&a.steps).unwrap_or(serde_json::Value::Null);
            }
            actions_json.push(item);
        }
        out.push(serde_json::json!({
            "app_id": l.manifest.id,
            "app_name": l.manifest.name,
            "actions": actions_json,
        }));
    }
    Ok(serde_json::Value::Array(out))
}
