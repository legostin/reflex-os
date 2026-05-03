use crate::app_bus::{self, AppBusBridge};
use crate::{app_runtime, app_server};
use crate::apps;
use crate::memory::agents::recall::{self, RecallRequest};
use crate::memory::files;
use crate::memory::rag;
use crate::memory::schema::{MemoryKind, MemoryScope, ScopeRoots};
use crate::memory::store::{self, ListFilter, SaveRequest};
use crate::{browser, logs, memory, project, storage};
use crate::scheduler;
use crate::QuickContext;
use std::path::{Component, Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager};

pub async fn dispatch_app_method(
    app: &AppHandle,
    app_id: &str,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    eprintln!("[reflex] dispatch app={app_id} method={method}");
    match method {
        "bridge.catalog" => bridge_catalog_for_app(app, app_id),
        "system.context" => system_context(app, app_id),
        "system.openPanel" | "system.open_panel" => system_open_panel(app, app_id, &params),
        "system.openUrl" | "system.open_url" => system_open_url(app, &params),
        "system.openPath" | "system.open_path" => system_open_path(app, app_id, &params),
        "system.revealPath" | "system.reveal_path" => system_reveal_path(app, app_id, &params),
        "logs.write" => logs_write_for_app(app, app_id, &params),
        "logs.list" => logs_list_for_app(app, app_id, &params),
        "clipboard.readText" | "clipboard.read_text" => clipboard_read_text(app, app_id),
        "clipboard.writeText" | "clipboard.write_text" => {
            clipboard_write_text(app, app_id, &params)
        }
        "manifest.get" => manifest_get(app, app_id),
        "manifest.update" => manifest_update(app, app_id, params),
        "integration.catalog" => integration_catalog(params),
        "integration.profile" => integration_profile(app, app_id),
        "integration.update" => integration_update(app, app_id, params),
        "integration.learnVisible" | "integration.learn_visible" => {
            integration_learn_visible(app, app_id, params).await
        }
        "permissions.list" => permissions_list_for_app(app, app_id),
        "permissions.ensure" => permissions_ensure_for_app(app, app_id, params),
        "permissions.revoke" => permissions_revoke_for_app(app, app_id, params),
        "network.hosts" => network_hosts_for_app(app, app_id),
        "network.allowHost" => network_allow_host_for_app(app, app_id, params),
        "network.revokeHost" => network_revoke_host_for_app(app, app_id, params),
        "widgets.list" => widgets_list_for_app(app, app_id),
        "widgets.upsert" => widgets_upsert_for_app(app, app_id, params),
        "widgets.delete" => widgets_delete_for_app(app, app_id, params),
        "actions.list" => actions_list_for_app(app, app_id),
        "actions.upsert" => actions_upsert_for_app(app, app_id, params),
        "actions.delete" => actions_delete_for_app(app, app_id, params),
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
                .unwrap_or("Select folder")
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
                .unwrap_or("Select file")
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
                .unwrap_or("Save as")
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
            let prompt = build_app_agent_prompt(app_id, &params, prompt, &cwd_target).await;

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
            agent_task_for_app(app, app_id, params).await
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
            let topic = required_string_param(&params, "topic", "topic")?;
            let payload = params.get("payload").cloned().unwrap_or(serde_json::Value::Null);
            let bus = app.state::<memory::MemoryState>().bus.clone();
            app_bus::emit_event(&bus, app_id, &topic, payload.clone()).await?;
            let bridge: AppBusBridge =
                app.state::<AppBusBridge>().inner().clone();
            let event = bridge.record_event(app_id, &topic, payload);
            Ok(serde_json::json!({ "ok": true, "event": event }))
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
        "events.subscriptions" => {
            let bridge: AppBusBridge =
                app.state::<AppBusBridge>().inner().clone();
            Ok(serde_json::json!({ "topics": bridge.subscriptions(app_id) }))
        }
        "events.recent" => {
            let bridge: AppBusBridge =
                app.state::<AppBusBridge>().inner().clone();
            let topic = string_param(&params, "topic", "topic");
            let limit = bounded_usize_param(&params, "limit", "limit", 50, 200);
            let events = bridge.recent_events(app_id, topic.as_deref(), limit);
            Ok(serde_json::json!({ "events": events }))
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
        "apps.create" => apps_create_for_app(app, app_id, params).await,
        "apps.export" => apps_export_for_app(app, app_id, params),
        "apps.import" => apps_import_for_app(app, app_id, params),
        "apps.delete" => apps_delete_for_app(app, app_id, params),
        "apps.trashList" => apps_trash_list_for_app(app, app_id),
        "apps.restore" => apps_restore_for_app(app, app_id, params),
        "apps.purge" => apps_purge_for_app(app, app_id, params),
        "apps.status" => apps_status_for_app(app, app_id, params),
        "apps.diff" => apps_diff_for_app(app, app_id, params),
        "apps.commit" => apps_commit_for_app(app, app_id, params),
        "apps.commitPartial" | "apps.commit_partial" => {
            apps_commit_partial_for_app(app, app_id, params)
        }
        "apps.revert" => apps_revert_for_app(app, app_id, params),
        "apps.server.status" => apps_server_status_for_app(app, app_id, params).await,
        "apps.server.logs" => apps_server_logs_for_app(app, app_id, params).await,
        "apps.server.start" => apps_server_start_for_app(app, app_id, params).await,
        "apps.server.stop" => apps_server_stop_for_app(app, app_id, params).await,
        "apps.server.restart" => apps_server_restart_for_app(app, app_id, params).await,
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
        "project.profile.update" => project_profile_update_for_app(app, app_id, params),
        "project.sandbox.set" => project_sandbox_set_for_app(app, app_id, params),
        "project.apps.link" => project_apps_link_for_app(app, app_id, params),
        "project.apps.unlink" => project_apps_unlink_for_app(app, app_id, params),
        "topics.list" | "threads.list" => topics_list_for_app(app, app_id, params),
        "topics.open" | "threads.open" => topics_open_for_app(app, app_id, params),
        "skills.list" => skills_list_for_app(app, app_id, params),
        "project.skills.ensure" => project_skills_ensure_for_app(app, app_id, params),
        "project.skills.revoke" => project_skills_revoke_for_app(app, app_id, params),
        "mcp.servers" | "mcp.list" => mcp_servers_for_app(app, app_id, params),
        "project.mcp.upsert" => project_mcp_upsert_for_app(app, app_id, params),
        "project.mcp.delete" => project_mcp_delete_for_app(app, app_id, params),
        "project.browser.setEnabled" => project_browser_set_enabled_for_app(app, app_id, params),
        "project.files.list" => project_files_list_for_app(app, app_id, params),
        "project.files.read" => project_files_read_for_app(app, app_id, params),
        "project.files.search" => project_files_search_for_app(app, app_id, params),
        "project.files.write" => project_files_write_for_app(app, app_id, params),
        "project.files.mkdir" => project_files_mkdir_for_app(app, app_id, params),
        "project.files.move" => project_files_move_for_app(app, app_id, params),
        "project.files.copy" => project_files_copy_for_app(app, app_id, params),
        "project.files.delete" => project_files_delete_for_app(app, app_id, params),
        "browser.init" => browser_init_for_app(app, app_id, params).await,
        "browser.tabs.list" | "browser.tabsList" => {
            ensure_browser_permission(app, app_id, "read")?;
            browser::browser_tabs_list(app.clone()).await
        }
        "browser.tab.open" | "browser.open" => browser_open_for_app(app, app_id, params).await,
        "browser.tab.close" | "browser.close" => browser_close_for_app(app, app_id, params).await,
        "browser.setActive" | "browser.set_active" => {
            browser_set_active_for_app(app, app_id, params).await
        }
        "browser.navigate" => browser_navigate_for_app(app, app_id, params).await,
        "browser.back" => browser_back_for_app(app, app_id, params).await,
        "browser.forward" => browser_forward_for_app(app, app_id, params).await,
        "browser.reload" => browser_reload_for_app(app, app_id, params).await,
        "browser.currentUrl" | "browser.current_url" => {
            browser_current_url_for_app(app, app_id, params).await
        }
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
        "browser.scroll" => browser_scroll_for_app(app, app_id, params).await,
        "browser.waitFor" | "browser.wait_for" => {
            browser_wait_for_app(app, app_id, params).await
        }
        "scheduler.list" => scheduler_list_for_app(app, app_id, params),
        "scheduler.upsert" => scheduler_upsert_for_app(app, app_id, params),
        "scheduler.delete" => scheduler_delete_for_app(app, app_id, params).await,
        "scheduler.runNow" | "scheduler.run_now" => {
            scheduler_run_now_for_app(app, app_id, params).await
        }
        "scheduler.setPaused" | "scheduler.set_paused" => {
            scheduler_set_paused_for_app(app, app_id, params).await
        }
        "scheduler.runs" => scheduler_runs_for_app(app, app_id, params),
        "scheduler.stats" => scheduler_stats_for_app(app, app_id, params),
        "scheduler.runDetail" | "scheduler.run_detail" => {
            scheduler_run_detail_for_app(app, app_id, params)
        }
        "memory.save" => memory_save_for_app(app, app_id, params).await,
        "memory.read" => memory_read_for_app(app, app_id, params),
        "memory.update" => memory_update_for_app(app, app_id, params).await,
        "memory.list" => memory_list_for_app(app, app_id, params),
        "memory.delete" => memory_delete_for_app(app, app_id, params),
        "memory.search" => memory_search_for_app(app, app_id, params).await,
        "memory.recall" => memory_recall_for_app(app, app_id, params).await,
        "memory.stats" => memory_stats_for_app(app, app_id, params),
        "memory.reindex" => memory_reindex_for_app(app, app_id, params).await,
        "memory.indexPath" | "memory.index_path" => {
            memory_index_path_for_app(app, app_id, params).await
        }
        "memory.pathStatus" | "memory.path_status" => {
            memory_path_status_for_app(app, app_id, params)
        }
        "memory.pathStatusBatch" | "memory.path_status_batch" => {
            memory_path_status_batch_for_app(app, app_id, params)
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

fn integration_catalog(params: serde_json::Value) -> Result<serde_json::Value, String> {
    let provider = string_param(&params, "provider", "provider").map(|s| s.to_lowercase());
    let mut recipes = vec![
        serde_json::json!({
            "provider": "generic_web",
            "display_name": "Generic web app",
            "external": {
                "url": "",
                "open_url": "",
            },
            "capabilities": ["visible_session.read", "browser.control", "mcp.optional"],
            "data_strategy": [
                "Use runtime=external only when the service can be framed.",
                "Use the Browser bridge to inspect a visible user session when embedding is blocked.",
                "Publish normalized manifest.actions for any data the utility exposes to other apps.",
            ],
            "mcp": {
                "recommended": false,
                "notes": "Add a provider-specific MCP server when durable authenticated data access is required."
            }
        }),
        serde_json::json!({
            "provider": "telegram",
            "display_name": "Telegram",
            "external": {
                "url": "https://web.telegram.org/a/",
                "open_url": "https://web.telegram.org/a/"
            },
            "capabilities": [
                "messages.visible_session.read",
                "messages.search",
                "chats.list",
                "summaries.write"
            ],
            "data_strategy": [
                "Show Telegram Web when it can be framed; otherwise provide an Open button and use the Browser bridge after the user logs in.",
                "For personal chats, use user-approved Telegram client access such as MTProto/TDLib or a dedicated MCP server.",
                "Do not use a bot-token-only flow to claim access to arbitrary personal messages; bots only see chats where they are present and allowed.",
                "Store derived summaries by default. Store raw messages only when the user explicitly enables it."
            ],
            "mcp": {
                "recommended": true,
                "server_name": "telegram",
                "config_shape": {
                    "command": "node",
                    "args": ["path/to/telegram-mcp-server.js"],
                    "env": {
                        "TELEGRAM_API_ID": "<from user>",
                        "TELEGRAM_API_HASH": "<from user>",
                        "TELEGRAM_SESSION": "<stored outside manifest>"
                    }
                },
                "notes": "This is a shape for a user-provided MCP bridge. Reflex does not ship Telegram credentials or a Telegram client."
            }
        }),
    ];
    if let Some(provider) = provider {
        recipes.retain(|item| {
            item.get("provider")
                .and_then(|v| v.as_str())
                .map(|p| p.eq_ignore_ascii_case(&provider))
                .unwrap_or(false)
        });
    }
    Ok(serde_json::json!({ "recipes": recipes }))
}

fn integration_profile(app: &AppHandle, app_id: &str) -> Result<serde_json::Value, String> {
    let manifest = apps::read_manifest(app, app_id).map_err(|e| e.to_string())?;
    let context = system_context(app, app_id).unwrap_or_else(|_| serde_json::json!({}));
    Ok(serde_json::json!({
        "app_id": app_id,
        "provider": manifest.integration.as_ref().map(|i| i.provider.clone()).unwrap_or_default(),
        "integration": manifest.integration,
        "external": manifest.external,
        "runtime": manifest.runtime.unwrap_or_else(|| "static".into()),
        "linked_projects": context.get("linked_projects").cloned().unwrap_or_else(|| serde_json::json!([])),
        "app_project": context.get("app_project").cloned().unwrap_or(serde_json::Value::Null),
    }))
}

fn integration_update(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let current = apps::read_manifest(app, app_id).map_err(|e| e.to_string())?;
    let mut value = serde_json::to_value(current).map_err(|e| e.to_string())?;
    let direct_integration_patch =
        if params.get("integration").is_none() && params.get("patch").is_none() && params.get("external").is_none() {
            Some(params.clone())
        } else {
            None
        };

    if let Some(patch) = params
        .get("integration")
        .or_else(|| params.get("patch"))
        .cloned()
        .or(direct_integration_patch)
    {
        if !patch.is_object() {
            return Err("integration patch must be a JSON object".into());
        }
        if !value
            .get("integration")
            .map(|v| v.is_object())
            .unwrap_or(false)
        {
            value["integration"] = serde_json::json!({});
        }
        merge_json(&mut value["integration"], patch);
    }

    if let Some(patch) = params.get("external") {
        if !patch.is_object() {
            return Err("external patch must be a JSON object".into());
        }
        if !value.get("external").map(|v| v.is_object()).unwrap_or(false) {
            value["external"] = serde_json::json!({});
        }
        merge_json(&mut value["external"], patch.clone());
    }

    value["id"] = serde_json::Value::String(app_id.to_string());
    let mut manifest: apps::AppManifest =
        serde_json::from_value(value).map_err(|e| format!("invalid manifest: {e}"))?;
    manifest.id = app_id.to_string();
    write_manifest_and_emit(app, app_id, &manifest)?;
    Ok(serde_json::json!({
        "ok": true,
        "integration": manifest.integration,
        "external": manifest.external,
    }))
}

async fn integration_learn_visible(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let manifest = apps::read_manifest(app, app_id).map_err(|e| e.to_string())?;
    let provider = string_param(&params, "provider", "provider")
        .or_else(|| manifest.integration.as_ref().map(|i| i.provider.clone()))
        .filter(|provider| !provider.trim().is_empty())
        .unwrap_or_else(|| "generic_web".into());
    let service_url = string_param(&params, "service_url", "serviceUrl")
        .or_else(|| string_param(&params, "url", "url"))
        .or_else(|| manifest.external.as_ref().map(|external| external.url.clone()))
        .or_else(|| {
            manifest
                .external
                .as_ref()
                .and_then(|external| external.open_url.clone())
        })
        .unwrap_or_default();
    let mut tab_id = string_param(&params, "tab_id", "tabId");
    let mut current_url = string_param(&params, "current_url", "currentUrl");
    let mut visible_text = string_param(&params, "visible_text", "visibleText");
    let mut outline = params
        .get("outline")
        .cloned()
        .filter(|value| !value.is_null())
        .unwrap_or(serde_json::Value::Null);

    if visible_text.is_none() || outline.is_null() {
        ensure_browser_permission(app, app_id, "read")?;

        if tab_id.is_none() {
            if service_url.trim().is_empty() {
                return Err(
                    "missing tabId or serviceUrl; integration.learnVisible needs visible text, \
                     an existing browser tab, or a URL to open"
                        .into(),
                );
            }
            ensure_browser_permission(app, app_id, "control")?;
            browser_init_for_app(
                app,
                app_id,
                serde_json::json!({
                    "headless": bool_param(&params, "headless", "headless").unwrap_or(true),
                    "projectId": string_param(&params, "project_id", "projectId"),
                }),
            )
            .await?;
            let opened = browser_open_for_app(
                app,
                app_id,
                serde_json::json!({ "url": service_url.clone() }),
            )
            .await?;
            tab_id = opened
                .get("tab_id")
                .or_else(|| opened.get("tabId"))
                .and_then(|value| value.as_str())
                .map(|value| value.to_string());
            if let Some(tab_id) = tab_id.as_ref() {
                let _ = browser_wait_for_app(
                    app,
                    app_id,
                    serde_json::json!({
                        "tabId": tab_id,
                        "selector": "body",
                        "timeoutMs": 15000,
                    }),
                )
                .await;
            }
        }

        let tab_id_for_read = tab_id
            .clone()
            .ok_or_else(|| "missing tabId for visible interface learning".to_string())?;

        if visible_text.is_none() {
            let read = browser_read_text_for_app(
                app,
                app_id,
                serde_json::json!({ "tabId": tab_id_for_read.clone() }),
            )
            .await?;
            visible_text = Some(browser_text_from_value(&read));
        }

        if outline.is_null() {
            outline = browser_read_outline_for_app(
                app,
                app_id,
                serde_json::json!({ "tabId": tab_id_for_read.clone() }),
            )
            .await?;
        }

        if current_url.is_none() {
            current_url = browser_current_url_for_app(
                app,
                app_id,
                serde_json::json!({ "tabId": tab_id_for_read }),
            )
            .await
            .ok()
            .and_then(|value| {
                value
                    .get("url")
                    .and_then(|url| url.as_str())
                    .map(|url| url.to_string())
            });
        }
    }

    let visible_text = visible_text.unwrap_or_default();
    let prompt =
        build_connected_app_learn_prompt(&provider, &service_url, &visible_text, &outline);
    let learned = agent_task_for_app(
        app,
        app_id,
        serde_json::json!({
            "prompt": prompt,
            "includeContext": false,
            "sandbox": "read-only",
        }),
    )
    .await?;
    let learned_text = learned
        .get("result")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();
    let learned_at_ms = current_time_ms();
    let captured_at_ms = learned_at_ms;
    let storage_key = format!("connected.{provider}.latestVisibleSession");
    let learned_key = format!("connected.{provider}.learnedInterface");
    let visible_snapshot = serde_json::json!({
        "provider": provider,
        "service_url": service_url,
        "current_url": current_url,
        "tab_id": tab_id,
        "captured_at_ms": captured_at_ms,
        "text": visible_text,
    });
    let profile = serde_json::json!({
        "provider": provider,
        "service_url": service_url,
        "current_url": current_url,
        "tab_id": tab_id,
        "learned_at_ms": learned_at_ms,
        "visible_text_chars": visible_snapshot
            .get("text")
            .and_then(|value| value.as_str())
            .map(|text| text.chars().count())
            .unwrap_or(0),
        "outline_items": outline_item_count(&outline),
        "profile": learned_text,
        "agent_thread_id": learned
            .get("threadId")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
    });

    storage_set_value(app, app_id, &storage_key, visible_snapshot.clone())?;
    storage_set_value(app, app_id, &learned_key, profile.clone())?;

    let bridge: AppBusBridge = app.state::<AppBusBridge>().inner().clone();
    let event = bridge.record_event(
        app_id,
        &format!("connected.{provider}.visible_session"),
        serde_json::json!({
            "provider": provider,
            "captured_at_ms": captured_at_ms,
            "learned_at_ms": learned_at_ms,
            "text_length": visible_snapshot
                .get("text")
                .and_then(|value| value.as_str())
                .map(|text| text.len())
                .unwrap_or(0),
            "outline_items": outline_item_count(&outline),
        }),
    );

    let mut capabilities = manifest
        .integration
        .as_ref()
        .map(|integration| integration.capabilities.clone())
        .unwrap_or_default();
    if !capabilities
        .iter()
        .any(|capability| capability == "interface.visible_session.learn")
    {
        capabilities.push("interface.visible_session.learn".into());
    }
    let integration_result = integration_update(
        app,
        app_id,
        serde_json::json!({
            "integration": {
                "provider": provider,
                "capabilities": capabilities,
                "data_model": {
                    "learned_profile": profile,
                },
            }
        }),
    )?;

    Ok(serde_json::json!({
        "ok": true,
        "profile": integration_result
            .get("integration")
            .and_then(|integration| integration.get("data_model"))
            .and_then(|data_model| data_model.get("learned_profile"))
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        "storageKey": storage_key,
        "learnedKey": learned_key,
        "event": event,
    }))
}

fn build_connected_app_learn_prompt(
    provider: &str,
    service_url: &str,
    visible_text: &str,
    outline: &serde_json::Value,
) -> String {
    let outline_json =
        serde_json::to_string_pretty(outline).unwrap_or_else(|_| "null".to_string());
    format!(
        "Build a connected-app adapter profile from the visible web UI below. Use only visible text and outline. Infer data entities, user actions, likely selectors or text anchors, safe automation workflows, data-access boundaries, and MCP bridge opportunities. Do not claim access to hidden data. Return concise JSON with provider, entities, actions, workflows, selectors, data_access, mcp_bridge, risks, and next_steps.\n\nPROVIDER:\n{provider}\n\nSERVICE_URL:\n{service_url}\n\nVISIBLE_TEXT:\n{visible_text}\n\nOUTLINE:\n{outline_json}"
    )
}

fn browser_text_from_value(value: &serde_json::Value) -> String {
    value
        .get("text")
        .and_then(|text| text.as_str())
        .or_else(|| value.as_str())
        .unwrap_or_default()
        .to_string()
}

fn outline_item_count(value: &serde_json::Value) -> usize {
    value
        .get("outline")
        .and_then(|outline| outline.as_array())
        .or_else(|| value.as_array())
        .map(|items| items.len())
        .unwrap_or(0)
}

fn storage_set_value(
    app: &AppHandle,
    app_id: &str,
    key: &str,
    value: serde_json::Value,
) -> Result<(), String> {
    let mut store = apps::read_storage(app, app_id).map_err(|e| e.to_string())?;
    if !store.is_object() {
        store = serde_json::json!({});
    }
    store
        .as_object_mut()
        .expect("storage object")
        .insert(key.to_string(), value);
    apps::write_storage(app, app_id, &store).map_err(|e| e.to_string())
}

fn current_time_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn write_manifest_and_emit(
    app: &AppHandle,
    app_id: &str,
    manifest: &apps::AppManifest,
) -> Result<(), String> {
    apps::write_manifest(app, app_id, manifest).map_err(|e| e.to_string())?;
    emit_apps_changed(app);
    if let Some(handle) = app.try_state::<scheduler::SchedulerHandle>() {
        handle.inner().rescan();
    }
    Ok(())
}

fn permissions_list_for_app(app: &AppHandle, app_id: &str) -> Result<serde_json::Value, String> {
    let manifest = apps::read_manifest(app, app_id).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "permissions": manifest.permissions }))
}

fn permissions_ensure_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let permissions = parse_string_list(&params, "permission", "permissions")?;
    let mut manifest = apps::read_manifest(app, app_id).map_err(|e| e.to_string())?;
    let mut added = Vec::new();
    for permission in permissions {
        let permission = normalize_permission(&permission)?;
        if !manifest.permissions.iter().any(|p| p == &permission) {
            manifest.permissions.push(permission.clone());
            added.push(permission);
        }
    }
    write_manifest_and_emit(app, app_id, &manifest)?;
    Ok(serde_json::json!({
        "ok": true,
        "added": added,
        "permissions": manifest.permissions,
    }))
}

fn permissions_revoke_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let permissions = parse_string_list(&params, "permission", "permissions")?;
    let revoke: std::collections::HashSet<String> = permissions
        .into_iter()
        .map(|permission| normalize_permission(&permission))
        .collect::<Result<_, _>>()?;
    let mut manifest = apps::read_manifest(app, app_id).map_err(|e| e.to_string())?;
    let before = manifest.permissions.len();
    manifest.permissions.retain(|permission| !revoke.contains(permission));
    let removed = before.saturating_sub(manifest.permissions.len());
    write_manifest_and_emit(app, app_id, &manifest)?;
    Ok(serde_json::json!({
        "ok": true,
        "removed": removed,
        "permissions": manifest.permissions,
    }))
}

fn network_hosts_for_app(app: &AppHandle, app_id: &str) -> Result<serde_json::Value, String> {
    let manifest = apps::read_manifest(app, app_id).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "allowed_hosts": manifest
            .network
            .map(|network| network.allowed_hosts)
            .unwrap_or_default(),
    }))
}

fn network_allow_host_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let hosts = parse_string_list(&params, "host", "hosts")?;
    let mut manifest = apps::read_manifest(app, app_id).map_err(|e| e.to_string())?;
    let network = manifest.network.get_or_insert_with(apps::NetworkPolicy::default);
    let mut added = Vec::new();
    for host in hosts {
        let host = normalize_allowed_host(&host)?;
        if !network.allowed_hosts.iter().any(|h| h == &host) {
            network.allowed_hosts.push(host.clone());
            added.push(host);
        }
    }
    let allowed_hosts = network.allowed_hosts.clone();
    write_manifest_and_emit(app, app_id, &manifest)?;
    Ok(serde_json::json!({
        "ok": true,
        "added": added,
        "allowed_hosts": allowed_hosts,
    }))
}

fn network_revoke_host_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let hosts = parse_string_list(&params, "host", "hosts")?;
    let revoke: std::collections::HashSet<String> = hosts
        .into_iter()
        .map(|host| normalize_allowed_host(&host))
        .collect::<Result<_, _>>()?;
    let mut manifest = apps::read_manifest(app, app_id).map_err(|e| e.to_string())?;
    let mut removed = 0usize;
    let mut allowed_hosts = Vec::new();
    if let Some(network) = manifest.network.as_mut() {
        let before = network.allowed_hosts.len();
        network.allowed_hosts.retain(|host| !revoke.contains(host));
        removed = before.saturating_sub(network.allowed_hosts.len());
        allowed_hosts = network.allowed_hosts.clone();
        if network.allowed_hosts.is_empty() {
            manifest.network = None;
        }
    }
    write_manifest_and_emit(app, app_id, &manifest)?;
    Ok(serde_json::json!({
        "ok": true,
        "removed": removed,
        "allowed_hosts": allowed_hosts,
    }))
}

fn parse_string_list(
    params: &serde_json::Value,
    single_key: &str,
    list_key: &str,
) -> Result<Vec<String>, String> {
    if let Some(value) = params.get(single_key).and_then(|v| v.as_str()) {
        let value = value.trim();
        if !value.is_empty() {
            return Ok(vec![value.to_string()]);
        }
    }
    let value = params
        .get(list_key)
        .or_else(|| params.get(&snake_to_camel(list_key)))
        .ok_or_else(|| format!("missing {single_key} or {list_key}"))?;
    if let Some(value) = value.as_str() {
        let value = value.trim();
        if !value.is_empty() {
            return Ok(vec![value.to_string()]);
        }
    }
    let items = value
        .as_array()
        .ok_or_else(|| format!("{list_key} must be a string or array of strings"))?;
    let out: Vec<String> = items
        .iter()
        .filter_map(|item| item.as_str())
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect();
    if out.is_empty() {
        return Err(format!("{list_key} must include at least one non-empty string"));
    }
    Ok(out)
}

fn snake_to_camel(input: &str) -> String {
    let mut out = String::new();
    let mut upper_next = false;
    for ch in input.chars() {
        if ch == '_' {
            upper_next = true;
        } else if upper_next {
            out.extend(ch.to_uppercase());
            upper_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

fn normalize_permission(permission: &str) -> Result<String, String> {
    let permission = permission.trim();
    if permission.is_empty() {
        return Err("permission must be non-empty".into());
    }
    if permission.len() > 160 {
        return Err("permission must be 160 characters or fewer".into());
    }
    if !permission
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | ':' | '*' | '-' | '_'))
    {
        return Err("permission may contain only ASCII letters, numbers, '.', ':', '*', '-' or '_'".into());
    }
    Ok(permission.to_string())
}

fn normalize_allowed_host(raw: &str) -> Result<String, String> {
    let raw = raw.trim().to_lowercase();
    if raw.is_empty() {
        return Err("host must be non-empty".into());
    }
    let wildcard = raw.starts_with("*.");
    let input = raw.strip_prefix("*.").unwrap_or(&raw);
    let parsed_host = if input.contains("://") {
        reqwest::Url::parse(input)
            .map_err(|e| format!("invalid host url: {e}"))?
            .host_str()
            .ok_or_else(|| "host url has no host".to_string())?
            .to_string()
    } else {
        reqwest::Url::parse(&format!("https://{input}"))
            .ok()
            .and_then(|url| url.host_str().map(str::to_string))
            .unwrap_or_else(|| input.to_string())
    };
    let host = parsed_host.trim().trim_matches('.').to_string();
    if host.is_empty() {
        return Err("host must be non-empty".into());
    }
    if host.len() > 253 {
        return Err("host must be 253 characters or fewer".into());
    }
    if host.contains('/') || host.contains(char::is_whitespace) || host.contains('*') {
        return Err("host must be a hostname, IP address, or leading wildcard hostname".into());
    }
    if wildcard && (host == "localhost" || host.parse::<std::net::IpAddr>().is_ok()) {
        return Err("wildcard host must be a DNS hostname".into());
    }
    Ok(if wildcard { format!("*.{host}") } else { host })
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

fn widgets_list_for_app(app: &AppHandle, app_id: &str) -> Result<serde_json::Value, String> {
    let manifest = apps::read_manifest(app, app_id).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "widgets": manifest.widgets }))
}

fn widgets_upsert_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let (widget, html) = parse_widget_upsert(params)?;
    if let Some(html) = html {
        apps::write_app_file(app, app_id, &widget.entry, html.as_bytes())
            .map_err(|e| e.to_string())?;
    }
    let mut manifest = apps::read_manifest(app, app_id).map_err(|e| e.to_string())?;
    let created = match manifest
        .widgets
        .iter()
        .position(|existing| existing.id == widget.id)
    {
        Some(idx) => {
            manifest.widgets[idx] = widget.clone();
            false
        }
        None => {
            manifest.widgets.push(widget.clone());
            true
        }
    };
    apps::write_manifest(app, app_id, &manifest).map_err(|e| e.to_string())?;
    emit_apps_changed(app);
    Ok(serde_json::json!({
        "ok": true,
        "created": created,
        "widget": widget,
    }))
}

fn widgets_delete_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let id = string_param(&params, "widget_id", "widgetId")
        .or_else(|| string_param(&params, "id", "id"))
        .ok_or_else(|| "missing widget_id".to_string())?;
    validate_widget_id(&id)?;
    let delete_entry = bool_param(&params, "delete_entry", "deleteEntry")
        .or_else(|| bool_param(&params, "delete_file", "deleteFile"))
        .unwrap_or(false);
    let mut manifest = apps::read_manifest(app, app_id).map_err(|e| e.to_string())?;
    let removed = manifest.widgets.iter().find(|widget| widget.id == id).cloned();
    let Some(removed) = removed else {
        return Ok(serde_json::json!({
            "ok": true,
            "deleted": false,
            "widget_id": id,
        }));
    };
    manifest.widgets.retain(|widget| widget.id != id);
    apps::write_manifest(app, app_id, &manifest).map_err(|e| e.to_string())?;
    let mut deleted_entry = false;
    if delete_entry {
        let entry = normalize_widget_entry(&removed.entry)?;
        match apps::delete_app_path(app, app_id, &entry, false) {
            Ok(_) => deleted_entry = true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    emit_apps_changed(app);
    Ok(serde_json::json!({
        "ok": true,
        "deleted": true,
        "deleted_entry": deleted_entry,
        "widget_id": id,
        "widget": removed,
    }))
}

fn parse_widget_upsert(
    params: serde_json::Value,
) -> Result<(apps::WidgetDef, Option<String>), String> {
    let mut value = params
        .get("widget")
        .cloned()
        .unwrap_or_else(|| params.clone());
    let html = params
        .get("html")
        .or_else(|| params.get("content"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let obj = value
        .as_object_mut()
        .ok_or_else(|| "widget must be a JSON object".to_string())?;
    let id = obj
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .ok_or_else(|| "widget.id is required".to_string())?;
    validate_widget_id(&id)?;
    obj.insert("id".into(), serde_json::Value::String(id.clone()));
    if !obj.contains_key("name") {
        obj.insert("name".into(), serde_json::Value::String(id.clone()));
    }
    if !obj.contains_key("entry") {
        obj.insert(
            "entry".into(),
            serde_json::Value::String(format!("widgets/{id}.html")),
        );
    }
    let mut widget: apps::WidgetDef =
        serde_json::from_value(value).map_err(|e| format!("invalid widget: {e}"))?;
    widget.id = widget.id.trim().to_string();
    widget.name = widget.name.trim().to_string();
    widget.entry = normalize_widget_entry(&widget.entry)?;
    widget.size = normalize_widget_size(&widget.size)?;
    if let Some(desc) = widget.description.as_mut() {
        *desc = desc.trim().to_string();
    }
    if widget.name.is_empty() {
        return Err("widget.name is required".into());
    }
    Ok((widget, html))
}

fn validate_widget_id(id: &str) -> Result<(), String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("widget.id is required".into());
    }
    if id.len() > 80 {
        return Err("widget.id must be 80 characters or fewer".into());
    }
    if !id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err("widget.id may contain only ASCII letters, numbers, '-', '_' or '.'".into());
    }
    Ok(())
}

fn normalize_widget_entry(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim().trim_start_matches('/');
    if trimmed.is_empty() {
        return Err("widget.entry is required".into());
    }
    let path = Path::new(trimmed);
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => {
                let part = part.to_string_lossy();
                if part.starts_with('.') {
                    return Err("widget.entry may not contain hidden path components".into());
                }
                parts.push(part.to_string());
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err("widget.entry must be a relative app path".into());
            }
        }
    }
    if parts.is_empty() {
        return Err("widget.entry is required".into());
    }
    if matches!(parts.first().map(String::as_str), Some(".reflex" | ".git")) {
        return Err("widget.entry may not target internal app metadata".into());
    }
    if matches!(parts.as_slice(), [only] if only == "manifest.json" || only == "storage.json") {
        return Err("widget.entry may not target app metadata files".into());
    }
    Ok(parts.join("/"))
}

fn normalize_widget_size(raw: &str) -> Result<String, String> {
    let size = raw.trim();
    let size = if size.is_empty() { "small" } else { size };
    match size {
        "small" | "medium" | "wide" | "large" => Ok(size.to_string()),
        _ => Err("widget.size must be one of small, medium, wide, large".into()),
    }
}

fn emit_apps_changed(app: &AppHandle) {
    let _ = app.emit("reflex://apps-changed", &serde_json::json!({}));
}

fn bridge_catalog_for_app(app: &AppHandle, app_id: &str) -> Result<serde_json::Value, String> {
    let manifest = apps::read_manifest(app, app_id).map_err(|e| e.to_string())?;
    let methods = vec![
        bridge_group(
            "System and Manifest",
            &[
                "bridge.catalog",
                "system.context",
                "system.openPanel",
                "system.openUrl",
                "system.openPath",
                "system.revealPath",
                "logs.write",
                "logs.list",
                "manifest.get",
                "manifest.update",
                "integration.catalog",
                "integration.profile",
                "integration.update",
                "integration.learnVisible",
                "permissions.list",
                "permissions.ensure",
                "permissions.revoke",
                "network.hosts",
                "network.allowHost",
                "network.revokeHost",
                "widgets.list",
                "widgets.upsert",
                "widgets.delete",
                "actions.list",
                "actions.upsert",
                "actions.delete",
            ],
        ),
        bridge_group(
            "Agent Runtime",
            &[
                "agent.ask",
                "agent.startTopic",
                "agent.task",
                "agent.stream",
                "agent.streamAbort",
            ],
        ),
        bridge_group(
            "App Data and Files",
            &[
                "storage.get",
                "storage.set",
                "storage.list",
                "storage.delete",
                "fs.read",
                "fs.list",
                "fs.write",
                "fs.delete",
            ],
        ),
        bridge_group(
            "Projects and Topics",
            &[
                "projects.list",
                "projects.open",
                "project.profile.update",
                "project.sandbox.set",
                "project.apps.link",
                "project.apps.unlink",
                "topics.list",
                "topics.open",
                "skills.list",
                "project.skills.ensure",
                "project.skills.revoke",
                "mcp.servers",
                "project.mcp.upsert",
                "project.mcp.delete",
                "project.files.list",
                "project.files.read",
                "project.files.search",
                "project.files.write",
                "project.files.mkdir",
                "project.files.move",
                "project.files.copy",
                "project.files.delete",
            ],
        ),
        bridge_group(
            "Browser sidecar",
            &[
                "browser.init",
                "project.browser.setEnabled",
                "browser.tabs.list",
                "browser.open",
                "browser.close",
                "browser.setActive",
                "browser.navigate",
                "browser.back",
                "browser.forward",
                "browser.reload",
                "browser.currentUrl",
                "browser.readText",
                "browser.readOutline",
                "browser.screenshot",
                "browser.clickText",
                "browser.clickSelector",
                "browser.fill",
                "browser.scroll",
                "browser.waitFor",
            ],
        ),
        bridge_group(
            "Native macOS",
            &[
                "clipboard.readText",
                "clipboard.writeText",
                "dialog.openDirectory",
                "dialog.openFile",
                "dialog.saveFile",
                "notify.show",
            ],
        ),
        bridge_group("Network", &["net.fetch"]),
        bridge_group(
            "Memory",
            &[
                "memory.save",
                "memory.read",
                "memory.update",
                "memory.list",
                "memory.delete",
                "memory.search",
                "memory.recall",
                "memory.stats",
                "memory.reindex",
                "memory.indexPath",
                "memory.pathStatus",
                "memory.pathStatusBatch",
                "memory.forgetPath",
            ],
        ),
        bridge_group(
            "Automations",
            &[
                "scheduler.list",
                "scheduler.upsert",
                "scheduler.delete",
                "scheduler.runNow",
                "scheduler.setPaused",
                "scheduler.runs",
                "scheduler.stats",
                "scheduler.runDetail",
            ],
        ),
        bridge_group(
            "App Grid",
            &[
                "events.emit",
                "events.subscribe",
                "events.unsubscribe",
                "events.subscriptions",
                "events.recent",
                "events.clearSubscriptions",
                "apps.list",
                "apps.create",
                "apps.export",
                "apps.import",
                "apps.delete",
                "apps.trashList",
                "apps.restore",
                "apps.purge",
                "apps.status",
                "apps.diff",
                "apps.commit",
                "apps.commitPartial",
                "apps.revert",
                "apps.server.status",
                "apps.server.logs",
                "apps.server.start",
                "apps.server.stop",
                "apps.server.restart",
                "apps.open",
                "apps.invoke",
                "apps.list_actions",
            ],
        ),
    ];
    let helpers = vec![
        bridge_group(
            "Core",
            &[
                "reflexInvoke",
                "reflexBridgeCatalog",
                "reflexSystemContext",
                "reflexSystemOpenPanel",
                "reflexSystemOpenUrl",
                "reflexSystemOpenPath",
                "reflexSystemRevealPath",
                "reflexLog",
                "reflexLogList",
                "reflexManifestGet",
                "reflexManifestUpdate",
                "reflexIntegrationCatalog",
                "reflexIntegrationProfile",
                "reflexIntegrationUpdate",
                "reflexIntegrationLearnVisible",
                "reflexPermissionsList",
                "reflexPermissionsEnsure",
                "reflexPermissionsRevoke",
                "reflexNetworkHosts",
                "reflexNetworkAllowHost",
                "reflexNetworkRevokeHost",
                "reflexWidgetsList",
                "reflexWidgetsUpsert",
                "reflexWidgetsDelete",
                "reflexActionsList",
                "reflexActionsUpsert",
                "reflexActionsDelete",
                "reflexCapabilities",
            ],
        ),
        bridge_group(
            "Agent",
            &[
                "reflexAgentAsk",
                "reflexAgentStartTopic",
                "reflexAgentTask",
                "reflexAgentStream",
                "reflexAgentStreamAbort",
            ],
        ),
        bridge_group(
            "Storage / IO",
            &[
                "reflexStorageGet",
                "reflexStorageSet",
                "reflexStorageList",
                "reflexStorageDelete",
                "reflexFsRead",
                "reflexFsList",
                "reflexFsWrite",
                "reflexFsDelete",
                "reflexClipboardReadText",
                "reflexClipboardWriteText",
                "reflexNetFetch",
                "reflexNotifyShow",
                "reflexDialogOpenDirectory",
                "reflexDialogOpenFile",
                "reflexDialogSaveFile",
            ],
        ),
        bridge_group(
            "Projects / Browser",
            &[
                "reflexProjectsList",
                "reflexProjectsOpen",
                "reflexProjectProfileUpdate",
                "reflexProjectSandboxSet",
                "reflexProjectAppsLink",
                "reflexProjectAppsUnlink",
                "reflexTopicsList",
                "reflexTopicsOpen",
                "reflexSkillsList",
                "reflexProjectSkillsEnsure",
                "reflexProjectSkillsRevoke",
                "reflexMcpServers",
                "reflexProjectMcpUpsert",
                "reflexProjectMcpDelete",
                "reflexProjectFilesList",
                "reflexProjectFilesRead",
                "reflexProjectFilesSearch",
                "reflexProjectFilesWrite",
                "reflexProjectFilesMkdir",
                "reflexProjectFilesMove",
                "reflexProjectFilesCopy",
                "reflexProjectFilesDelete",
                "reflexBrowserInit",
                "reflexProjectBrowserSetEnabled",
                "reflexBrowserTabs",
                "reflexBrowserOpen",
                "reflexBrowserClose",
                "reflexBrowserSetActive",
                "reflexBrowserNavigate",
                "reflexBrowserBack",
                "reflexBrowserForward",
                "reflexBrowserReload",
                "reflexBrowserCurrentUrl",
                "reflexBrowserReadText",
                "reflexBrowserReadOutline",
                "reflexBrowserScreenshot",
                "reflexBrowserClickText",
                "reflexBrowserClickSelector",
                "reflexBrowserFill",
                "reflexBrowserScroll",
                "reflexBrowserWaitFor",
            ],
        ),
        bridge_group(
            "Memory / Automation / Apps",
            &[
                "reflexMemorySave",
                "reflexMemoryRead",
                "reflexMemoryUpdate",
                "reflexMemoryList",
                "reflexMemoryDelete",
                "reflexMemorySearch",
                "reflexMemoryRecall",
                "reflexMemoryStats",
                "reflexMemoryReindex",
                "reflexMemoryIndexPath",
                "reflexMemoryPathStatus",
                "reflexMemoryPathStatusBatch",
                "reflexMemoryForgetPath",
                "reflexSchedulerList",
                "reflexSchedulerUpsert",
                "reflexSchedulerDelete",
                "reflexSchedulerRunNow",
                "reflexSchedulerSetPaused",
                "reflexSchedulerRuns",
                "reflexSchedulerStats",
                "reflexSchedulerRunDetail",
                "reflexAppsList",
                "reflexAppsCreate",
                "reflexAppsExport",
                "reflexAppsImport",
                "reflexAppsDelete",
                "reflexAppsTrashList",
                "reflexAppsRestore",
                "reflexAppsPurge",
                "reflexAppsStatus",
                "reflexAppsDiff",
                "reflexAppsCommit",
                "reflexAppsCommitPartial",
                "reflexAppsRevert",
                "reflexAppsServerStatus",
                "reflexAppsServerLogs",
                "reflexAppsServerStart",
                "reflexAppsServerStop",
                "reflexAppsServerRestart",
                "reflexAppsOpen",
                "reflexAppsInvoke",
                "reflexAppsListActions",
                "reflexEventOn",
                "reflexEventOff",
                "reflexEventEmit",
                "reflexEventRecent",
                "reflexEventSubscriptions",
                "reflexEventClearSubscriptions",
            ],
        ),
    ];

    Ok(serde_json::json!({
        "version": 1,
        "methods": methods,
        "helpers": helpers,
        "permissions": bridge_permission_hints(),
        "app": {
            "id": manifest.id,
            "runtime": manifest.runtime.unwrap_or_else(|| "static".into()),
            "permissions": manifest.permissions,
            "network_hosts": manifest
                .network
                .map(|n| n.allowed_hosts)
                .unwrap_or_default(),
        },
        "notes": {
            "scheduler_ui_blocklist": [
                "dialog.*",
                "clipboard.*",
                "system.openPanel",
                "system.openUrl",
                "system.openPath",
                "system.revealPath",
                "apps.create",
                "apps.import",
                "apps.delete",
                "apps.restore",
                "apps.purge",
                "apps.open",
                "projects.open",
                "topics.open"
            ],
            "network": "net.fetch requires manifest.network.allowed_hosts",
            "cross_project": "linked projects are available by default; other projects require scoped permissions"
        }
    }))
}

fn bridge_group(title: &str, items: &[&str]) -> serde_json::Value {
    serde_json::json!({
        "title": title,
        "items": items,
    })
}

fn bridge_permission_hints() -> serde_json::Value {
    serde_json::json!([
        { "scope": "clipboard", "grants": ["clipboard.read", "clipboard.write", "clipboard:*"] },
        { "scope": "browser", "grants": ["browser.read", "browser.control", "browser:*", "browser.project:<project>"] },
        { "scope": "projects", "grants": ["projects.read:*", "projects.write:<project>", "projects.write:*"] },
        { "scope": "topics", "grants": ["topics.read:<project>", "topics.read:*"] },
        { "scope": "skills", "grants": ["skills.read:<project>", "skills.read:*", "skills.write:<project>", "skills.write:*"] },
        { "scope": "mcp", "grants": ["mcp.read:<project>", "mcp.read:*", "mcp.write:<project>", "mcp.write:*"] },
        { "scope": "project.files", "grants": ["project.files.read:<project>", "project.files.read:*", "project.files.write:<project>", "project.files.write:*"] },
        { "scope": "memory", "grants": ["memory.global.read", "memory.global.write"] },
        { "scope": "agent", "grants": ["agent.project:<project>", "agent.project:*", "agent.cwd:*"] },
        { "scope": "scheduler", "grants": ["scheduler.read:*", "scheduler.run:<app>", "scheduler.write:<app>::<schedule>", "scheduler.write:*", "scheduler:*"] },
        { "scope": "apps", "grants": ["apps.create", "apps.manage", "apps:*", "apps.invoke:*", "apps.invoke:<app>", "apps.invoke:<app>::<action>"] }
    ])
}

fn actions_list_for_app(app: &AppHandle, app_id: &str) -> Result<serde_json::Value, String> {
    let manifest = apps::read_manifest(app, app_id).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "actions": manifest.actions }))
}

fn actions_upsert_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let action = parse_action_upsert(params)?;
    let mut manifest = apps::read_manifest(app, app_id).map_err(|e| e.to_string())?;
    let created = match manifest
        .actions
        .iter()
        .position(|existing| existing.id == action.id)
    {
        Some(idx) => {
            manifest.actions[idx] = action.clone();
            false
        }
        None => {
            manifest.actions.push(action.clone());
            true
        }
    };
    apps::write_manifest(app, app_id, &manifest).map_err(|e| e.to_string())?;
    emit_apps_changed(app);
    Ok(serde_json::json!({
        "ok": true,
        "created": created,
        "action": action,
    }))
}

fn actions_delete_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let id = string_param(&params, "action_id", "actionId")
        .or_else(|| string_param(&params, "id", "id"))
        .ok_or_else(|| "missing action_id".to_string())?;
    validate_action_id(&id)?;
    let mut manifest = apps::read_manifest(app, app_id).map_err(|e| e.to_string())?;
    let removed = manifest.actions.iter().find(|action| action.id == id).cloned();
    let Some(removed) = removed else {
        return Ok(serde_json::json!({
            "ok": true,
            "deleted": false,
            "action_id": id,
        }));
    };
    manifest.actions.retain(|action| action.id != id);
    apps::write_manifest(app, app_id, &manifest).map_err(|e| e.to_string())?;
    emit_apps_changed(app);
    Ok(serde_json::json!({
        "ok": true,
        "deleted": true,
        "action_id": id,
        "action": removed,
    }))
}

fn parse_action_upsert(params: serde_json::Value) -> Result<apps::ActionDef, String> {
    let mut value = params
        .get("action")
        .cloned()
        .unwrap_or(params);
    let obj = value
        .as_object_mut()
        .ok_or_else(|| "action must be a JSON object".to_string())?;
    let id = obj
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .ok_or_else(|| "action.id is required".to_string())?;
    validate_action_id(&id)?;
    obj.insert("id".into(), serde_json::Value::String(id.clone()));
    if !obj.contains_key("name") {
        obj.insert("name".into(), serde_json::Value::String(id));
    }
    if let Some(params_schema) = obj.remove("paramsSchema") {
        obj.entry("params_schema").or_insert(params_schema);
    }
    let mut action: apps::ActionDef =
        serde_json::from_value(value).map_err(|e| format!("invalid action: {e}"))?;
    action.id = action.id.trim().to_string();
    action.name = action.name.trim().to_string();
    validate_action_def(&action)?;
    if let Some(desc) = action.description.as_mut() {
        *desc = desc.trim().to_string();
    }
    Ok(action)
}

fn validate_action_def(action: &apps::ActionDef) -> Result<(), String> {
    validate_action_id(&action.id)?;
    if action.name.trim().is_empty() {
        return Err("action.name is required".into());
    }
    if action.steps.is_empty() {
        return Err("action.steps must contain at least one step".into());
    }
    if let Some(schema) = &action.params_schema {
        if !schema.is_object() {
            return Err("action.params_schema must be a JSON object".into());
        }
    }
    for step in &action.steps {
        if step.method.trim().is_empty() {
            return Err("action step method is required".into());
        }
    }
    Ok(())
}

fn validate_action_id(id: &str) -> Result<(), String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("action.id is required".into());
    }
    if id.len() > 80 {
        return Err("action.id must be 80 characters or fewer".into());
    }
    if !id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err("action.id may contain only ASCII letters, numbers, '-', '_' or '.'".into());
    }
    Ok(())
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

fn system_open_panel(
    app: &AppHandle,
    app_id: &str,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let raw_panel = params
        .get("panel")
        .or_else(|| params.get("name"))
        .and_then(|value| value.as_str())
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "missing panel".to_string())?;
    let panel = normalize_system_panel(raw_panel)?;
    let project_id = string_param(params, "project_id", "projectId");
    let thread_id = string_param(params, "thread_id", "threadId");

    let mut payload = serde_json::Map::new();
    payload.insert("panel".into(), serde_json::Value::String(panel.to_string()));
    payload.insert("from_app".into(), serde_json::Value::String(app_id.to_string()));
    if let Some(project_id) = project_id.clone() {
        payload.insert("project_id".into(), serde_json::Value::String(project_id));
    }
    if let Some(thread_id) = thread_id.clone() {
        payload.insert("thread_id".into(), serde_json::Value::String(thread_id));
    }
    app.emit(
        "reflex://app-open-request",
        &serde_json::Value::Object(payload),
    )
    .map_err(|e| e.to_string())?;

    let mut out = serde_json::Map::new();
    out.insert("ok".into(), serde_json::Value::Bool(true));
    out.insert("panel".into(), serde_json::Value::String(panel.to_string()));
    if let Some(project_id) = project_id {
        out.insert("project_id".into(), serde_json::Value::String(project_id));
    }
    if let Some(thread_id) = thread_id {
        out.insert("thread_id".into(), serde_json::Value::String(thread_id));
    }
    Ok(serde_json::Value::Object(out))
}

fn normalize_system_panel(raw: &str) -> Result<&'static str, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "apps" | "app" | "utilities" | "utils" | "app-grid" | "apps-grid" | "утилиты"
        | "приложения" => Ok("apps"),
        "memory" | "memories" | "rag" | "knowledge" | "память" => Ok("memory"),
        "automations" | "automation" | "schedules" | "schedule" | "scheduler"
        | "автоматизации" | "расписания" => Ok("automations"),
        "browser" | "web" | "браузер" => Ok("browser"),
        "settings" | "preferences" | "prefs" | "logs" | "настройки" => Ok("settings"),
        other => Err(format!(
            "invalid panel: {other}; expected apps, memory, automations, browser, or settings"
        )),
    }
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

fn logs_write_for_app(
    app: &AppHandle,
    app_id: &str,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let level = parse_log_level(
        &string_param(params, "level", "level").unwrap_or_else(|| "info".to_string()),
    );
    let message = params
        .get("message")
        .or_else(|| params.get("body"))
        .and_then(|v| v.as_str())
        .ok_or("missing message")?;
    let mut message: String = message.chars().take(2_000).collect();
    if message.trim().is_empty() {
        message = "(empty app log message)".into();
    }

    let source_suffix = string_param(params, "source", "source").and_then(sanitize_log_source);
    let source = source_suffix
        .map(|suffix| format!("app:{app_id}:{suffix}"))
        .unwrap_or_else(|| format!("app:{app_id}"));

    logs::log_with(app, level, &source, message);
    Ok(serde_json::json!({ "ok": true }))
}

fn logs_list_for_app(
    app: &AppHandle,
    app_id: &str,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let limit = params
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(100)
        .clamp(1, 500) as usize;
    let since_seq = params
        .get("since_seq")
        .or_else(|| params.get("sinceSeq"))
        .and_then(|v| v.as_u64());
    let source = string_param(params, "source", "source").and_then(sanitize_log_source);
    let level = string_param(params, "level", "level").map(|raw| parse_log_level(&raw));
    let source_base = format!("app:{app_id}");
    let source_exact = source.map(|suffix| format!("{source_base}:{suffix}"));

    let store = app.state::<logs::LogStore>();
    let mut entries = store.snapshot(usize::MAX, since_seq);
    entries.retain(|entry| is_app_log_source(&entry.source, app_id));
    if let Some(source_exact) = source_exact {
        entries.retain(|entry| entry.source == source_exact);
    }
    if let Some(level) = level {
        entries.retain(|entry| entry.level == level);
    }
    if entries.len() > limit {
        let drop = entries.len() - limit;
        entries.drain(0..drop);
    }
    let latest_seq = entries
        .last()
        .map(|entry| entry.seq)
        .unwrap_or_else(|| since_seq.unwrap_or(0));
    Ok(serde_json::json!({
        "entries": entries,
        "latestSeq": latest_seq,
    }))
}

fn parse_log_level(raw: &str) -> logs::LogLevel {
    match raw.to_lowercase().as_str() {
        "trace" => logs::LogLevel::Trace,
        "debug" => logs::LogLevel::Debug,
        "warn" | "warning" => logs::LogLevel::Warn,
        "error" | "err" => logs::LogLevel::Error,
        _ => logs::LogLevel::Info,
    }
}

fn sanitize_log_source(raw: String) -> Option<String> {
    let source = raw
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':'))
        .take(48)
        .collect::<String>();
    if source.is_empty() {
        None
    } else {
        Some(source)
    }
}

fn is_app_log_source(source: &str, app_id: &str) -> bool {
    let source_base = format!("app:{app_id}");
    let source_prefix = format!("{source_base}:");
    source == source_base || source.starts_with(&source_prefix)
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
    project: Option<project::Project>,
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
        let mcp_servers = project.as_ref().and_then(|p| p.mcp_servers.clone());
        return Ok(AgentCwdTarget {
            cwd: cwd_path,
            mcp_servers,
            project,
        });
    }

    if let Some(project) = project::find_project_for(&cwd_path) {
        if project_is_linked_to_app(app, app_id, &project)
            || scoped_permission_allowed(app, app_id, "agent.project", &project.id)
        {
            let mcp_servers = project.mcp_servers.clone();
            return Ok(AgentCwdTarget {
                cwd: cwd_path,
                mcp_servers,
                project: Some(project),
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
            project: None,
        });
    }

    Err(
        "permission denied: agent cwd must be this app root, a linked project, or require manifest.permissions entry 'agent.cwd:*'"
            .into(),
    )
}

async fn agent_task_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
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
    let cwd_target = resolve_agent_cwd_for_app(
        app,
        app_id,
        params.get("cwd").and_then(|v| v.as_str()),
    )?;
    let prompt = build_app_agent_prompt(app_id, &params, prompt, &cwd_target).await;

    let handle = app.state::<app_server::AppServerHandle>();
    let server = handle.wait().await;
    let app_thread_id = server
        .thread_start(&cwd_target.cwd, &sandbox, cwd_target.mcp_servers.as_ref())
        .await
        .map_err(|e| format!("thread_start: {e}"))?;
    let _ = server.turn_start(&app_thread_id, &prompt).await;
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

async fn build_app_agent_prompt(
    app_id: &str,
    params: &serde_json::Value,
    prompt: String,
    target: &AgentCwdTarget,
) -> String {
    if bool_param(params, "include_context", "includeContext") == Some(false) {
        return prompt;
    }
    let Some(project) = target.project.as_ref() else {
        return prompt;
    };
    let project_root = PathBuf::from(&project.root);
    let thread_id = string_param(params, "memory_thread_id", "memoryThreadId")
        .or_else(|| string_param(params, "thread_id", "threadId"))
        .unwrap_or_else(|| format!("app:{app_id}"));
    let prompt = match memory::injection::build_preface(&project_root, &thread_id, &prompt).await {
        Ok(r) => memory::injection::wrap_user_prompt(&r.preface, &prompt),
        Err(e) => {
            eprintln!("[reflex] app agent memory inject failed: {e}");
            prompt
        }
    };
    let profile = crate::project_agent_profile_preface(project);
    crate::wrap_with_project_agent_profile(&profile, &prompt)
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

fn memory_read_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let scope = parse_scope(&params, MemoryScope::Project)?;
    if scope == MemoryScope::Global {
        ensure_global_memory_permission(app, app_id, false)?;
    }
    let rel_path = parse_memory_rel_path(&params)?;
    let target = resolve_memory_target(app, app_id, &params)?;
    let roots = scope_roots(&target)?;
    let note = store::read(&roots, scope, &rel_path).map_err(|e| e.to_string())?;
    Ok(serde_json::to_value(note).unwrap_or(serde_json::Value::Null))
}

async fn memory_update_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let scope = parse_scope(&params, MemoryScope::Project)?;
    if scope == MemoryScope::Global {
        ensure_global_memory_permission(app, app_id, true)?;
    }
    let rel_path = parse_memory_rel_path(&params)?;
    let target = resolve_memory_target(app, app_id, &params)?;
    let roots = scope_roots(&target)?;
    let existing = store::read(&roots, scope, &rel_path).map_err(|e| e.to_string())?;
    let body = params
        .get("body")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| existing.body.clone());
    let tags = params
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| existing.front.tags.clone());
    let source = params
        .get("source")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| existing.front.source.clone());
    let req = SaveRequest {
        scope,
        kind: parse_kind(&params, existing.front.kind)?,
        name: params
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| existing.front.name.clone()),
        description: params
            .get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| existing.front.description.clone()),
        body: body.clone(),
        rel_path: Some(rel_path.clone()),
        tags,
        source,
    };
    let note = store::save(&roots, req).map_err(|e| e.to_string())?;
    if scope != MemoryScope::Global {
        let doc_id = format!("memory:{}", rel_path.display());
        let root = target.root.clone();
        tokio::spawn(async move {
            let _ = rag::index_text(&root, &doc_id, "memory", &body).await;
        });
    }
    Ok(serde_json::to_value(note).unwrap_or(serde_json::Value::Null))
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
    let rel_path = parse_memory_rel_path(&params)?;
    let target = resolve_memory_target(app, app_id, &params)?;
    let roots = scope_roots(&target)?;
    store::delete(&roots, scope, &rel_path).map_err(|e| e.to_string())?;
    if scope != MemoryScope::Global {
        let doc_id = format!("memory:{}", rel_path.display());
        let root = target.root.clone();
        tokio::spawn(async move {
            let _ = rag::forget(&root, &doc_id).await;
        });
    }
    Ok(serde_json::json!({ "ok": true }))
}

fn parse_memory_rel_path(params: &serde_json::Value) -> Result<PathBuf, String> {
    let raw = params
        .get("rel_path")
        .or_else(|| params.get("relPath"))
        .and_then(|v| v.as_str())
        .ok_or("missing rel_path")?
        .trim();
    if raw.is_empty() {
        return Err("rel_path must be non-empty".into());
    }
    let path = PathBuf::from(raw);
    if path.is_absolute()
        || path
            .components()
            .any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
    {
        return Err("rel_path must stay inside the memory scope".into());
    }
    Ok(path)
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

async fn memory_reindex_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let target = resolve_memory_target(app, app_id, &params)?;
    let indexed = rag::reindex_project(&target.root)
        .await
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "indexed": indexed }))
}

fn memory_stats_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let target = resolve_memory_target(app, app_id, &params)?;
    let stats = files::stats(&target.root).map_err(|e| e.to_string())?;
    Ok(serde_json::to_value(stats).unwrap_or(serde_json::Value::Null))
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

fn parse_memory_paths(params: &serde_json::Value) -> Result<Vec<String>, String> {
    let paths = params
        .get("paths")
        .and_then(|v| v.as_array())
        .ok_or("missing paths array")?;
    if paths.is_empty() {
        return Err("paths must be non-empty array of strings".into());
    }
    paths
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .map(str::to_string)
                .ok_or_else(|| "paths must be non-empty array of strings".to_string())
        })
        .collect()
}

fn memory_path_status_batch_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let raw_paths = parse_memory_paths(&params)?;
    let target = resolve_memory_target(app, app_id, &params)?;
    let paths = raw_paths
        .iter()
        .map(|path| resolve_project_path(&target, path))
        .collect::<Result<Vec<_>, _>>()?;
    let statuses = files::status_batch(&target.root, &paths).map_err(|e| e.to_string())?;
    Ok(serde_json::to_value(statuses).unwrap_or(serde_json::Value::Array(vec![])))
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
        if let Some(s) = t.as_str().map(str::trim).filter(|s| !s.is_empty()) {
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
        .map(|listing| app_summary_from_manifest(listing.manifest, listing.ready))
        .collect();
    Ok(serde_json::Value::Array(out))
}

fn app_summary_from_manifest(manifest: apps::AppManifest, ready: bool) -> serde_json::Value {
    serde_json::json!({
        "id": manifest.id,
        "name": manifest.name,
        "icon": manifest.icon,
        "description": manifest.description,
        "kind": manifest.kind,
        "runtime": manifest.runtime.unwrap_or_else(|| "static".into()),
        "external": manifest.external.as_ref().map(|external| serde_json::json!({
            "url": external.url.clone(),
            "title": external.title.clone(),
            "open_url": external.open_url.clone(),
        })),
        "integration": manifest.integration.as_ref().map(|integration| serde_json::json!({
            "provider": integration.provider.clone(),
            "display_name": integration.display_name.clone(),
            "capabilities": integration.capabilities.clone(),
        })),
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
}

fn ensure_apps_create_permission(app: &AppHandle, app_id: &str) -> Result<(), String> {
    let manifest = apps::read_manifest(app, app_id).map_err(|e| e.to_string())?;
    if manifest
        .permissions
        .iter()
        .any(|permission| matches!(permission.as_str(), "*" | "apps:*" | "apps.create"))
    {
        Ok(())
    } else {
        Err(
            "permission denied: apps.create requires manifest.permissions entry 'apps.create' or 'apps:*'"
                .into(),
        )
    }
}

fn ensure_apps_manage_permission(app: &AppHandle, app_id: &str) -> Result<(), String> {
    let manifest = apps::read_manifest(app, app_id).map_err(|e| e.to_string())?;
    if manifest
        .permissions
        .iter()
        .any(|permission| matches!(permission.as_str(), "*" | "apps:*" | "apps.manage"))
    {
        Ok(())
    } else {
        Err(
            "permission denied: app lifecycle management requires manifest.permissions entry 'apps.manage' or 'apps:*'"
                .into(),
        )
    }
}

async fn apps_create_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    ensure_apps_create_permission(app, app_id)?;
    let description = string_param(&params, "description", "description")
        .or_else(|| string_param(&params, "prompt", "prompt"))
        .ok_or_else(|| "missing description".to_string())?;
    let template = string_param(&params, "template", "template");
    let project_id = if string_param(&params, "project_id", "projectId").is_some() {
        let project = resolve_project_write_target(app, app_id, &params, "projects.write")?;
        Some(project.id)
    } else {
        None
    };
    crate::create_app(app.clone(), description, template, project_id).await
}

fn apps_export_for_app(
    app: &AppHandle,
    caller_app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    ensure_apps_manage_permission(app, caller_app_id)?;
    let target_app_id = required_string_param(&params, "app_id", "appId")?;
    let target_path = required_string_param(&params, "target_path", "targetPath")?;
    apps::export_app(app, &target_app_id, Path::new(&target_path)).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "ok": true,
        "app_id": target_app_id,
        "path": target_path,
    }))
}

fn apps_import_for_app(
    app: &AppHandle,
    caller_app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    ensure_apps_manage_permission(app, caller_app_id)?;
    let zip_path = required_string_param(&params, "zip_path", "zipPath")?;
    let manifest = apps::import_app(app, Path::new(&zip_path)).map_err(|e| e.to_string())?;
    app.emit(
        "reflex://apps-changed",
        &serde_json::json!({ "app_id": manifest.id }),
    )
    .map_err(|e| e.to_string())?;
    let ready = apps::app_dir(app, &manifest.id)
        .map(|dir| dir.join(&manifest.entry).exists())
        .unwrap_or(false);
    Ok(serde_json::json!({
        "ok": true,
        "app": app_summary_from_manifest(manifest, ready),
    }))
}

fn apps_delete_for_app(
    app: &AppHandle,
    caller_app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    ensure_apps_manage_permission(app, caller_app_id)?;
    let target_id = string_param(&params, "app_id", "appId")
        .ok_or_else(|| "missing app_id".to_string())?;
    if target_id == caller_app_id {
        return Err("apps.delete cannot delete the calling app".into());
    }
    let entry = crate::delete_app(app.clone(), target_id)?;
    serde_json::to_value(entry).map_err(|e| e.to_string())
}

fn apps_trash_list_for_app(
    app: &AppHandle,
    caller_app_id: &str,
) -> Result<serde_json::Value, String> {
    ensure_apps_manage_permission(app, caller_app_id)?;
    let entries = crate::list_trashed_apps(app.clone())?;
    serde_json::to_value(entries).map_err(|e| e.to_string())
}

fn apps_restore_for_app(
    app: &AppHandle,
    caller_app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    ensure_apps_manage_permission(app, caller_app_id)?;
    let trash_id = string_param(&params, "trash_id", "trashId")
        .ok_or_else(|| "missing trash_id".to_string())?;
    let app_id = crate::restore_app(app.clone(), trash_id)?;
    Ok(serde_json::json!({ "ok": true, "app_id": app_id }))
}

fn apps_purge_for_app(
    app: &AppHandle,
    caller_app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    ensure_apps_manage_permission(app, caller_app_id)?;
    let trash_id = string_param(&params, "trash_id", "trashId")
        .ok_or_else(|| "missing trash_id".to_string())?;
    crate::purge_trashed_app(app.clone(), trash_id)?;
    Ok(serde_json::json!({ "ok": true }))
}

fn apps_status_for_app(
    app: &AppHandle,
    caller_app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    ensure_apps_manage_permission(app, caller_app_id)?;
    let target_app_id = required_string_param(&params, "app_id", "appId")?;
    crate::app_status(app.clone(), target_app_id)
}

fn apps_diff_for_app(
    app: &AppHandle,
    caller_app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    ensure_apps_manage_permission(app, caller_app_id)?;
    let target_app_id = required_string_param(&params, "app_id", "appId")?;
    let diff = crate::app_diff(app.clone(), target_app_id.clone())?;
    Ok(serde_json::json!({ "app_id": target_app_id, "diff": diff }))
}

fn apps_commit_for_app(
    app: &AppHandle,
    caller_app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    ensure_apps_manage_permission(app, caller_app_id)?;
    let target_app_id = required_string_param(&params, "app_id", "appId")?;
    let message = string_param(&params, "message", "message");
    crate::app_save(app.clone(), target_app_id.clone(), message)?;
    Ok(serde_json::json!({ "ok": true, "app_id": target_app_id }))
}

fn apps_commit_partial_for_app(
    app: &AppHandle,
    caller_app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    ensure_apps_manage_permission(app, caller_app_id)?;
    let target_app_id = required_string_param(&params, "app_id", "appId")?;
    let patch = required_string_param(&params, "patch", "patch")?;
    let message = string_param(&params, "message", "message");
    crate::app_save_partial(app.clone(), target_app_id.clone(), patch, message)?;
    Ok(serde_json::json!({ "ok": true, "app_id": target_app_id }))
}

fn apps_revert_for_app(
    app: &AppHandle,
    caller_app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    ensure_apps_manage_permission(app, caller_app_id)?;
    let target_app_id = required_string_param(&params, "app_id", "appId")?;
    crate::app_revert(app.clone(), target_app_id.clone())?;
    Ok(serde_json::json!({ "ok": true, "app_id": target_app_id }))
}

async fn apps_server_status_for_app(
    app: &AppHandle,
    caller_app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    ensure_apps_manage_permission(app, caller_app_id)?;
    let target_app_id = required_string_param(&params, "app_id", "appId")?;
    apps::read_manifest(app, &target_app_id).map_err(|e| e.to_string())?;
    let runtimes = app.state::<app_runtime::AppRuntimes>();
    let status = app_runtime::status(&runtimes, &target_app_id).await;
    serde_json::to_value(status).map_err(|e| e.to_string())
}

async fn apps_server_logs_for_app(
    app: &AppHandle,
    caller_app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    ensure_apps_manage_permission(app, caller_app_id)?;
    let target_app_id = required_string_param(&params, "app_id", "appId")?;
    apps::read_manifest(app, &target_app_id).map_err(|e| e.to_string())?;
    let runtimes = app.state::<app_runtime::AppRuntimes>();
    let logs = app_runtime::logs(&runtimes, &target_app_id).await;
    serde_json::to_value(logs).map_err(|e| e.to_string())
}

async fn apps_server_start_for_app(
    app: &AppHandle,
    caller_app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    ensure_apps_manage_permission(app, caller_app_id)?;
    let target_app_id = required_string_param(&params, "app_id", "appId")?;
    let runtimes = app.state::<app_runtime::AppRuntimes>();
    let port = app_runtime::ensure_started(&runtimes, app, &target_app_id).await?;
    Ok(serde_json::json!({ "ok": true, "app_id": target_app_id, "port": port }))
}

async fn apps_server_stop_for_app(
    app: &AppHandle,
    caller_app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    ensure_apps_manage_permission(app, caller_app_id)?;
    let target_app_id = required_string_param(&params, "app_id", "appId")?;
    apps::read_manifest(app, &target_app_id).map_err(|e| e.to_string())?;
    let runtimes = app.state::<app_runtime::AppRuntimes>();
    app_runtime::stop(&runtimes, &target_app_id).await;
    Ok(serde_json::json!({ "ok": true, "app_id": target_app_id }))
}

async fn apps_server_restart_for_app(
    app: &AppHandle,
    caller_app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    ensure_apps_manage_permission(app, caller_app_id)?;
    let target_app_id = required_string_param(&params, "app_id", "appId")?;
    let runtimes = app.state::<app_runtime::AppRuntimes>();
    let port = app_runtime::restart(&runtimes, app, &target_app_id).await?;
    Ok(serde_json::json!({ "ok": true, "app_id": target_app_id, "port": port }))
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

fn project_profile_update_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let mut project = resolve_project_write_target(app, app_id, &params, "projects.write")?;
    let mut changed = Vec::new();

    if params.get("description").is_some() {
        project.description = normalize_optional_project_text(
            params.get("description").unwrap(),
            "description",
            2_000,
        )?;
        changed.push("description");
    }

    if let Some(value) = params
        .get("agent_instructions")
        .or_else(|| params.get("agentInstructions"))
    {
        project.agent_instructions =
            normalize_optional_project_text(value, "agentInstructions", 20_000)?;
        changed.push("agent_instructions");
    }

    if changed.is_empty() {
        return Err("project.profile.update requires description or agentInstructions".into());
    }

    write_project_and_register(app, &project)?;
    Ok(serde_json::json!({
        "ok": true,
        "changed": changed,
        "project": project_summary(&project),
    }))
}

fn project_sandbox_set_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let mut project = resolve_project_write_target(app, app_id, &params, "projects.write")?;
    let sandbox = normalize_project_sandbox(&required_string_param(&params, "sandbox", "sandbox")?)?;
    let changed = project.sandbox != sandbox;
    project.sandbox = sandbox.clone();
    write_project_and_register(app, &project)?;
    Ok(serde_json::json!({
        "ok": true,
        "changed": changed,
        "sandbox": sandbox,
        "project": project_summary(&project),
    }))
}

fn project_apps_link_for_app(
    app: &AppHandle,
    caller_app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let mut project = resolve_project_write_target(app, caller_app_id, &params, "projects.write")?;
    let target_app_id = project_app_target_param(&params).unwrap_or_else(|| caller_app_id.into());
    apps::read_manifest(app, &target_app_id)
        .map_err(|e| format!("app not found or unreadable: {target_app_id}: {e}"))?;
    let linked = if project.apps.iter().any(|id| id == &target_app_id) {
        false
    } else {
        project.apps.push(target_app_id.clone());
        true
    };
    write_project_and_register(app, &project)?;
    Ok(serde_json::json!({
        "ok": true,
        "linked": linked,
        "app_id": target_app_id,
        "project": project_summary(&project),
    }))
}

fn project_apps_unlink_for_app(
    app: &AppHandle,
    caller_app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let mut project = resolve_project_write_target(app, caller_app_id, &params, "projects.write")?;
    let target_app_id = project_app_target_param(&params).unwrap_or_else(|| caller_app_id.into());
    let before = project.apps.len();
    project.apps.retain(|id| id != &target_app_id);
    let unlinked = project.apps.len() != before;
    write_project_and_register(app, &project)?;
    Ok(serde_json::json!({
        "ok": true,
        "unlinked": unlinked,
        "app_id": target_app_id,
        "project": project_summary(&project),
    }))
}

fn resolve_project_write_target(
    app: &AppHandle,
    app_id: &str,
    params: &serde_json::Value,
    scope: &str,
) -> Result<project::Project, String> {
    if let Some(project_id) = string_param(params, "project_id", "projectId") {
        let project = list_user_projects(app)?
            .into_iter()
            .find(|project| project.id == project_id)
            .ok_or_else(|| format!("project not found: {project_id}"))?;
        ensure_scoped_permission(app, app_id, scope, &project.id)?;
        return Ok(project);
    }
    let targets: Vec<project::Project> = list_user_projects(app)?
        .into_iter()
        .filter(|project| scoped_permission_allowed(app, app_id, scope, &project.id))
        .collect();
    if targets.len() == 1 {
        return Ok(targets.into_iter().next().expect("one project write target"));
    }
    if targets.is_empty() {
        return Err(format!(
            "missing project_id; add manifest.permissions entry '{scope}:<project>' or '{scope}:*'"
        ));
    }
    Err(format!(
        "missing project_id; multiple {scope} targets are available"
    ))
}

fn normalize_optional_project_text(
    value: &serde_json::Value,
    label: &str,
    max_len: usize,
) -> Result<Option<String>, String> {
    if value.is_null() {
        return Ok(None);
    }
    let text = value
        .as_str()
        .ok_or_else(|| format!("{label} must be a string or null"))?
        .trim()
        .to_string();
    if text.is_empty() {
        return Ok(None);
    }
    if text.len() > max_len {
        return Err(format!("{label} must be {max_len} characters or fewer"));
    }
    Ok(Some(text))
}

fn project_app_target_param(params: &serde_json::Value) -> Option<String> {
    string_param(params, "app_id", "appId")
}

fn normalize_project_sandbox(raw: &str) -> Result<String, String> {
    let sandbox = raw.trim();
    match sandbox {
        "read-only" | "workspace-write" | "danger-full-access" => Ok(sandbox.to_string()),
        _ => Err(format!("invalid sandbox: {sandbox}")),
    }
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

fn project_skills_ensure_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let mut project = resolve_project_skill_write_target(app, app_id, &params)?;
    let skills = parse_project_skill_names(&params)?;
    let mut added = Vec::new();
    for skill in skills {
        if !project
            .skills
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(&skill))
        {
            project.skills.push(skill.clone());
            added.push(skill);
        }
    }
    write_project_and_register(app, &project)?;
    Ok(serde_json::json!({
        "ok": true,
        "project_id": project.id,
        "project_name": project.name,
        "added": added,
        "skills": project.skills,
    }))
}

fn project_skills_revoke_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let mut project = resolve_project_skill_write_target(app, app_id, &params)?;
    let skills = parse_project_skill_names(&params)?;
    let lower: std::collections::HashSet<String> =
        skills.iter().map(|skill| skill.to_ascii_lowercase()).collect();
    let before = project.skills.clone();
    project
        .skills
        .retain(|skill| !lower.contains(&skill.to_ascii_lowercase()));
    let removed: Vec<String> = before
        .into_iter()
        .filter(|skill| lower.contains(&skill.to_ascii_lowercase()))
        .collect();
    write_project_and_register(app, &project)?;
    Ok(serde_json::json!({
        "ok": true,
        "project_id": project.id,
        "project_name": project.name,
        "removed": removed,
        "skills": project.skills,
    }))
}

fn resolve_project_skill_write_target(
    app: &AppHandle,
    app_id: &str,
    params: &serde_json::Value,
) -> Result<project::Project, String> {
    if let Some(project_id) = string_param(params, "project_id", "projectId") {
        let project = list_user_projects(app)?
            .into_iter()
            .find(|project| project.id == project_id)
            .ok_or_else(|| format!("project not found: {project_id}"))?;
        ensure_scoped_permission(app, app_id, "skills.write", &project.id)?;
        return Ok(project);
    }
    let targets: Vec<project::Project> = list_user_projects(app)?
        .into_iter()
        .filter(|project| scoped_permission_allowed(app, app_id, "skills.write", &project.id))
        .collect();
    if targets.len() == 1 {
        return Ok(targets.into_iter().next().expect("one skill write target"));
    }
    if targets.is_empty() {
        return Err(
            "missing project_id; add manifest.permissions entry 'skills.write:<project>' or 'skills.write:*'"
                .into(),
        );
    }
    Err("missing project_id; multiple skills.write targets are available".into())
}

fn parse_project_skill_names(params: &serde_json::Value) -> Result<Vec<String>, String> {
    let raw = parse_string_list(params, "skill", "skills")?;
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for skill in raw {
        let skill = normalize_project_skill_name(&skill)?;
        if seen.insert(skill.to_ascii_lowercase()) {
            out.push(skill);
        }
    }
    if out.is_empty() {
        return Err("skills must include at least one skill".into());
    }
    Ok(out)
}

fn normalize_project_skill_name(raw: &str) -> Result<String, String> {
    let skill = raw.trim();
    if skill.is_empty() {
        return Err("skill must be non-empty".into());
    }
    if skill.len() > 160 {
        return Err("skill must be 160 characters or fewer".into());
    }
    if skill.chars().any(|ch| ch.is_control() || ch == ',' || ch == '\n' || ch == '\r') {
        return Err("skill must not contain commas or control characters".into());
    }
    Ok(skill.to_string())
}

fn write_project_and_register(app: &AppHandle, project: &project::Project) -> Result<(), String> {
    project::write_project(&PathBuf::from(&project.root), project).map_err(|e| e.to_string())?;
    project::register(app, project).map_err(|e| e.to_string())
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

fn project_mcp_upsert_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let mut project = resolve_project_mcp_write_target(app, app_id, &params)?;
    let name = normalize_mcp_server_name(
        &mcp_server_name_param(&params).ok_or("missing name or serverName")?,
    )?;
    let config = normalize_mcp_server_config(
        params
            .get("config")
            .cloned()
            .or_else(|| params.get("server").cloned())
            .ok_or("missing config")?,
    )?;
    let mut servers = project
        .mcp_servers
        .clone()
        .unwrap_or_else(|| serde_json::json!({}));
    let object = servers
        .as_object_mut()
        .ok_or("project mcp_servers must be a JSON object")?;
    let replaced = object.insert(name.clone(), config.clone()).is_some();
    let server_names = sorted_mcp_server_names(&servers);
    project.mcp_servers = Some(servers);
    write_project_and_register(app, &project)?;
    Ok(serde_json::json!({
        "ok": true,
        "project_id": project.id,
        "project_name": project.name,
        "name": name,
        "replaced": replaced,
        "server": config,
        "server_names": server_names,
    }))
}

fn project_mcp_delete_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let mut project = resolve_project_mcp_write_target(app, app_id, &params)?;
    let names = parse_mcp_server_names(&params)?;
    let mut removed = Vec::new();
    if let Some(servers) = project.mcp_servers.as_mut() {
        let object = servers
            .as_object_mut()
            .ok_or("project mcp_servers must be a JSON object")?;
        for name in names {
            if object.remove(&name).is_some() {
                removed.push(name);
            }
        }
    }
    let next_names = project
        .mcp_servers
        .as_ref()
        .map(sorted_mcp_server_names)
        .unwrap_or_default();
    if next_names.is_empty() {
        project.mcp_servers = None;
    }
    write_project_and_register(app, &project)?;
    Ok(serde_json::json!({
        "ok": true,
        "project_id": project.id,
        "project_name": project.name,
        "removed": removed,
        "server_names": next_names,
    }))
}

fn project_browser_set_enabled_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let mut project = resolve_project_mcp_write_target(app, app_id, &params)?;
    let enabled = params
        .get("enabled")
        .and_then(|v| v.as_bool())
        .ok_or("missing enabled")?;
    let server_names = set_project_browser_mcp_enabled(app, &mut project, enabled)?;
    write_project_and_register(app, &project)?;
    Ok(serde_json::json!({
        "ok": true,
        "project_id": project.id,
        "project_name": project.name,
        "enabled": enabled,
        "server_names": server_names,
    }))
}

fn set_project_browser_mcp_enabled(
    app: &AppHandle,
    project: &mut project::Project,
    enabled: bool,
) -> Result<Vec<String>, String> {
    let mut servers = project
        .mcp_servers
        .clone()
        .unwrap_or_else(|| serde_json::json!({}));
    let object = servers
        .as_object_mut()
        .ok_or("project mcp_servers must be a JSON object")?;

    if enabled {
        let bridge = browser::mcp_bridge_path(app).map_err(|e| format!("bridge path: {e}"))?;
        let node = browser::resolve_node().unwrap_or_else(|_| "node".to_string());
        object.insert(
            "reflex_browser".to_string(),
            serde_json::json!({
                "command": node,
                "args": [bridge.to_string_lossy()],
            }),
        );
        object.remove("playwright");
    } else {
        object.remove("reflex_browser");
        object.remove("playwright");
    }

    let server_names = sorted_mcp_server_names(&servers);
    project.mcp_servers = if server_names.is_empty() {
        None
    } else {
        Some(servers)
    };
    Ok(server_names)
}

fn resolve_project_mcp_write_target(
    app: &AppHandle,
    app_id: &str,
    params: &serde_json::Value,
) -> Result<project::Project, String> {
    if let Some(project_id) = string_param(params, "project_id", "projectId") {
        let project = list_user_projects(app)?
            .into_iter()
            .find(|project| project.id == project_id)
            .ok_or_else(|| format!("project not found: {project_id}"))?;
        ensure_scoped_permission(app, app_id, "mcp.write", &project.id)?;
        return Ok(project);
    }
    let targets: Vec<project::Project> = list_user_projects(app)?
        .into_iter()
        .filter(|project| scoped_permission_allowed(app, app_id, "mcp.write", &project.id))
        .collect();
    if targets.len() == 1 {
        return Ok(targets.into_iter().next().expect("one mcp write target"));
    }
    if targets.is_empty() {
        return Err(
            "missing project_id; add manifest.permissions entry 'mcp.write:<project>' or 'mcp.write:*'"
                .into(),
        );
    }
    Err("missing project_id; multiple mcp.write targets are available".into())
}

fn normalize_mcp_server_config(value: serde_json::Value) -> Result<serde_json::Value, String> {
    match value {
        serde_json::Value::Object(object) if !object.is_empty() => {
            Ok(serde_json::Value::Object(object))
        }
        serde_json::Value::Object(_) => Err("config must be a non-empty JSON object".into()),
        _ => Err("config must be a JSON object".into()),
    }
}

fn parse_mcp_server_names(params: &serde_json::Value) -> Result<Vec<String>, String> {
    if let Some(value) = mcp_server_name_param(params) {
        return Ok(vec![normalize_mcp_server_name(&value)?]);
    }
    let value = params
        .get("names")
        .or_else(|| params.get("server_names"))
        .or_else(|| params.get("serverNames"))
        .ok_or("missing name or names")?;
    let raw = if let Some(value) = value.as_str() {
        vec![value.to_string()]
    } else {
        value
            .as_array()
            .ok_or("names must be a string or array of strings")?
            .iter()
            .filter_map(|item| item.as_str())
            .map(str::to_string)
            .collect()
    };
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for name in raw {
        let name = normalize_mcp_server_name(&name)?;
        if seen.insert(name.to_ascii_lowercase()) {
            out.push(name);
        }
    }
    if out.is_empty() {
        return Err("names must include at least one MCP server name".into());
    }
    Ok(out)
}

fn mcp_server_name_param(params: &serde_json::Value) -> Option<String> {
    string_param(params, "name", "name")
        .or_else(|| string_param(params, "server_name", "serverName"))
}

fn normalize_mcp_server_name(raw: &str) -> Result<String, String> {
    let name = raw.trim();
    if name.is_empty() {
        return Err("MCP server name must be non-empty".into());
    }
    if name.len() > 80 {
        return Err("MCP server name must be 80 characters or fewer".into());
    }
    if name
        .chars()
        .any(|ch| ch.is_control() || ch == ',' || ch == '/' || ch == '\\')
    {
        return Err("MCP server name must not contain commas, slashes, or control characters".into());
    }
    Ok(name.to_string())
}

fn sorted_mcp_server_names(value: &serde_json::Value) -> Vec<String> {
    let mut names: Vec<String> = value
        .as_object()
        .map(|object| object.keys().cloned().collect())
        .unwrap_or_default();
    names.sort();
    names
}

fn project_files_list_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let project = resolve_project_file_target(app, app_id, &params, "project.files.read", true)?;
    let rel_path = string_param(&params, "path", "path").unwrap_or_else(|| ".".into());
    let recursive = bool_param(&params, "recursive", "recursive").unwrap_or(false);
    let include_hidden = bool_param(&params, "include_hidden", "includeHidden").unwrap_or(false);
    let root = canonical_project_root(&project.root)?;
    let target = resolve_project_file_path(&root, &rel_path)?;
    let meta = std::fs::symlink_metadata(&target).map_err(|e| e.to_string())?;
    let mut entries = Vec::new();
    if meta.is_file() || meta.file_type().is_symlink() {
        if include_hidden || !project_path_is_hidden(&root, &target) {
            entries.push(project_file_entry(&root, &target, &meta)?);
        }
    } else {
        collect_project_file_entries(&root, &target, recursive, include_hidden, &mut entries)?;
    }
    Ok(serde_json::json!({
        "project_id": project.id,
        "project_name": project.name,
        "entries": entries,
    }))
}

const MAX_PROJECT_FILE_READ_BYTES: u64 = 1_048_576;
const PROJECT_FILE_SEARCH_DEFAULT_LIMIT: usize = 50;
const PROJECT_FILE_SEARCH_MAX_LIMIT: usize = 200;
const PROJECT_FILE_SEARCH_MAX_SCANNED: usize = 5_000;
const PROJECT_FILE_SEARCH_MAX_CONTENT_BYTES: u64 = 262_144;

fn project_files_read_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let project = resolve_project_file_target(app, app_id, &params, "project.files.read", true)?;
    let rel_path = required_string_param(&params, "path", "path")?;
    let root = canonical_project_root(&project.root)?;
    let target = resolve_project_file_path(&root, &rel_path)?;
    let meta = std::fs::metadata(&target).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("path is not a file".into());
    }
    if meta.len() > MAX_PROJECT_FILE_READ_BYTES {
        return Err("file is too large for project.files.read".into());
    }
    let bytes = std::fs::read(&target).map_err(|e| e.to_string())?;
    let content = String::from_utf8(bytes).map_err(|_| "file is not valid utf-8".to_string())?;
    let rel = project_rel_path(&root, &target)?;
    Ok(serde_json::json!({
        "project_id": project.id,
        "project_name": project.name,
        "path": rel,
        "size": meta.len(),
        "content": content,
    }))
}

fn project_files_search_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let project = resolve_project_file_target(app, app_id, &params, "project.files.read", true)?;
    let query = required_string_param(&params, "query", "query")?;
    let needle = query.to_lowercase();
    let rel_path = string_param(&params, "path", "path").unwrap_or_else(|| ".".into());
    let recursive = bool_param(&params, "recursive", "recursive").unwrap_or(true);
    let include_hidden = bool_param(&params, "include_hidden", "includeHidden").unwrap_or(false);
    let include_content = bool_param(&params, "include_content", "includeContent")
        .or_else(|| bool_param(&params, "content", "content"))
        .unwrap_or(false);
    let limit = bounded_usize_param(
        &params,
        "limit",
        "limit",
        PROJECT_FILE_SEARCH_DEFAULT_LIMIT,
        PROJECT_FILE_SEARCH_MAX_LIMIT,
    );
    let root = canonical_project_root(&project.root)?;
    let target = resolve_project_file_path(&root, &rel_path)?;
    let mut matches = Vec::new();
    let mut scanned = 0usize;
    let truncated = search_project_file_entries(
        &root,
        &target,
        &needle,
        recursive,
        include_hidden,
        include_content,
        limit,
        &mut scanned,
        &mut matches,
    )?;

    Ok(serde_json::json!({
        "project_id": project.id,
        "project_name": project.name,
        "query": query,
        "matches": matches,
        "scanned": scanned,
        "truncated": truncated,
    }))
}

fn project_files_write_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let project =
        resolve_project_file_target(app, app_id, &params, "project.files.write", false)?;
    let rel_path = required_string_param(&params, "path", "path")?;
    let content = params
        .get("content")
        .and_then(|v| v.as_str())
        .ok_or("missing content")?;
    let overwrite = bool_param(&params, "overwrite", "overwrite").unwrap_or(true);
    let create_dirs = bool_param(&params, "create_dirs", "createDirs").unwrap_or(true);
    let root = canonical_project_root(&project.root)?;
    let (target, rel) = resolve_project_mutation_path(&root, &rel_path, create_dirs)?;
    let existed = std::fs::symlink_metadata(&target).is_ok();
    if existed && !overwrite {
        return Err("file already exists".into());
    }
    if let Ok(meta) = std::fs::symlink_metadata(&target) {
        if meta.is_dir() && !meta.file_type().is_symlink() {
            return Err("path is a directory".into());
        }
    }
    std::fs::write(&target, content.as_bytes()).map_err(|e| e.to_string())?;
    let meta = std::fs::metadata(&target).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "ok": true,
        "project_id": project.id,
        "project_name": project.name,
        "path": rel,
        "created": !existed,
        "size": meta.len(),
    }))
}

fn project_files_mkdir_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let project =
        resolve_project_file_target(app, app_id, &params, "project.files.write", false)?;
    let rel_path = required_string_param(&params, "path", "path")?;
    let recursive = bool_param(&params, "recursive", "recursive").unwrap_or(true);
    let root = canonical_project_root(&project.root)?;
    let (target, rel) = resolve_project_mutation_path(&root, &rel_path, recursive)?;
    let existed = std::fs::symlink_metadata(&target).is_ok();
    if recursive {
        std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    } else {
        std::fs::create_dir(&target).map_err(|e| e.to_string())?;
    }
    let meta = std::fs::metadata(&target).map_err(|e| e.to_string())?;
    if !meta.is_dir() {
        return Err("path is not a directory".into());
    }
    Ok(serde_json::json!({
        "ok": true,
        "project_id": project.id,
        "project_name": project.name,
        "path": rel,
        "created": !existed,
    }))
}

fn project_files_move_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let project =
        resolve_project_file_target(app, app_id, &params, "project.files.write", false)?;
    let from_path = required_project_file_from(&params)?;
    let to_path = required_project_file_to(&params)?;
    let overwrite = bool_param(&params, "overwrite", "overwrite").unwrap_or(false);
    let create_dirs = bool_param(&params, "create_dirs", "createDirs").unwrap_or(true);
    let root = canonical_project_root(&project.root)?;
    let source = resolve_project_file_path(&root, &from_path)?;
    if source == root {
        return Err("refusing to move project root".into());
    }
    let source_meta = std::fs::symlink_metadata(&source).map_err(|e| e.to_string())?;
    let kind = project_file_kind(&source_meta);
    let from_rel = project_rel_path(&root, &source)?;
    let (target, to_rel) = resolve_project_mutation_path(&root, &to_path, create_dirs)?;
    ensure_project_transfer_target(&source, &target, &source_meta)?;
    prepare_project_move_target(&target, &source_meta, overwrite)?;
    std::fs::rename(&source, &target).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "ok": true,
        "project_id": project.id,
        "project_name": project.name,
        "from": from_rel,
        "to": to_rel,
        "kind": kind,
    }))
}

fn project_files_copy_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let project =
        resolve_project_file_target(app, app_id, &params, "project.files.write", false)?;
    let from_path = required_project_file_from(&params)?;
    let to_path = required_project_file_to(&params)?;
    let overwrite = bool_param(&params, "overwrite", "overwrite").unwrap_or(false);
    let recursive = bool_param(&params, "recursive", "recursive").unwrap_or(false);
    let create_dirs = bool_param(&params, "create_dirs", "createDirs").unwrap_or(true);
    let root = canonical_project_root(&project.root)?;
    let source = resolve_project_file_path(&root, &from_path)?;
    if source == root {
        return Err("refusing to copy project root".into());
    }
    let source_meta = std::fs::symlink_metadata(&source).map_err(|e| e.to_string())?;
    let kind = project_file_kind(&source_meta);
    if source_meta.is_dir() && !source_meta.file_type().is_symlink() && !recursive {
        return Err("copying a directory requires recursive=true".into());
    }
    let from_rel = project_rel_path(&root, &source)?;
    let (target, to_rel) = resolve_project_mutation_path(&root, &to_path, create_dirs)?;
    ensure_project_transfer_target(&source, &target, &source_meta)?;
    copy_project_path(&root, &source, &target, &source_meta, overwrite)?;
    Ok(serde_json::json!({
        "ok": true,
        "project_id": project.id,
        "project_name": project.name,
        "from": from_rel,
        "to": to_rel,
        "kind": kind,
    }))
}

fn project_files_delete_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let project =
        resolve_project_file_target(app, app_id, &params, "project.files.write", false)?;
    let rel_path = required_string_param(&params, "path", "path")?;
    let recursive = bool_param(&params, "recursive", "recursive").unwrap_or(false);
    let root = canonical_project_root(&project.root)?;
    let target = resolve_project_file_path(&root, &rel_path)?;
    if target == root {
        return Err("refusing to delete project root".into());
    }
    let meta = std::fs::symlink_metadata(&target).map_err(|e| e.to_string())?;
    let kind = project_file_kind(&meta);
    if meta.is_dir() && !meta.file_type().is_symlink() {
        if recursive {
            std::fs::remove_dir_all(&target).map_err(|e| e.to_string())?;
        } else {
            std::fs::remove_dir(&target).map_err(|e| e.to_string())?;
        }
    } else {
        std::fs::remove_file(&target).map_err(|e| e.to_string())?;
    }
    Ok(serde_json::json!({
        "ok": true,
        "project_id": project.id,
        "project_name": project.name,
        "path": project_rel_path(&root, &target)?,
        "kind": kind,
    }))
}

fn required_project_file_from(params: &serde_json::Value) -> Result<String, String> {
    string_param(params, "from", "from")
        .or_else(|| string_param(params, "from_path", "fromPath"))
        .or_else(|| string_param(params, "source", "source"))
        .or_else(|| string_param(params, "source_path", "sourcePath"))
        .ok_or_else(|| "missing from".to_string())
}

fn required_project_file_to(params: &serde_json::Value) -> Result<String, String> {
    string_param(params, "to", "to")
        .or_else(|| string_param(params, "to_path", "toPath"))
        .or_else(|| string_param(params, "target", "target"))
        .or_else(|| string_param(params, "target_path", "targetPath"))
        .ok_or_else(|| "missing to".to_string())
}

fn resolve_project_file_target(
    app: &AppHandle,
    app_id: &str,
    params: &serde_json::Value,
    scope: &str,
    linked_allowed: bool,
) -> Result<project::Project, String> {
    if let Some(project_id) = string_param(params, "project_id", "projectId") {
        let project = list_user_projects(app)?
            .into_iter()
            .find(|project| project.id == project_id)
            .ok_or_else(|| format!("project not found: {project_id}"))?;
        if linked_allowed {
            ensure_project_scope_access(app, app_id, scope, &project)?;
        } else {
            ensure_scoped_permission(app, app_id, scope, &project.id)?;
        }
        return Ok(project);
    }
    let targets: Vec<project::Project> = list_user_projects(app)?
        .into_iter()
        .filter(|project| {
            (linked_allowed && project_is_linked_to_app(app, app_id, project))
                || scoped_permission_allowed(app, app_id, scope, &project.id)
        })
        .collect();
    if targets.len() == 1 {
        return Ok(targets.into_iter().next().expect("one project file target"));
    }
    if targets.is_empty() {
        return Err(format!(
            "missing project_id; pass one from system.context().linked_projects or add manifest.permissions entry '{scope}:<project>'"
        ));
    }
    Err("missing project_id; multiple project file targets are available".into())
}

fn canonical_project_root(root: &str) -> Result<PathBuf, String> {
    Path::new(root)
        .canonicalize()
        .map_err(|e| format!("canonicalize project root: {e}"))
}

fn resolve_project_file_path(root: &Path, raw: &str) -> Result<PathBuf, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("canonicalize project root: {e}"))?;
    let raw = raw.trim();
    let candidate = if raw.is_empty() || raw == "." {
        root.clone()
    } else {
        let path = PathBuf::from(raw);
        if path.is_absolute() {
            path
        } else {
            root.join(path)
        }
    };
    let canonical = candidate
        .canonicalize()
        .map_err(|e| format!("canonicalize project file path: {e}"))?;
    if !canonical.starts_with(&root) {
        return Err("path must stay inside the selected project root".into());
    }
    let rel = canonical.strip_prefix(&root).unwrap_or(&canonical);
    if rel
        .components()
        .any(|component| matches!(component, Component::Normal(name) if name == ".reflex"))
    {
        return Err("project.files cannot read .reflex internals".into());
    }
    Ok(canonical)
}

fn resolve_project_mutation_path(
    root: &Path,
    raw: &str,
    create_parent_dirs: bool,
) -> Result<(PathBuf, String), String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("canonicalize project root: {e}"))?;
    let rel = normalize_project_mutation_rel_path(raw)?;
    let candidate = root.join(&rel);
    let parent = candidate
        .parent()
        .ok_or_else(|| "path must have a parent directory".to_string())?;
    let existing_parent = nearest_existing_ancestor(parent)?;
    let existing_parent_canon = existing_parent
        .canonicalize()
        .map_err(|e| format!("canonicalize project file parent: {e}"))?;
    if !existing_parent_canon.starts_with(&root) {
        return Err("path must stay inside the selected project root".into());
    }
    if create_parent_dirs {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let parent_canon = parent
        .canonicalize()
        .map_err(|e| format!("canonicalize project file parent: {e}"))?;
    if !parent_canon.starts_with(&root) {
        return Err("path must stay inside the selected project root".into());
    }
    if std::fs::symlink_metadata(&candidate).is_ok() {
        let canonical = candidate
            .canonicalize()
            .map_err(|e| format!("canonicalize project file path: {e}"))?;
        if !canonical.starts_with(&root) {
            return Err("path must stay inside the selected project root".into());
        }
        if project_path_is_blocked(&root, &canonical) {
            return Err("project.files cannot access .reflex internals".into());
        }
    }
    Ok((candidate, project_rel_string(&rel)))
}

fn normalize_project_mutation_rel_path(raw: &str) -> Result<PathBuf, String> {
    let trimmed = raw.trim().trim_start_matches('/');
    let mut out = PathBuf::new();
    for component in Path::new(trimmed).components() {
        match component {
            Component::Normal(name) if name == ".reflex" => {
                return Err("project.files cannot access .reflex internals".into());
            }
            Component::Normal(name) => out.push(name),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err("path must be relative and stay inside the selected project root".into());
            }
        }
    }
    if out.as_os_str().is_empty() {
        return Err("path is required".into());
    }
    Ok(out)
}

fn nearest_existing_ancestor(path: &Path) -> Result<PathBuf, String> {
    let mut current = path.to_path_buf();
    loop {
        if current.exists() {
            return Ok(current);
        }
        if !current.pop() {
            return Err("no existing parent directory".into());
        }
    }
}

fn prepare_project_move_target(
    target: &Path,
    source_meta: &std::fs::Metadata,
    overwrite: bool,
) -> Result<(), String> {
    let Ok(target_meta) = std::fs::symlink_metadata(target) else {
        return Ok(());
    };
    if !overwrite {
        return Err("target already exists".into());
    }
    if source_meta.is_dir() && !source_meta.file_type().is_symlink() {
        return Err("moving a directory over an existing path is not supported".into());
    }
    if target_meta.is_dir() && !target_meta.file_type().is_symlink() {
        return Err("target is a directory".into());
    }
    std::fs::remove_file(target).map_err(|e| e.to_string())
}

fn ensure_project_transfer_target(
    source: &Path,
    target: &Path,
    source_meta: &std::fs::Metadata,
) -> Result<(), String> {
    if source == target {
        return Err("source and target must differ".into());
    }
    if source_meta.is_dir() && !source_meta.file_type().is_symlink() && target.starts_with(source) {
        return Err("target cannot be inside the source directory".into());
    }
    Ok(())
}

fn copy_project_path(
    root: &Path,
    source: &Path,
    target: &Path,
    source_meta: &std::fs::Metadata,
    overwrite: bool,
) -> Result<(), String> {
    if source_meta.is_dir() && !source_meta.file_type().is_symlink() {
        if std::fs::symlink_metadata(target).is_ok() {
            return Err("target already exists".into());
        }
        copy_project_dir(root, source, target)?;
        return Ok(());
    }
    if let Ok(target_meta) = std::fs::symlink_metadata(target) {
        if !overwrite {
            return Err("target already exists".into());
        }
        if target_meta.is_dir() && !target_meta.file_type().is_symlink() {
            return Err("target is a directory".into());
        }
    }
    std::fs::copy(source, target)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn copy_project_dir(root: &Path, source: &Path, target: &Path) -> Result<(), String> {
    if project_path_is_blocked(root, source) || project_path_is_blocked(root, target) {
        return Err("project.files cannot access .reflex internals".into());
    }
    std::fs::create_dir(target).map_err(|e| e.to_string())?;
    let mut entries = std::fs::read_dir(source)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let child_source = entry.path();
        if project_path_is_blocked(root, &child_source) {
            continue;
        }
        let child_target = target.join(entry.file_name());
        let child_meta = std::fs::symlink_metadata(&child_source).map_err(|e| e.to_string())?;
        if child_meta.is_dir() && !child_meta.file_type().is_symlink() {
            copy_project_dir(root, &child_source, &child_target)?;
        } else {
            let child_canon = child_source
                .canonicalize()
                .map_err(|e| format!("canonicalize copied project file: {e}"))?;
            if !child_canon.starts_with(root) {
                return Err("path must stay inside the selected project root".into());
            }
            if project_path_is_blocked(root, &child_canon) {
                return Err("project.files cannot access .reflex internals".into());
            }
            std::fs::copy(&child_canon, &child_target)
                .map(|_| ())
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn search_project_file_entries(
    root: &Path,
    target: &Path,
    needle: &str,
    recursive: bool,
    include_hidden: bool,
    include_content: bool,
    limit: usize,
    scanned: &mut usize,
    out: &mut Vec<serde_json::Value>,
) -> Result<bool, String> {
    let meta = std::fs::symlink_metadata(target).map_err(|e| e.to_string())?;
    if meta.is_dir() && !meta.file_type().is_symlink() {
        let mut entries = std::fs::read_dir(target)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            if search_project_file_entry(
                root,
                &entry.path(),
                needle,
                recursive,
                include_hidden,
                include_content,
                limit,
                scanned,
                out,
            )? {
                return Ok(true);
            }
        }
        Ok(false)
    } else {
        search_project_file_entry(
            root,
            target,
            needle,
            recursive,
            include_hidden,
            include_content,
            limit,
            scanned,
            out,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn search_project_file_entry(
    root: &Path,
    path: &Path,
    needle: &str,
    recursive: bool,
    include_hidden: bool,
    include_content: bool,
    limit: usize,
    scanned: &mut usize,
    out: &mut Vec<serde_json::Value>,
) -> Result<bool, String> {
    if out.len() >= limit || *scanned >= PROJECT_FILE_SEARCH_MAX_SCANNED {
        return Ok(true);
    }
    if project_path_is_blocked(root, path) {
        return Ok(false);
    }
    if !include_hidden && project_path_is_hidden(root, path) {
        return Ok(false);
    }

    let meta = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    *scanned += 1;
    let rel = project_rel_path(root, path)?;
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("");
    let haystack = format!("{}\n{}", rel.to_lowercase(), name.to_lowercase());
    if haystack.contains(needle) {
        out.push(project_file_match(root, path, &meta, "path", None, None)?);
        if out.len() >= limit {
            return Ok(true);
        }
    }

    if include_content
        && meta.is_file()
        && meta.len() <= PROJECT_FILE_SEARCH_MAX_CONTENT_BYTES
        && search_project_file_content(root, path, &meta, needle, limit, out)?
    {
        return Ok(true);
    }

    if recursive && meta.is_dir() && !meta.file_type().is_symlink() {
        let mut entries = std::fs::read_dir(path)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            if search_project_file_entry(
                root,
                &entry.path(),
                needle,
                recursive,
                include_hidden,
                include_content,
                limit,
                scanned,
                out,
            )? {
                return Ok(true);
            }
        }
    }

    Ok(out.len() >= limit || *scanned >= PROJECT_FILE_SEARCH_MAX_SCANNED)
}

fn search_project_file_content(
    root: &Path,
    path: &Path,
    meta: &std::fs::Metadata,
    needle: &str,
    limit: usize,
    out: &mut Vec<serde_json::Value>,
) -> Result<bool, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let Ok(content) = String::from_utf8(bytes) else {
        return Ok(false);
    };
    for (idx, line) in content.lines().enumerate() {
        if line.to_lowercase().contains(needle) {
            let preview: String = line.trim().chars().take(300).collect();
            out.push(project_file_match(
                root,
                path,
                meta,
                "content",
                Some(idx + 1),
                Some(preview),
            )?);
            if out.len() >= limit {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn project_file_match(
    root: &Path,
    path: &Path,
    meta: &std::fs::Metadata,
    match_type: &str,
    line_number: Option<usize>,
    preview: Option<String>,
) -> Result<serde_json::Value, String> {
    let mut value = project_file_entry(root, path, meta)?;
    if let Some(object) = value.as_object_mut() {
        object.insert("match".into(), serde_json::Value::String(match_type.into()));
        if let Some(line_number) = line_number {
            object.insert(
                "line_number".into(),
                serde_json::Value::Number(serde_json::Number::from(line_number)),
            );
        }
        if let Some(preview) = preview {
            object.insert("preview".into(), serde_json::Value::String(preview));
        }
    }
    Ok(value)
}

fn collect_project_file_entries(
    root: &Path,
    dir: &Path,
    recursive: bool,
    include_hidden: bool,
    out: &mut Vec<serde_json::Value>,
) -> Result<(), String> {
    let mut entries = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        if project_path_is_blocked(root, &path) {
            continue;
        }
        if !include_hidden && project_path_is_hidden(root, &path) {
            continue;
        }
        let meta = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        out.push(project_file_entry(root, &path, &meta)?);
        if recursive && meta.is_dir() && !meta.file_type().is_symlink() {
            collect_project_file_entries(root, &path, recursive, include_hidden, out)?;
        }
    }
    Ok(())
}

fn project_file_entry(
    root: &Path,
    path: &Path,
    meta: &std::fs::Metadata,
) -> Result<serde_json::Value, String> {
    let rel = project_rel_path(root, path)?;
    let kind = project_file_kind(meta);
    let modified_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis());
    Ok(serde_json::json!({
        "path": rel,
        "name": path.file_name().and_then(|name| name.to_str()).unwrap_or(""),
        "kind": kind,
        "size": meta.is_file().then_some(meta.len()),
        "modified_ms": modified_ms,
        "is_hidden": project_path_is_hidden(root, path),
    }))
}

fn project_rel_path(root: &Path, path: &Path) -> Result<String, String> {
    let rel = path.strip_prefix(root).map_err(|e| e.to_string())?;
    Ok(project_rel_string(rel))
}

fn project_rel_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn project_file_kind(meta: &std::fs::Metadata) -> &'static str {
    if meta.file_type().is_symlink() {
        "symlink"
    } else if meta.is_dir() {
        "directory"
    } else {
        "file"
    }
}

fn project_path_is_blocked(root: &Path, path: &Path) -> bool {
    path.strip_prefix(root)
        .ok()
        .map(|rel| {
            rel.components()
                .any(|component| matches!(component, Component::Normal(name) if name == ".reflex"))
        })
        .unwrap_or(true)
}

fn project_path_is_hidden(root: &Path, path: &Path) -> bool {
    path.strip_prefix(root)
        .ok()
        .map(|rel| {
            rel.components().any(|component| {
                matches!(component, Component::Normal(name) if name.to_string_lossy().starts_with('.'))
            })
        })
        .unwrap_or(false)
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

async fn browser_close_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    ensure_browser_permission(app, app_id, "control")?;
    let tab_id = required_string_param(&params, "tab_id", "tabId")?;
    browser::browser_tab_close(app.clone(), tab_id).await
}

async fn browser_set_active_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    ensure_browser_permission(app, app_id, "control")?;
    let tab_id = required_string_param(&params, "tab_id", "tabId")?;
    browser::browser_set_active_tab(app.clone(), tab_id).await
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

async fn browser_back_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    ensure_browser_permission(app, app_id, "control")?;
    let tab_id = required_string_param(&params, "tab_id", "tabId")?;
    browser::browser_back(app.clone(), tab_id).await
}

async fn browser_forward_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    ensure_browser_permission(app, app_id, "control")?;
    let tab_id = required_string_param(&params, "tab_id", "tabId")?;
    browser::browser_forward(app.clone(), tab_id).await
}

async fn browser_reload_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    ensure_browser_permission(app, app_id, "control")?;
    let tab_id = required_string_param(&params, "tab_id", "tabId")?;
    browser::browser_reload(app.clone(), tab_id).await
}

async fn browser_current_url_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    ensure_browser_permission(app, app_id, "read")?;
    let tab_id = required_string_param(&params, "tab_id", "tabId")?;
    browser::browser_current_url(app.clone(), tab_id).await
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

async fn browser_scroll_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    ensure_browser_permission(app, app_id, "control")?;
    let tab_id = required_string_param(&params, "tab_id", "tabId")?;
    let dx = params.get("dx").and_then(|v| v.as_i64());
    let dy = params.get("dy").and_then(|v| v.as_i64());
    browser::browser_scroll(app.clone(), tab_id, dx, dy).await
}

async fn browser_wait_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    ensure_browser_permission(app, app_id, "read")?;
    let tab_id = required_string_param(&params, "tab_id", "tabId")?;
    let selector = required_string_param(&params, "selector", "selector")?;
    let timeout_ms = params
        .get("timeout_ms")
        .or_else(|| params.get("timeoutMs"))
        .and_then(|v| v.as_u64());
    browser::browser_wait_for(app.clone(), tab_id, selector, timeout_ms).await
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

fn scheduler_upsert_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let schedule = parse_schedule_def(params)?;
    let full_id = scheduler::make_full_id(app_id, &schedule.id);
    let mut manifest = apps::read_manifest(app, app_id).map_err(|e| e.to_string())?;
    let created = match manifest
        .schedules
        .iter()
        .position(|existing| existing.id == schedule.id)
    {
        Some(idx) => {
            manifest.schedules[idx] = schedule.clone();
            false
        }
        None => {
            manifest.schedules.push(schedule.clone());
            true
        }
    };
    apps::write_manifest(app, app_id, &manifest).map_err(|e| e.to_string())?;
    emit_scheduler_manifest_changed(app, &full_id, "upsert");
    Ok(serde_json::json!({
        "ok": true,
        "created": created,
        "schedule_id": full_id,
        "schedule": schedule,
    }))
}

async fn scheduler_delete_for_app(
    app: &AppHandle,
    app_id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let raw_id = required_string_param(&params, "schedule_id", "scheduleId")?;
    let local_id = own_local_schedule_id(app_id, &raw_id)?;
    let full_id = scheduler::make_full_id(app_id, &local_id);
    let mut manifest = apps::read_manifest(app, app_id).map_err(|e| e.to_string())?;
    let before = manifest.schedules.len();
    manifest
        .schedules
        .retain(|schedule| schedule.id != local_id);
    let deleted = manifest.schedules.len() != before;
    if deleted {
        apps::write_manifest(app, app_id, &manifest).map_err(|e| e.to_string())?;
        if let Some(handle) = app.try_state::<scheduler::SchedulerHandle>() {
            let _guard = handle.inner().inner.state_lock.lock().await;
            let mut state = scheduler::state::load_state(app).map_err(|e| e.to_string())?;
            state.schedules.remove(&full_id);
            scheduler::state::save_state(app, &state).map_err(|e| e.to_string())?;
        }
        emit_scheduler_manifest_changed(app, &full_id, "delete");
    }
    Ok(serde_json::json!({
        "ok": true,
        "deleted": deleted,
        "schedule_id": full_id,
    }))
}

fn parse_schedule_def(params: serde_json::Value) -> Result<apps::ScheduleDef, String> {
    let mut value = params
        .get("schedule")
        .cloned()
        .unwrap_or(params);
    let obj = value
        .as_object_mut()
        .ok_or_else(|| "schedule must be a JSON object".to_string())?;
    if !obj.contains_key("name") {
        if let Some(id) = obj.get("id").and_then(|v| v.as_str()) {
            obj.insert("name".into(), serde_json::Value::String(id.to_string()));
        }
    }
    if !obj.contains_key("catch_up") {
        if let Some(catch_up) = obj.get("catchUp").cloned() {
            obj.insert("catch_up".into(), catch_up);
        }
    }
    let mut schedule: apps::ScheduleDef =
        serde_json::from_value(value).map_err(|e| format!("invalid schedule: {e}"))?;
    schedule.id = schedule.id.trim().to_string();
    schedule.name = schedule.name.trim().to_string();
    schedule.cron = schedule.cron.trim().to_string();
    schedule.catch_up = schedule.catch_up.trim().to_string();
    validate_schedule_def(&schedule)?;
    Ok(schedule)
}

fn validate_schedule_def(schedule: &apps::ScheduleDef) -> Result<(), String> {
    validate_local_schedule_id(&schedule.id)?;
    if schedule.name.is_empty() {
        return Err("schedule.name is required".into());
    }
    schedule
        .cron
        .parse::<cron::Schedule>()
        .map_err(|e| format!("invalid schedule.cron: {e}"))?;
    if schedule.steps.is_empty() {
        return Err("schedule.steps must contain at least one step".into());
    }
    for step in &schedule.steps {
        if step.method.trim().is_empty() {
            return Err("schedule step method is required".into());
        }
        if scheduler::runner::is_method_blocked_in_unattended(&step.method) {
            return Err(format!(
                "schedule step method '{}' is not allowed in unattended workflows",
                step.method
            ));
        }
    }
    Ok(())
}

fn own_local_schedule_id(app_id: &str, raw_id: &str) -> Result<String, String> {
    let local_id = if let Some((target_app, local_id)) = scheduler::split_full_id(raw_id) {
        if target_app != app_id {
            return Err("scheduler.delete can only delete schedules owned by this app".into());
        }
        local_id.to_string()
    } else {
        raw_id.to_string()
    };
    validate_local_schedule_id(&local_id)?;
    Ok(local_id)
}

fn validate_local_schedule_id(id: &str) -> Result<(), String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("schedule.id is required".into());
    }
    if id.len() > 80 {
        return Err("schedule.id must be 80 characters or fewer".into());
    }
    if !id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err("schedule.id may contain only ASCII letters, numbers, '-', '_' or '.'".into());
    }
    Ok(())
}

fn emit_scheduler_manifest_changed(app: &AppHandle, schedule_id: &str, operation: &str) {
    let _ = app.emit("reflex://apps-changed", &serde_json::json!({}));
    if let Some(handle) = app.try_state::<scheduler::SchedulerHandle>() {
        handle.inner().rescan();
    }
    let _ = app.emit(
        "reflex://scheduler-state-changed",
        &serde_json::json!({
            "schedule_id": schedule_id,
            "operation": operation,
        }),
    );
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

fn scheduler_stats_for_app(
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

    let mut schedules = scheduler::commands::scheduler_list(app.clone())?;
    if let Some(target) = filter_app {
        schedules.retain(|item| item.app_id == target);
    }

    let recent_limit = bounded_usize_param(&params, "recent_limit", "recentLimit", 50, 500);
    let fetch_limit = if filter_app.is_some() {
        500
    } else {
        recent_limit
    };
    let mut runs = scheduler::commands::scheduler_runs(app.clone(), Some(fetch_limit), None)?;
    if let Some(target) = filter_app {
        runs.retain(|run| run.app_id == target);
    }
    runs.truncate(recent_limit);

    let stats = scheduler::commands::summarize_scheduler_stats(&schedules, &runs);
    serde_json::to_value(stats).map_err(|e| e.to_string())
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

fn bounded_usize_param(
    params: &serde_json::Value,
    snake: &str,
    camel: &str,
    default: usize,
    max: usize,
) -> usize {
    params
        .get(snake)
        .or_else(|| params.get(camel))
        .and_then(|v| v.as_u64())
        .map(|value| value.clamp(1, max as u64) as usize)
        .unwrap_or(default.min(max).max(1))
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
    fn connected_app_learn_prompt_is_grounded_in_visible_ui() {
        let outline = serde_json::json!({
            "outline": [
                { "tag": "button", "text": "Search" },
                { "tag": "a", "text": "Inbox" }
            ]
        });
        let prompt = build_connected_app_learn_prompt(
            "generic_web",
            "https://service.example/app",
            "Inbox\nSearch\nLatest item",
            &outline,
        );

        assert!(prompt.contains("Use only visible text and outline"));
        assert!(prompt.contains("Do not claim access to hidden data"));
        assert!(prompt.contains("PROVIDER:\ngeneric_web"));
        assert!(prompt.contains("SERVICE_URL:\nhttps://service.example/app"));
        assert!(prompt.contains("Latest item"));
        assert!(prompt.contains("\"Search\""));
    }

    #[test]
    fn connected_app_visible_helpers_accept_browser_shapes() {
        assert_eq!(
            browser_text_from_value(&serde_json::json!({ "text": "visible" })),
            "visible"
        );
        assert_eq!(
            outline_item_count(&serde_json::json!({
                "outline": [{ "text": "One" }, { "text": "Two" }]
            })),
            2
        );
        assert_eq!(
            outline_item_count(&serde_json::json!([{ "text": "One" }])),
            1
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

    #[test]
    fn memory_rel_path_rejects_escape_paths() {
        for raw in ["/tmp/note.md", "../note.md", "notes/../../secret.md"] {
            let params = serde_json::json!({ "relPath": raw });
            assert!(
                parse_memory_rel_path(&params).is_err(),
                "{raw} should not be accepted as a memory rel path"
            );
        }

        let params = serde_json::json!({ "relPath": "facts/project.md" });
        assert_eq!(
            parse_memory_rel_path(&params).unwrap(),
            PathBuf::from("facts/project.md")
        );
    }

    #[test]
    fn memory_paths_require_non_empty_string_array() {
        let params = serde_json::json!({ "paths": [" README.md ", "src/main.tsx"] });
        assert_eq!(
            parse_memory_paths(&params).unwrap(),
            vec!["README.md".to_string(), "src/main.tsx".to_string()]
        );

        for params in [
            serde_json::json!({}),
            serde_json::json!({ "paths": [] }),
            serde_json::json!({ "paths": [""] }),
            serde_json::json!({ "paths": ["ok.md", 42] }),
        ] {
            assert!(parse_memory_paths(&params).is_err());
        }
    }

    #[test]
    fn project_file_path_stays_inside_project_and_blocks_reflex() {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("reflex-project-files-{suffix}"));
        let outside = std::env::temp_dir().join(format!("reflex-project-files-outside-{suffix}.md"));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join(".reflex")).unwrap();
        std::fs::write(root.join("src/readme.md"), "ok").unwrap();
        std::fs::write(root.join("src/notes.md"), "Alpha target line\nsecond").unwrap();
        std::fs::write(root.join(".reflex/project.json"), "{}").unwrap();
        std::fs::write(&outside, "secret").unwrap();

        let canonical_root = root.canonicalize().unwrap();
        let resolved = resolve_project_file_path(&canonical_root, "src/readme.md").unwrap();
        assert_eq!(
            project_rel_path(&canonical_root, &resolved).unwrap(),
            "src/readme.md"
        );
        let (target, rel) =
            resolve_project_mutation_path(&canonical_root, "generated/report.md", true).unwrap();
        assert!(target.ends_with("generated/report.md"));
        assert_eq!(rel, "generated/report.md");
        assert!(target.parent().unwrap().exists());

        let source = canonical_root.join("src/notes.md");
        let copy_target = canonical_root.join("generated/copied.md");
        let source_meta = std::fs::symlink_metadata(&source).unwrap();
        copy_project_path(&canonical_root, &source, &copy_target, &source_meta, false).unwrap();
        assert_eq!(
            std::fs::read_to_string(&copy_target).unwrap(),
            "Alpha target line\nsecond"
        );
        assert!(copy_project_path(&canonical_root, &source, &copy_target, &source_meta, false)
            .unwrap_err()
            .contains("target already exists"));
        let move_target = canonical_root.join("generated/moved.md");
        let copy_meta = std::fs::symlink_metadata(&copy_target).unwrap();
        ensure_project_transfer_target(&copy_target, &move_target, &copy_meta).unwrap();
        prepare_project_move_target(&move_target, &copy_meta, false).unwrap();
        std::fs::rename(&copy_target, &move_target).unwrap();
        assert!(!copy_target.exists());
        assert!(move_target.exists());
        let dir_meta = std::fs::symlink_metadata(canonical_root.join("src")).unwrap();
        assert!(ensure_project_transfer_target(
            &canonical_root.join("src"),
            &canonical_root.join("src/nested"),
            &dir_meta,
        )
        .unwrap_err()
        .contains("inside"));

        let mut matches = Vec::new();
        let mut scanned = 0;
        let truncated = search_project_file_entries(
            &canonical_root,
            &canonical_root,
            "target",
            true,
            false,
            true,
            10,
            &mut scanned,
            &mut matches,
        )
        .unwrap();
        assert!(!truncated);
        assert!(scanned > 0);
        assert!(matches.iter().any(|entry| {
            entry.get("match").and_then(|v| v.as_str()) == Some("content")
                && entry.get("path").and_then(|v| v.as_str()) == Some("src/notes.md")
                && entry.get("line_number").and_then(|v| v.as_u64()) == Some(1)
        }));

        assert!(resolve_project_file_path(&canonical_root, ".reflex/project.json")
            .unwrap_err()
            .contains(".reflex"));
        assert!(resolve_project_mutation_path(&canonical_root, ".reflex/new.json", true)
            .unwrap_err()
            .contains(".reflex"));
        assert!(resolve_project_mutation_path(&canonical_root, "../outside.md", true)
            .unwrap_err()
            .contains("relative"));
        assert!(resolve_project_file_path(
            &canonical_root,
            &format!("../{}", outside.file_name().unwrap().to_string_lossy())
        )
        .unwrap_err()
        .contains("selected project root"));

        let _ = std::fs::remove_file(&outside);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn app_agent_prompt_adds_project_profile_by_default() {
        let project = project::Project {
            id: "p1".into(),
            name: "Project".into(),
            root: std::env::temp_dir()
                .join("reflex-app-agent-context-test")
                .to_string_lossy()
                .into_owned(),
            created_at_ms: 42,
            sandbox: "workspace-write".into(),
            mcp_servers: Some(serde_json::json!({ "browser": { "command": "browser-mcp" } })),
            description: Some("desc".into()),
            agent_instructions: Some("follow project rules".into()),
            skills: vec!["build-web-apps:react-best-practices".into()],
            apps: vec!["helper-app".into()],
        };
        let target = AgentCwdTarget {
            cwd: PathBuf::from(&project.root),
            mcp_servers: project.mcp_servers.clone(),
            project: Some(project),
        };

        let out =
            build_app_agent_prompt("caller", &serde_json::json!({}), "do work".into(), &target)
                .await;

        assert!(out.contains("## Reflex project profile"));
        assert!(out.contains("Preferred skills: build-web-apps:react-best-practices"));
        assert!(out.contains("MCP servers available in this project: browser"));
        assert!(out.contains("follow project rules"));
        assert!(out.contains("do work"));
    }

    #[tokio::test]
    async fn app_agent_prompt_can_skip_project_context() {
        let project = project::Project {
            id: "p1".into(),
            name: "Project".into(),
            root: "/tmp/project".into(),
            created_at_ms: 42,
            sandbox: "workspace-write".into(),
            mcp_servers: None,
            description: Some("desc".into()),
            agent_instructions: Some("private instructions".into()),
            skills: vec!["build-web-apps:react-best-practices".into()],
            apps: Vec::new(),
        };
        let target = AgentCwdTarget {
            cwd: PathBuf::from(&project.root),
            mcp_servers: None,
            project: Some(project),
        };

        let out = build_app_agent_prompt(
            "caller",
            &serde_json::json!({ "includeContext": false }),
            "raw prompt".into(),
            &target,
        )
        .await;

        assert_eq!(out, "raw prompt");
    }

    #[test]
    fn app_log_source_requires_namespace_boundary() {
        assert!(is_app_log_source("app:foo", "foo"));
        assert!(is_app_log_source("app:foo:widget", "foo"));
        assert!(!is_app_log_source("app:foo2", "foo"));
        assert!(!is_app_log_source("browser", "foo"));
    }

    #[test]
    fn schedule_validation_blocks_ui_and_recursive_methods() {
        let schedule = apps::ScheduleDef {
            id: "poll".into(),
            name: "Poll".into(),
            cron: "0 */5 * * * *".into(),
            enabled: true,
            catch_up: "once".into(),
            steps: vec![apps::Step {
                method: "scheduler.upsert".into(),
                params: serde_json::json!({}),
                save_as: None,
            }],
        };

        assert!(validate_schedule_def(&schedule)
            .unwrap_err()
            .contains("not allowed"));
    }

    #[test]
    fn schedule_id_rejects_full_or_escape_like_ids() {
        for id in ["app::daily", "../daily", "daily:bad", ""] {
            assert!(validate_local_schedule_id(id).is_err(), "{id} should fail");
        }
        assert_eq!(
            own_local_schedule_id("app", "app::daily").unwrap(),
            "daily".to_string()
        );
    }

    #[test]
    fn widget_entry_rejects_internal_or_escape_paths() {
        for entry in [
            "../widget.html",
            ".reflex/widget.html",
            ".hidden/widget.html",
            "manifest.json",
            "storage.json",
        ] {
            assert!(normalize_widget_entry(entry).is_err(), "{entry} should fail");
        }
        assert_eq!(
            normalize_widget_entry("/widgets/today.html").unwrap(),
            "widgets/today.html"
        );
    }

    #[test]
    fn widget_upsert_defaults_entry_and_name() {
        let (widget, html) = parse_widget_upsert(serde_json::json!({
            "id": "today",
            "html": "<html></html>"
        }))
        .unwrap();

        assert_eq!(widget.id, "today");
        assert_eq!(widget.name, "today");
        assert_eq!(widget.entry, "widgets/today.html");
        assert_eq!(widget.size, "small");
        assert_eq!(html.as_deref(), Some("<html></html>"));
    }

    #[test]
    fn action_upsert_defaults_name_and_accepts_params_schema_alias() {
        let action = parse_action_upsert(serde_json::json!({
            "id": "today",
            "public": true,
            "paramsSchema": {
                "type": "object",
                "properties": { "limit": { "type": "integer" } }
            },
            "steps": [
                { "method": "storage.get", "params": { "key": "today" } }
            ]
        }))
        .unwrap();

        assert_eq!(action.id, "today");
        assert_eq!(action.name, "today");
        assert!(action.public);
        assert!(action.params_schema.is_some());
        assert_eq!(action.steps.len(), 1);
    }

    #[test]
    fn action_validation_rejects_bad_id_and_empty_steps() {
        assert!(validate_action_id("../bad").is_err());
        let action = apps::ActionDef {
            id: "ok".into(),
            name: "OK".into(),
            description: None,
            params_schema: None,
            public: false,
            steps: Vec::new(),
        };
        assert!(validate_action_def(&action)
            .unwrap_err()
            .contains("steps"));
    }

    #[test]
    fn event_topics_are_trimmed_and_non_empty() {
        let params = serde_json::json!({ "topics": [" alpha ", "", "beta"] });
        assert_eq!(
            parse_topics(&params).unwrap(),
            vec!["alpha".to_string(), "beta".to_string()]
        );

        assert!(parse_topics(&serde_json::json!({ "topics": ["", "  "] })).is_err());
    }

    #[test]
    fn permission_and_host_normalizers_accept_targeted_manifest_grants() {
        assert_eq!(
            normalize_permission(" apps.invoke:health::today ").unwrap(),
            "apps.invoke:health::today"
        );
        assert!(normalize_permission("bad permission").is_err());
        assert_eq!(
            normalize_allowed_host("https://API.Example.com/v1").unwrap(),
            "api.example.com"
        );
        assert_eq!(
            normalize_allowed_host("*.Example.com").unwrap(),
            "*.example.com"
        );
        assert!(normalize_allowed_host("*.127.0.0.1").is_err());
    }

    #[test]
    fn project_skill_names_are_normalized_and_deduped() {
        let params = serde_json::json!({
            "skills": [
                " build-web-apps:react-best-practices ",
                "BUILD-WEB-APPS:REACT-BEST-PRACTICES",
                "custom-workflow"
            ]
        });
        assert_eq!(
            parse_project_skill_names(&params).unwrap(),
            vec![
                "build-web-apps:react-best-practices".to_string(),
                "custom-workflow".to_string()
            ]
        );
        assert!(normalize_project_skill_name("bad,skill").is_err());
    }

    #[test]
    fn mcp_server_names_are_normalized_and_deduped() {
        let params = serde_json::json!({
            "serverNames": [" reflex_browser ", "REFLEX_BROWSER", "linear"]
        });
        assert_eq!(
            parse_mcp_server_names(&params).unwrap(),
            vec!["reflex_browser".to_string(), "linear".to_string()]
        );
        assert!(normalize_mcp_server_name("bad/server").is_err());
        assert!(normalize_mcp_server_config(serde_json::json!({})).is_err());
        assert!(normalize_mcp_server_config(serde_json::json!({
            "command": "node",
            "args": ["bridge.js"]
        }))
        .is_ok());
    }

    #[test]
    fn project_profile_text_is_trimmed_bounded_and_nullable() {
        assert_eq!(
            normalize_optional_project_text(
                &serde_json::Value::String("  useful context  ".into()),
                "description",
                100,
            )
            .unwrap(),
            Some("useful context".into())
        );
        assert_eq!(
            normalize_optional_project_text(&serde_json::Value::Null, "description", 100)
                .unwrap(),
            None
        );
        assert!(normalize_optional_project_text(
            &serde_json::Value::String("too long".into()),
            "description",
            3,
        )
        .is_err());
    }

    #[test]
    fn project_sandbox_accepts_only_known_modes() {
        assert_eq!(
            normalize_project_sandbox(" workspace-write ").unwrap(),
            "workspace-write"
        );
        assert!(normalize_project_sandbox("read-only").is_ok());
        assert!(normalize_project_sandbox("danger-full-access").is_ok());
        assert!(normalize_project_sandbox("full-access").is_err());
    }

    #[test]
    fn project_app_target_param_accepts_snake_and_camel() {
        assert_eq!(
            project_app_target_param(&serde_json::json!({ "app_id": "notes" })).unwrap(),
            "notes"
        );
        assert_eq!(
            project_app_target_param(&serde_json::json!({ "appId": "calendar" })).unwrap(),
            "calendar"
        );
        assert!(project_app_target_param(&serde_json::json!({})).is_none());
    }
}
