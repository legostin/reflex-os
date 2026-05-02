use crate::app_bus::{self, AppBusBridge};
use crate::app_server;
use crate::apps;
use crate::memory::agents::recall::{self, RecallRequest};
use crate::memory::files;
use crate::memory::rag;
use crate::memory::schema::{MemoryKind, MemoryScope, ScopeRoots};
use crate::memory::store::{self, ListFilter, SaveRequest};
use crate::{browser, memory, project, storage};
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
        "system.openUrl" | "system.open_url" => system_open_url(app, &params),
        "system.openPath" | "system.open_path" => system_open_path(app, app_id, &params),
        "system.revealPath" | "system.reveal_path" => system_reveal_path(app, app_id, &params),
        "clipboard.readText" | "clipboard.read_text" => clipboard_read_text(app, app_id),
        "clipboard.writeText" | "clipboard.write_text" => {
            clipboard_write_text(app, app_id, &params)
        }
        "manifest.get" => manifest_get(app, app_id),
        "manifest.update" => manifest_update(app, app_id, params),
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
            if !store.is_object() {
                store = serde_json::json!({});
            }
            store
                .as_object_mut()
                .expect("storage object")
                .insert(key.to_string(), value);
            apps::write_storage(app, app_id, &store).map_err(|e| e.to_string())?;
            Ok(serde_json::json!({ "ok": true }))
        }
        "storage.list" => {
            let prefix = string_param(&params, "prefix", "prefix");
            let store = apps::read_storage(app, app_id).map_err(|e| e.to_string())?;
            let mut keys: Vec<String> = store
                .as_object()
                .map(|obj| {
                    obj.keys()
                        .filter(|key| {
                            prefix
                                .as_ref()
                                .map(|prefix| key.starts_with(prefix))
                                .unwrap_or(true)
                        })
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            keys.sort();

            let mut entries = serde_json::Map::new();
            if let Some(obj) = store.as_object() {
                for key in &keys {
                    if let Some(value) = obj.get(key) {
                        entries.insert(key.clone(), value.clone());
                    }
                }
            }

            Ok(serde_json::json!({ "keys": keys, "entries": entries }))
        }
        "storage.delete" => {
            let keys = parse_storage_keys(&params)?;
            let mut store = apps::read_storage(app, app_id).map_err(|e| e.to_string())?;
            if !store.is_object() {
                store = serde_json::json!({});
            }
            let obj = store.as_object_mut().expect("storage object");
            let mut deleted = Vec::new();
            let mut missing = Vec::new();
            for key in keys {
                if obj.remove(&key).is_some() {
                    deleted.push(key);
                } else {
                    missing.push(key);
                }
            }
            apps::write_storage(app, app_id, &store).map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "ok": true,
                "deleted": deleted,
                "missing": missing,
            }))
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
        "fs.list" => {
            let path = string_param(&params, "path", "path").unwrap_or_default();
            let recursive = bool_param(&params, "recursive", "recursive").unwrap_or(false);
            let include_hidden =
                bool_param(&params, "include_hidden", "includeHidden").unwrap_or(false);
            let entries = apps::list_app_files(app, app_id, &path, recursive, include_hidden)
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({ "entries": entries }))
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
        "fs.delete" => {
            let path = params
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or("missing path")?;
            let recursive = bool_param(&params, "recursive", "recursive").unwrap_or(false);
            let kind =
                apps::delete_app_path(app, app_id, path, recursive).map_err(|e| e.to_string())?;
            Ok(serde_json::json!({ "ok": true, "path": path, "kind": kind }))
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
            let cwd_target =
                resolve_agent_cwd_for_app(app, app_id, params.get("cwd").and_then(|v| v.as_str()))?;

            let handle = app.state::<app_server::AppServerHandle>();
            let server = handle.wait().await;
            let app_thread_id = server
                .thread_start(&cwd_target.cwd, &sandbox, cwd_target.mcp_servers.as_ref())
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
            let cwd_target =
                resolve_agent_cwd_for_app(app, app_id, params.get("cwd").and_then(|v| v.as_str()))?;

            let handle = app.state::<app_server::AppServerHandle>();
            let server = handle.wait().await;
            let app_thread_id = server
                .thread_start(&cwd_target.cwd, &sandbox, cwd_target.mcp_servers.as_ref())
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
        "apps.list" => list_app_summaries(app),
        "apps.open" => {
            let target_id = params
                .get("app_id")
                .or_else(|| params.get("appId"))
                .and_then(|v| v.as_str())
                .ok_or("missing app_id")?;
            apps::read_manifest(app, target_id).map_err(|e| e.to_string())?;
            app.emit(
                "reflex://app-open-request",
                &serde_json::json!({
                    "app_id": target_id,
                    "from_app": app_id,
                }),
            )
            .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({ "ok": true, "app_id": target_id }))
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
        "projects.list" => projects_list_for_app(app, app_id, params),
        "projects.open" => projects_open_for_app(app, app_id, params),
        "topics.list" | "threads.list" => topics_list_for_app(app, app_id, params),
        "topics.open" | "threads.open" => topics_open_for_app(app, app_id, params),
        "skills.list" => skills_list_for_app(app, app_id, params),
        "mcp.servers" | "mcp.list" => mcp_servers_for_app(app, app_id, params),
        "browser.init" => browser_init_for_app(app, app_id, params).await,
        "browser.tabs.list" | "browser.tabsList" => {
            ensure_browser_permission(app, app_id, "read")?;
            browser::browser_tabs_list(app.clone()).await
        }
        "browser.tab.open" | "browser.open" => browser_open_for_app(app, app_id, params).await,
        "browser.navigate" => browser_navigate_for_app(app, app_id, params).await,
        "browser.readText" | "browser.read_text" => {
            browser_read_text_for_app(app, app_id, params).await
        }
        "browser.readOutline" | "browser.read_outline" => {
            browser_read_outline_for_app(app, app_id, params).await
        }
        "browser.screenshot" => browser_screenshot_for_app(app, app_id, params).await,
        "browser.clickText" | "browser.click_text" => {
            browser_click_text_for_app(app, app_id, params).await
        }
        "browser.clickSelector" | "browser.click_selector" => {
            browser_click_selector_for_app(app, app_id, params).await
        }
        "browser.fill" => browser_fill_for_app(app, app_id, params).await,
        "scheduler.list" => scheduler_list_for_app(app, app_id, params),
        "scheduler.runNow" | "scheduler.run_now" => {
            scheduler_run_now_for_app(app, app_id, params).await
        }
        "scheduler.setPaused" | "scheduler.set_paused" => {
            scheduler_set_paused_for_app(app, app_id, params).await
        }
        "scheduler.runs" => scheduler_runs_for_app(app, app_id, params),
        "scheduler.runDetail" | "scheduler.run_detail" => {
            scheduler_run_detail_for_app(app, app_id, params)
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

fn manifest_get(app: &AppHandle, app_id: &str) -> Result<serde_json::Value, String> {
    let manifest = apps::read_manifest(app, app_id).map_err(|e| e.to_string())?;
    Ok(serde_json::to_value(manifest).unwrap_or(serde_json::Value::Null))
}

fn manifest_update(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let current = apps::read_manifest(app, app_id).map_err(|e| e.to_string())?;
    let mut value = serde_json::to_value(current).map_err(|e| e.to_string())?;
    let patch = params
        .get("patch")
        .or_else(|| params.get("manifest"))
        .cloned()
        .ok_or("missing patch")?;
    if !patch.is_object() {
        return Err("manifest patch must be a JSON object".into());
    }
    merge_json(&mut value, patch);
    value["id"] = serde_json::Value::String(app_id.to_string());
    let mut manifest: apps::AppManifest =
        serde_json::from_value(value).map_err(|e| format!("invalid manifest: {e}"))?;
    manifest.id = app_id.to_string();

    apps::write_manifest(app, app_id, &manifest).map_err(|e| e.to_string())?;
    let _ = app.emit("reflex://apps-changed", &serde_json::json!({}));
    if let Some(handle) = app.try_state::<scheduler::SchedulerHandle>() {
        handle.inner().rescan();
    }

    Ok(serde_json::json!({
        "ok": true,
        "manifest": manifest,
    }))
}

fn merge_json(base: &mut serde_json::Value, patch: serde_json::Value) {
    match (base, patch) {
        (serde_json::Value::Object(base_obj), serde_json::Value::Object(patch_obj)) => {
            for (key, value) in patch_obj {
                match base_obj.get_mut(&key) {
                    Some(existing) => merge_json(existing, value),
                    None => {
                        base_obj.insert(key, value);
                    }
                }
            }
        }
        (base_slot, value) => {
            *base_slot = value;
        }
    }
}

fn system_context(app: &AppHandle, app_id: &str) -> Result<serde_json::Value, String> {
    let app_root = apps::app_dir(app, app_id).map_err(|e| e.to_string())?;
    let manifest = apps::read_manifest(app, app_id).ok();
    let app_project =
        project::find_project_for(&app_root).map(|project| project_summary(&project));
    let linked_projects: Vec<serde_json::Value> = linked_projects_for_app(app, app_id)?
        .into_iter()
        .map(|project| project_summary(&project))
        .collect();
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

fn system_open_url(
    app: &AppHandle,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let url = required_string_param(params, "url", "url")?;
    let lower = url.to_ascii_lowercase();
    if !["http://", "https://", "mailto:", "tel:"]
        .iter()
        .any(|prefix| lower.starts_with(prefix))
    {
        return Err("unsupported url scheme: use http, https, mailto, or tel".into());
    }

    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(url.clone(), None::<String>)
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "ok": true, "url": url }))
}

fn system_open_path(
    app: &AppHandle,
    app_id: &str,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let path = resolve_system_path(app, app_id, params)?;
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(path.to_string_lossy().to_string(), None::<String>)
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "ok": true, "path": path.to_string_lossy() }))
}

fn system_reveal_path(
    app: &AppHandle,
    app_id: &str,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let path = resolve_system_path(app, app_id, params)?;
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .reveal_item_in_dir(&path)
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "ok": true, "path": path.to_string_lossy() }))
}

fn resolve_system_path(
    app: &AppHandle,
    app_id: &str,
    params: &serde_json::Value,
) -> Result<PathBuf, String> {
    let raw = required_string_param(params, "path", "path")?;
    let path = PathBuf::from(&raw);
    let path = if path.is_absolute() {
        path
    } else {
        apps::app_dir(app, app_id)
            .map_err(|e| e.to_string())?
            .join(path)
    };

    if !path.exists() {
        return Err(format!("path not found: {}", path.to_string_lossy()));
    }

    Ok(path)
}

fn clipboard_read_text(app: &AppHandle, app_id: &str) -> Result<serde_json::Value, String> {
    ensure_clipboard_permission(app, app_id, "read")?;
    let output = std::process::Command::new("pbpaste")
        .output()
        .map_err(|e| format!("pbpaste failed: {e}"))?;
    if !output.status.success() {
        return Err(format!("pbpaste exited with status {}", output.status));
    }
    let text = String::from_utf8(output.stdout).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "text": text }))
}

fn clipboard_write_text(
    app: &AppHandle,
    app_id: &str,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    ensure_clipboard_permission(app, app_id, "write")?;
    let text = params
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or("missing text")?;
    let mut child = std::process::Command::new("pbcopy")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("pbcopy failed: {e}"))?;

    {
        use std::io::Write;
        let stdin = child.stdin.as_mut().ok_or("pbcopy stdin unavailable")?;
        stdin
            .write_all(text.as_bytes())
            .map_err(|e| format!("pbcopy write failed: {e}"))?;
    }

    let status = child.wait().map_err(|e| format!("pbcopy wait failed: {e}"))?;
    if !status.success() {
        return Err(format!("pbcopy exited with status {status}"));
    }

    Ok(serde_json::json!({ "ok": true }))
}

fn ensure_clipboard_permission(
    app: &AppHandle,
    app_id: &str,
    operation: &str,
) -> Result<(), String> {
    let permission = format!("clipboard.{operation}");
    if app_has_permission(app, app_id, &permission) {
        Ok(())
    } else {
        Err(format!(
            "permission denied: clipboard.{operation} requires manifest.permissions entry '{permission}' or 'clipboard:*'"
        ))
    }
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

struct AgentCwdTarget {
    cwd: PathBuf,
    mcp_servers: Option<serde_json::Value>,
}

fn resolve_agent_cwd_for_app(
    app: &AppHandle,
    app_id: &str,
    cwd: Option<&str>,
) -> Result<AgentCwdTarget, String> {
    let app_root = apps::app_dir(app, app_id).map_err(|e| e.to_string())?;
    let cwd_path = cwd
        .map(PathBuf::from)
        .unwrap_or_else(|| app_root.clone());

    if same_path(&cwd_path, &app_root) {
        let project = project::find_project_for(&cwd_path);
        return Ok(AgentCwdTarget {
            cwd: cwd_path,
            mcp_servers: project.and_then(|p| p.mcp_servers),
        });
    }

    if let Some(project) = project::find_project_for(&cwd_path) {
        if project_is_linked_to_app(app, app_id, &project)
            || scoped_permission_allowed(app, app_id, "agent.project", &project.id)
        {
            return Ok(AgentCwdTarget {
                cwd: cwd_path,
                mcp_servers: project.mcp_servers,
            });
        }

        return Err(format!(
            "permission denied: agent cwd in project '{}' requires linked project or manifest.permissions entry 'agent.project:{}' / 'agent.project:*'",
            project.id, project.id
        ));
    }

    if app_has_permission(app, app_id, "agent.cwd:*") {
        return Ok(AgentCwdTarget {
            cwd: cwd_path,
            mcp_servers: None,
        });
    }

    Err(
        "permission denied: agent cwd must be this app root, a linked project, or require manifest.permissions entry 'agent.cwd:*'"
            .into(),
    )
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
            || (permission.starts_with("clipboard.") && p == "clipboard:*")
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

fn parse_storage_keys(params: &serde_json::Value) -> Result<Vec<String>, String> {
    if let Some(key) = params.get("key").and_then(|v| v.as_str()) {
        let key = key.trim();
        if !key.is_empty() {
            return Ok(vec![key.to_string()]);
        }
    }

    let keys = params
        .get("keys")
        .and_then(|v| v.as_array())
        .ok_or("missing key or keys")?;
    let out: Vec<String> = keys
        .iter()
        .filter_map(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    if out.is_empty() {
        return Err("keys must include at least one non-empty string".into());
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
    if let Some(schema) = &action.params_schema {
        validate_action_params(schema, &action_params)
            .map_err(|e| format!("invalid action params for {target_id}::{action_id}: {e}"))?;
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

fn list_app_summaries(app: &AppHandle) -> Result<serde_json::Value, String> {
    let listings = apps::list_apps(app).map_err(|e| e.to_string())?;
    let out: Vec<serde_json::Value> = listings
        .into_iter()
        .map(|listing| {
            let ready = listing.ready;
            let manifest = listing.manifest;
            serde_json::json!({
                "id": manifest.id,
                "name": manifest.name,
                "icon": manifest.icon,
                "description": manifest.description,
                "kind": manifest.kind,
                "runtime": manifest.runtime.unwrap_or_else(|| "static".into()),
                "ready": ready,
                "capabilities": {
                    "permissions": manifest.permissions.len(),
                    "network_hosts": manifest
                        .network
                        .as_ref()
                        .map(|n| n.allowed_hosts.len())
                        .unwrap_or(0),
                    "schedules": manifest.schedules.len(),
                    "actions": manifest.actions.len(),
                    "widgets": manifest.widgets.len(),
                },
                "actions": manifest.actions.iter().map(|action| serde_json::json!({
                    "id": action.id,
                    "name": action.name,
                    "description": action.description,
                    "public": action.public,
                    "has_params": action.params_schema.is_some(),
                })).collect::<Vec<_>>(),
                "widgets": manifest.widgets.iter().map(|widget| serde_json::json!({
                    "id": widget.id,
                    "name": widget.name,
                    "size": widget.size,
                    "description": widget.description,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(serde_json::Value::Array(out))
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
                "params_schema": a.params_schema,
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

fn validate_action_params(
    schema: &serde_json::Value,
    value: &serde_json::Value,
) -> Result<(), String> {
    validate_schema_value(schema, value, "params")
}

fn validate_schema_value(
    schema: &serde_json::Value,
    value: &serde_json::Value,
    path: &str,
) -> Result<(), String> {
    if let Some(expected) = schema.get("const") {
        if value != expected {
            return Err(format!("{path} must equal {expected}"));
        }
    }
    if let Some(options) = schema.get("enum").and_then(|v| v.as_array()) {
        if !options.iter().any(|option| option == value) {
            return Err(format!("{path} must be one of {:?}", options));
        }
    }

    let inferred_object = schema.get("properties").is_some() || schema.get("required").is_some();
    if let Some(type_value) = schema.get("type") {
        if !schema_type_matches(type_value, value) {
            return Err(format!("{path} must be {}", schema_type_label(type_value)));
        }
    } else if inferred_object && !value.is_object() {
        return Err(format!("{path} must be object"));
    }

    if let Some(properties) = schema.get("properties").and_then(|v| v.as_object()) {
        let object = value
            .as_object()
            .ok_or_else(|| format!("{path} must be object"))?;
        for (key, child_schema) in properties {
            if let Some(child_value) = object.get(key) {
                validate_schema_value(child_schema, child_value, &format!("{path}.{key}"))?;
            }
        }
    }

    if let Some(required) = schema.get("required").and_then(|v| v.as_array()) {
        let object = value
            .as_object()
            .ok_or_else(|| format!("{path} must be object"))?;
        for key in required.iter().filter_map(|v| v.as_str()) {
            if !object.contains_key(key) {
                return Err(format!("{path}.{key} is required"));
            }
        }
    }

    if let (Some(items_schema), Some(items)) =
        (schema.get("items"), value.as_array())
    {
        for (idx, item) in items.iter().enumerate() {
            validate_schema_value(items_schema, item, &format!("{path}[{idx}]"))?;
        }
    }

    Ok(())
}

fn schema_type_matches(type_value: &serde_json::Value, value: &serde_json::Value) -> bool {
    if let Some(types) = type_value.as_array() {
        return types.iter().any(|t| schema_type_matches(t, value));
    }
    match type_value.as_str().unwrap_or("") {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "number" => value.is_number(),
        "null" => value.is_null(),
        _ => true,
    }
}

fn schema_type_label(type_value: &serde_json::Value) -> String {
    if let Some(types) = type_value.as_array() {
        let labels: Vec<&str> = types.iter().filter_map(|v| v.as_str()).collect();
        return labels.join(" | ");
    }
    type_value.as_str().unwrap_or("valid JSON").to_string()
}

fn projects_list_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let include_all = bool_param(&params, "include_all", "includeAll").unwrap_or(false);
    if include_all {
        ensure_scoped_permission(app, app_id, "projects.read", "*")?;
    }

    let projects = list_user_projects(app)?;
    let out: Vec<serde_json::Value> = projects
        .into_iter()
        .filter(|project| include_all || can_read_project(app, app_id, project))
        .map(|project| project_summary(&project))
        .collect();
    Ok(serde_json::Value::Array(out))
}

fn projects_open_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let project_id = required_string_param(&params, "project_id", "projectId")?;
    let project = list_user_projects(app)?
        .into_iter()
        .find(|project| project.id == project_id)
        .ok_or_else(|| format!("project not found: {project_id}"))?;
    ensure_project_scope_access(app, app_id, "projects.read", &project)?;
    let project_id = project.id.clone();
    app.emit(
        "reflex://project-open-request",
        &serde_json::json!({
            "project_id": project_id,
            "from_app": app_id,
        }),
    )
    .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "ok": true, "project_id": project_id }))
}

fn topics_list_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let include_all = bool_param(&params, "include_all", "includeAll").unwrap_or(false);
    let target_project_id = string_param(&params, "project_id", "projectId");
    let limit = params
        .get("limit")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(100)
        .min(500);

    let projects = list_user_projects(app)?;
    let targets: Vec<project::Project> = if let Some(project_id) = target_project_id {
        let project = projects
            .into_iter()
            .find(|project| project.id == project_id)
            .ok_or_else(|| format!("project not found: {project_id}"))?;
        ensure_topic_read_access(app, app_id, &project)?;
        vec![project]
    } else if include_all {
        ensure_scoped_permission(app, app_id, "topics.read", "*")?;
        projects
    } else {
        projects
            .into_iter()
            .filter(|project| can_read_topics(app, app_id, project))
            .collect()
    };

    let mut out = Vec::new();
    for project in targets {
        let root = PathBuf::from(&project.root);
        let threads = match storage::read_all_threads(&root) {
            Ok(threads) => threads,
            Err(e) => {
                eprintln!("[reflex] app topics.list read_all_threads({}): {e}", project.root);
                continue;
            }
        };
        for thread in threads {
            out.push(topic_summary(&project, &thread));
        }
    }
    out.sort_by(|a, b| {
        let a_ms = a
            .get("created_at_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let b_ms = b
            .get("created_at_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        b_ms.cmp(&a_ms)
    });
    out.truncate(limit);
    Ok(serde_json::Value::Array(out))
}

fn topics_open_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let thread_id = required_string_param(&params, "thread_id", "threadId")?;
    let projects = list_user_projects(app)?;

    let project = if let Some(project_id) = string_param(&params, "project_id", "projectId") {
        let project = projects
            .into_iter()
            .find(|project| project.id == project_id)
            .ok_or_else(|| format!("project not found: {project_id}"))?;
        ensure_topic_read_access(app, app_id, &project)?;
        if !project_has_thread(&project, &thread_id)? {
            return Err(format!("topic not found: {thread_id}"));
        }
        project
    } else {
        let mut found = None;
        for project in projects {
            if !can_read_topics(app, app_id, &project) {
                continue;
            }
            match project_has_thread(&project, &thread_id) {
                Ok(true) => {
                    found = Some(project);
                    break;
                }
                Ok(false) => {}
                Err(e) => eprintln!("[reflex] topics.open scan {}: {e}", project.root),
            }
        }
        found.ok_or_else(|| format!("topic not found or not accessible: {thread_id}"))?
    };

    let project_id = project.id.clone();
    app.emit(
        "reflex://topic-open-request",
        &serde_json::json!({
            "project_id": project_id,
            "thread_id": thread_id,
            "from_app": app_id,
        }),
    )
    .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "ok": true,
        "project_id": project_id,
        "thread_id": thread_id,
    }))
}

fn project_has_thread(project: &project::Project, thread_id: &str) -> Result<bool, String> {
    let root = PathBuf::from(&project.root);
    let threads = storage::read_all_threads(&root).map_err(|e| e.to_string())?;
    Ok(threads.iter().any(|thread| thread.meta.id == thread_id))
}

fn skills_list_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let targets = project_targets_for_app(app, app_id, &params, "skills.read")?;
    let out: Vec<serde_json::Value> = targets
        .into_iter()
        .map(|project| {
            serde_json::json!({
                "project_id": project.id,
                "project_name": project.name,
                "skills": project.skills,
            })
        })
        .collect();
    Ok(serde_json::Value::Array(out))
}

fn mcp_servers_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let include_config = bool_param(&params, "include_config", "includeConfig").unwrap_or(false);
    let targets = project_targets_for_app(app, app_id, &params, "mcp.read")?;
    let mut out = Vec::new();

    for project in targets {
        if include_config {
            ensure_scoped_permission(app, app_id, "mcp.read", &project.id)?;
        }

        let object = project
            .mcp_servers
            .as_ref()
            .and_then(|value| value.as_object());
        let mut names: Vec<String> = object
            .map(|servers| servers.keys().cloned().collect())
            .unwrap_or_default();
        names.sort();

        let servers: Vec<serde_json::Value> = names
            .iter()
            .map(|name| {
                let mut item = serde_json::json!({ "name": name });
                if include_config {
                    item["config"] = object
                        .and_then(|servers| servers.get(name))
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                }
                item
            })
            .collect();

        out.push(serde_json::json!({
            "project_id": project.id,
            "project_name": project.name,
            "server_names": names,
            "servers": servers,
        }));
    }

    Ok(serde_json::Value::Array(out))
}

fn project_targets_for_app(
    app: &AppHandle,
    app_id: &str,
    params: &serde_json::Value,
    scope: &str,
) -> Result<Vec<project::Project>, String> {
    let include_all = bool_param(params, "include_all", "includeAll").unwrap_or(false);
    let target_project_id = string_param(params, "project_id", "projectId");
    let projects = list_user_projects(app)?;

    if let Some(project_id) = target_project_id {
        let project = projects
            .into_iter()
            .find(|project| project.id == project_id)
            .ok_or_else(|| format!("project not found: {project_id}"))?;
        ensure_project_scope_access(app, app_id, scope, &project)?;
        return Ok(vec![project]);
    }

    if include_all {
        ensure_scoped_permission(app, app_id, scope, "*")?;
        return Ok(projects);
    }

    Ok(projects
        .into_iter()
        .filter(|project| {
            project_is_linked_to_app(app, app_id, project)
                || scoped_permission_allowed(app, app_id, scope, &project.id)
        })
        .collect())
}

fn list_user_projects(app: &AppHandle) -> Result<Vec<project::Project>, String> {
    let apps_root = apps::apps_dir(app)
        .ok()
        .and_then(|path| path.canonicalize().ok());
    let projects = project::list_registered(app).map_err(|e| e.to_string())?;
    Ok(projects
        .into_iter()
        .filter(|project| {
            if let Some(apps_root) = &apps_root {
                if let Ok(root) = PathBuf::from(&project.root).canonicalize() {
                    return !root.starts_with(apps_root);
                }
            }
            true
        })
        .collect())
}

fn project_summary(project: &project::Project) -> serde_json::Value {
    let mcp_server_names: Vec<String> = project
        .mcp_servers
        .as_ref()
        .and_then(|value| value.as_object())
        .map(|object| object.keys().cloned().collect())
        .unwrap_or_default();
    serde_json::json!({
        "id": project.id,
        "name": project.name,
        "root": project.root,
        "created_at_ms": project.created_at_ms,
        "sandbox": project.sandbox,
        "description": project.description,
        "skills": project.skills,
        "apps": project.apps,
        "mcp_server_names": mcp_server_names,
        "has_agent_instructions": project
            .agent_instructions
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false),
    })
}

fn topic_summary(
    project: &project::Project,
    thread: &storage::StoredThread,
) -> serde_json::Value {
    let meta = &thread.meta;
    serde_json::json!({
        "project_id": project.id,
        "project_name": project.name,
        "project_root": project.root,
        "thread_id": meta.id,
        "title": meta.title,
        "prompt": meta.prompt,
        "goal": meta.goal,
        "cwd": meta.cwd,
        "created_at_ms": meta.created_at_ms,
        "exit_code": meta.exit_code,
        "done": meta.done,
        "plan_mode": meta.plan_mode,
        "plan_confirmed": meta.plan_confirmed,
        "source": meta.source,
        "event_count": thread.events.len(),
        "browser_tabs_count": meta.browser_tabs.len(),
    })
}

fn can_read_project(app: &AppHandle, app_id: &str, target: &project::Project) -> bool {
    project_is_linked_to_app(app, app_id, target)
        || scoped_permission_allowed(app, app_id, "projects.read", &target.id)
}

fn can_read_topics(app: &AppHandle, app_id: &str, target: &project::Project) -> bool {
    project_is_linked_to_app(app, app_id, target)
        || scoped_permission_allowed(app, app_id, "topics.read", &target.id)
}

fn ensure_topic_read_access(
    app: &AppHandle,
    app_id: &str,
    target: &project::Project,
) -> Result<(), String> {
    if can_read_topics(app, app_id, target) {
        Ok(())
    } else {
        Err(format!(
            "permission denied: topics.read requires linked project or manifest.permissions entry 'topics.read:{}' / 'topics.read:*'",
            target.id
        ))
    }
}

fn ensure_project_scope_access(
    app: &AppHandle,
    app_id: &str,
    scope: &str,
    target: &project::Project,
) -> Result<(), String> {
    if project_is_linked_to_app(app, app_id, target)
        || scoped_permission_allowed(app, app_id, scope, &target.id)
    {
        Ok(())
    } else {
        Err(format!(
            "permission denied: {scope} requires linked project or manifest.permissions entry '{scope}:{}' / '{scope}:*'",
            target.id
        ))
    }
}

fn project_is_linked_to_app(app: &AppHandle, app_id: &str, target: &project::Project) -> bool {
    if target.apps.iter().any(|id| id == app_id) {
        return true;
    }
    let app_root = match apps::app_dir(app, app_id) {
        Ok(root) => root,
        Err(_) => return false,
    };
    same_path(&app_root, Path::new(&target.root))
}

fn ensure_scoped_permission(
    app: &AppHandle,
    app_id: &str,
    scope: &str,
    target: &str,
) -> Result<(), String> {
    if scoped_permission_allowed(app, app_id, scope, target) {
        Ok(())
    } else {
        Err(format!(
            "permission denied: {scope} requires manifest.permissions entry '{scope}:{target}' or '{scope}:*'"
        ))
    }
}

fn scoped_permission_allowed(
    app: &AppHandle,
    app_id: &str,
    scope: &str,
    target: &str,
) -> bool {
    let manifest = match apps::read_manifest(app, app_id) {
        Ok(manifest) => manifest,
        Err(_) => return false,
    };
    let family = scope.split('.').next().unwrap_or(scope);
    let family_wildcard = format!("{family}:*");
    let scope_wildcard = format!("{scope}:*");
    let exact = format!("{scope}:{target}");
    manifest.permissions.iter().any(|permission| {
        permission == "*"
            || permission == scope
            || permission == &family_wildcard
            || permission == &scope_wildcard
            || permission == &exact
    })
}

async fn browser_init_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    ensure_browser_permission(app, app_id, "control")?;
    let project_id = string_param(&params, "project_id", "projectId");
    ensure_browser_project_access(app, app_id, project_id.as_deref())?;
    let headless = bool_param(&params, "headless", "headless");
    browser::browser_init(app.clone(), headless, project_id).await
}

async fn browser_open_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    ensure_browser_permission(app, app_id, "control")?;
    let url = string_param(&params, "url", "url");
    browser::browser_tab_open(app.clone(), url).await
}

async fn browser_navigate_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    ensure_browser_permission(app, app_id, "control")?;
    let tab_id = required_string_param(&params, "tab_id", "tabId")?;
    let url = required_string_param(&params, "url", "url")?;
    browser::browser_navigate(app.clone(), tab_id, url).await
}

async fn browser_read_text_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    ensure_browser_permission(app, app_id, "read")?;
    let tab_id = required_string_param(&params, "tab_id", "tabId")?;
    browser::browser_read_text(app.clone(), tab_id).await
}

async fn browser_read_outline_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    ensure_browser_permission(app, app_id, "read")?;
    let tab_id = required_string_param(&params, "tab_id", "tabId")?;
    browser::browser_read_outline(app.clone(), tab_id).await
}

async fn browser_screenshot_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    ensure_browser_permission(app, app_id, "read")?;
    let tab_id = required_string_param(&params, "tab_id", "tabId")?;
    let full_page = bool_param(&params, "full_page", "fullPage");
    browser::browser_screenshot(app.clone(), tab_id, full_page).await
}

async fn browser_click_text_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    ensure_browser_permission(app, app_id, "control")?;
    let tab_id = required_string_param(&params, "tab_id", "tabId")?;
    let text = required_string_param(&params, "text", "text")?;
    let exact = bool_param(&params, "exact", "exact");
    browser::browser_click_text(app.clone(), tab_id, text, exact).await
}

async fn browser_click_selector_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    ensure_browser_permission(app, app_id, "control")?;
    let tab_id = required_string_param(&params, "tab_id", "tabId")?;
    let selector = required_string_param(&params, "selector", "selector")?;
    browser::browser_click_selector(app.clone(), tab_id, selector).await
}

async fn browser_fill_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    ensure_browser_permission(app, app_id, "control")?;
    let tab_id = required_string_param(&params, "tab_id", "tabId")?;
    let selector = required_string_param(&params, "selector", "selector")?;
    let value = required_string_param(&params, "value", "value")?;
    browser::browser_fill(app.clone(), tab_id, selector, value).await
}

fn ensure_browser_project_access(
    app: &AppHandle,
    app_id: &str,
    project_id: Option<&str>,
) -> Result<(), String> {
    let Some(project_id) = project_id else {
        return Ok(());
    };
    let target = project::get_by_id(app, project_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("project not found: {project_id}"))?;
    if project_is_linked_to_app(app, app_id, &target)
        || scoped_permission_allowed(app, app_id, "browser.project", &target.id)
    {
        Ok(())
    } else {
        Err(format!(
            "permission denied: browser project state requires linked project or manifest.permissions entry 'browser.project:{}'",
            target.id
        ))
    }
}

fn ensure_browser_permission(
    app: &AppHandle,
    app_id: &str,
    operation: &str,
) -> Result<(), String> {
    if browser_permission_allowed(app, app_id, operation) {
        Ok(())
    } else {
        Err(format!(
            "permission denied: browser.{operation} requires manifest.permissions entry 'browser.{operation}' or 'browser:*'"
        ))
    }
}

fn browser_permission_allowed(app: &AppHandle, app_id: &str, operation: &str) -> bool {
    let manifest = match apps::read_manifest(app, app_id) {
        Ok(manifest) => manifest,
        Err(_) => return false,
    };
    let exact = format!("browser.{operation}");
    manifest.permissions.iter().any(|permission| {
        permission == "*"
            || permission == "browser:*"
            || permission == &exact
            || (operation == "read" && permission == "browser.control")
    })
}

fn scheduler_list_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let include_all = bool_param(&params, "include_all", "includeAll").unwrap_or(false);
    let target_app = string_param(&params, "app_id", "appId");
    let filter_app = if include_all {
        None
    } else {
        Some(target_app.as_deref().unwrap_or(app_id))
    };

    if include_all {
        ensure_scheduler_app_access(app, app_id, "read", "*", None)?;
    } else if let Some(target) = filter_app {
        ensure_scheduler_app_access(app, app_id, "read", target, None)?;
    }

    let mut items = scheduler::commands::scheduler_list(app.clone())?;
    if let Some(target) = filter_app {
        items.retain(|item| item.app_id == target);
    }
    serde_json::to_value(items).map_err(|e| e.to_string())
}

async fn scheduler_run_now_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let raw = required_string_param(&params, "schedule_id", "scheduleId")?;
    let (full_id, target_app, local_id) = resolve_schedule_target(app_id, &raw)?;
    ensure_scheduler_schedule_exists(app, &target_app, &local_id)?;
    ensure_scheduler_app_access(app, app_id, "run", &target_app, Some(&local_id))?;
    scheduler::commands::scheduler_run_now(app.clone(), full_id.clone()).await?;
    Ok(serde_json::json!({ "ok": true, "schedule_id": full_id }))
}

async fn scheduler_set_paused_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let raw = required_string_param(&params, "schedule_id", "scheduleId")?;
    let paused = params
        .get("paused")
        .and_then(|v| v.as_bool())
        .ok_or("missing paused")?;
    let (full_id, target_app, local_id) = resolve_schedule_target(app_id, &raw)?;
    ensure_scheduler_schedule_exists(app, &target_app, &local_id)?;
    ensure_scheduler_app_access(app, app_id, "write", &target_app, Some(&local_id))?;
    scheduler::commands::scheduler_set_paused(app.clone(), full_id.clone(), paused).await?;
    Ok(serde_json::json!({
        "ok": true,
        "schedule_id": full_id,
        "paused": paused,
    }))
}

fn scheduler_runs_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let include_all = bool_param(&params, "include_all", "includeAll").unwrap_or(false);
    let target_app = string_param(&params, "app_id", "appId");
    let filter_app = if include_all {
        None
    } else {
        Some(target_app.as_deref().unwrap_or(app_id))
    };

    if include_all {
        ensure_scheduler_app_access(app, app_id, "read", "*", None)?;
    } else if let Some(target) = filter_app {
        ensure_scheduler_app_access(app, app_id, "read", target, None)?;
    }

    let requested_limit = params
        .get("limit")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(50)
        .min(500);
    let before_ts = params
        .get("before_ts")
        .or_else(|| params.get("beforeTs"))
        .and_then(|v| v.as_u64());
    let fetch_limit = if filter_app.is_some() {
        500
    } else {
        requested_limit
    };
    let mut runs = scheduler::commands::scheduler_runs(app.clone(), Some(fetch_limit), before_ts)?;
    if let Some(target) = filter_app {
        runs.retain(|run| run.app_id == target);
    }
    runs.truncate(requested_limit);
    serde_json::to_value(runs).map_err(|e| e.to_string())
}

fn scheduler_run_detail_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let run_id = required_string_param(&params, "run_id", "runId")?;
    let record = scheduler::commands::scheduler_run_detail(app.clone(), run_id)?;
    if let Some(record) = &record {
        let local_schedule = record
            .schedule_id
            .as_deref()
            .and_then(scheduler::split_full_id)
            .map(|(_, local)| local);
        ensure_scheduler_app_access(app, app_id, "read", &record.app_id, local_schedule)?;
    }
    serde_json::to_value(record).map_err(|e| e.to_string())
}

fn bool_param(params: &serde_json::Value, snake: &str, camel: &str) -> Option<bool> {
    params
        .get(snake)
        .or_else(|| params.get(camel))
        .and_then(|v| v.as_bool())
}

fn string_param(params: &serde_json::Value, snake: &str, camel: &str) -> Option<String> {
    params
        .get(snake)
        .or_else(|| params.get(camel))
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn required_string_param(
    params: &serde_json::Value,
    snake: &str,
    camel: &str,
) -> Result<String, String> {
    string_param(params, snake, camel).ok_or_else(|| format!("missing {snake}"))
}

fn resolve_schedule_target(
    caller_app_id: &str,
    raw_id: &str,
) -> Result<(String, String, String), String> {
    if let Some((target_app, local_id)) = scheduler::split_full_id(raw_id) {
        if target_app.trim().is_empty() || local_id.trim().is_empty() {
            return Err("scheduleId must be <app>::<schedule> or a local schedule id".into());
        }
        return Ok((
            scheduler::make_full_id(target_app.trim(), local_id.trim()),
            target_app.trim().to_string(),
            local_id.trim().to_string(),
        ));
    }
    if raw_id.trim().is_empty() {
        return Err("scheduleId must be non-empty".into());
    }
    let local_id = raw_id.trim().to_string();
    Ok((
        scheduler::make_full_id(caller_app_id, &local_id),
        caller_app_id.to_string(),
        local_id,
    ))
}

fn ensure_scheduler_schedule_exists(
    app: &AppHandle,
    target_app: &str,
    local_id: &str,
) -> Result<(), String> {
    scheduler::manifest::find_schedule(app, target_app, local_id)
        .map(|_| ())
        .ok_or_else(|| format!("schedule not found: {target_app}::{local_id}"))
}

fn ensure_scheduler_app_access(
    app: &AppHandle,
    caller_app_id: &str,
    operation: &str,
    target_app: &str,
    local_schedule_id: Option<&str>,
) -> Result<(), String> {
    if target_app == caller_app_id {
        return Ok(());
    }
    if scheduler_permission_allowed(app, caller_app_id, operation, target_app, local_schedule_id) {
        return Ok(());
    }
    let target = match local_schedule_id {
        Some(local) => format!("{target_app}::{local}"),
        None => target_app.to_string(),
    };
    Err(format!(
        "permission denied: scheduler.{operation} requires manifest.permissions entry 'scheduler.{operation}:{target}' or 'scheduler.{operation}:*'"
    ))
}

fn scheduler_permission_allowed(
    app: &AppHandle,
    caller_app_id: &str,
    operation: &str,
    target_app: &str,
    local_schedule_id: Option<&str>,
) -> bool {
    let manifest = match apps::read_manifest(app, caller_app_id) {
        Ok(m) => m,
        Err(_) => return false,
    };
    let operation_permission = format!("scheduler.{operation}");
    let wildcard = format!("scheduler.{operation}:*");
    let target_permission = format!("scheduler.{operation}:{target_app}");
    let schedule_permission =
        local_schedule_id.map(|local| format!("scheduler.{operation}:{target_app}::{local}"));
    manifest.permissions.iter().any(|permission| {
        permission == "*"
            || permission == "scheduler:*"
            || permission == &operation_permission
            || permission == &wildcard
            || permission == &target_permission
            || schedule_permission
                .as_ref()
                .map(|exact| permission == exact)
                .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_summary_omits_raw_mcp_and_agent_instructions() {
        let project = project::Project {
            id: "p1".into(),
            name: "Project".into(),
            root: "/tmp/project".into(),
            created_at_ms: 42,
            sandbox: "workspace-write".into(),
            mcp_servers: Some(serde_json::json!({
                "private_server": {
                    "command": "node",
                    "args": ["server.js"],
                    "env": { "TOKEN": "secret" }
                }
            })),
            description: Some("desc".into()),
            agent_instructions: Some("private instructions".into()),
            skills: vec!["build-web-apps:react-best-practices".into()],
            apps: vec!["app1".into()],
        };

        let summary = project_summary(&project);

        assert!(summary.get("mcp_servers").is_none());
        assert!(summary.get("agent_instructions").is_none());
        assert_eq!(summary["mcp_server_names"], serde_json::json!(["private_server"]));
        assert_eq!(summary["has_agent_instructions"], true);
        assert_eq!(
            summary["skills"],
            serde_json::json!(["build-web-apps:react-best-practices"])
        );
    }

    #[test]
    fn validates_action_params_against_schema() {
        let schema = serde_json::json!({
            "type": "object",
            "required": ["query", "limit"],
            "properties": {
                "query": { "type": "string" },
                "limit": { "type": "integer" },
                "mode": { "enum": ["fast", "deep"] },
                "tags": { "type": "array", "items": { "type": "string" } }
            }
        });

        let valid = serde_json::json!({
            "query": "status",
            "limit": 3,
            "mode": "fast",
            "tags": ["project"]
        });
        assert!(validate_action_params(&schema, &valid).is_ok());

        let missing = serde_json::json!({ "query": "status" });
        assert!(validate_action_params(&schema, &missing)
            .unwrap_err()
            .contains("params.limit is required"));

        let wrong_type = serde_json::json!({ "query": "status", "limit": "3" });
        assert!(validate_action_params(&schema, &wrong_type)
            .unwrap_err()
            .contains("params.limit must be integer"));

        let wrong_enum = serde_json::json!({
            "query": "status",
            "limit": 3,
            "mode": "slow"
        });
        assert!(validate_action_params(&schema, &wrong_enum)
            .unwrap_err()
            .contains("params.mode must be one of"));
    }
}
