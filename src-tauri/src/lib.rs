mod app_bus;
mod app_runtime;
mod app_server;
mod app_watcher;
mod apps;
mod apps_dispatch;
mod browser;
mod bus_log;
mod codex;
mod context;
mod logs;
mod memory;
mod project;
mod project_watcher;
mod scheduler;
mod storage;
mod suggester;

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};

const QUICK_WINDOW: &str = "quick";
const MAIN_WINDOW: &str = "main";
const QUICK_OPEN_EVENT: &str = "reflex://quick-open";
const THREAD_CREATED_EVENT: &str = "reflex://thread-created";

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct QuickContext {
    pub frontmost_app: Option<String>,
    pub finder_target: Option<String>,
}

#[derive(Clone, Serialize)]
struct QuickOpenPayload {
    ctx: QuickContext,
    project: Option<project::Project>,
    candidate_root: Option<String>,
    nearest: Vec<project::Project>,
}

#[derive(Clone, Serialize)]
struct ThreadCreated {
    id: String,
    project_id: String,
    project_name: String,
    prompt: String,
    cwd: String,
    ctx: QuickContext,
    created_at_ms: u128,
    #[serde(default)]
    goal: Option<String>,
    #[serde(default)]
    plan_mode: bool,
    #[serde(default)]
    source: String,
    #[serde(default)]
    browser_tabs: Vec<storage::BrowserTab>,
}

#[derive(Serialize, Clone)]
struct ProjectThread {
    project: project::Project,
    thread: storage::StoredThread,
}

#[tauri::command]
async fn capture_context(app: AppHandle) -> QuickContext {
    context::capture(&app).await
}

fn ui_state_path(app: &AppHandle) -> Result<PathBuf, String> {
    let base = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir: {e}"))?;
    std::fs::create_dir_all(&base).map_err(|e| format!("mkdir app_data_dir: {e}"))?;
    Ok(base.join("ui-state.json"))
}

fn read_ui_state(app: &AppHandle) -> serde_json::Value {
    let path = match ui_state_path(app) {
        Ok(p) => p,
        Err(_) => return serde_json::json!({}),
    };
    if !path.exists() {
        return serde_json::json!({});
    }
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_else(|_| serde_json::json!({})),
        Err(_) => serde_json::json!({}),
    }
}

fn write_ui_state(app: &AppHandle, state: &serde_json::Value) -> Result<(), String> {
    let path = ui_state_path(app)?;
    let s = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    std::fs::write(path, s).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_active_project(app: AppHandle) -> Result<Option<String>, String> {
    let state = read_ui_state(&app);
    let id = state
        .get("active_project_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    if let Some(ref pid) = id {
        if project::get_by_id(&app, pid).ok().flatten().is_none() {
            return Ok(None);
        }
    }
    Ok(id)
}

#[tauri::command]
fn set_active_project(app: AppHandle, project_id: Option<String>) -> Result<(), String> {
    let mut state = read_ui_state(&app);
    let map = state
        .as_object_mut()
        .ok_or_else(|| "ui-state.json is not an object".to_string())?;
    match project_id {
        Some(id) if !id.trim().is_empty() => {
            if project::get_by_id(&app, &id)
                .map_err(|e| e.to_string())?
                .is_none()
            {
                return Err(format!("project not found: {id}"));
            }
            map.insert(
                "active_project_id".into(),
                serde_json::Value::String(id),
            );
        }
        _ => {
            map.remove("active_project_id");
        }
    }
    write_ui_state(&app, &state)
}

#[tauri::command]
fn list_projects(app: AppHandle) -> Result<Vec<project::Project>, String> {
    let apps_root = apps::apps_dir(&app)
        .ok()
        .and_then(|p| p.canonicalize().ok());
    let all = project::list_registered(&app).map_err(|e| e.to_string())?;
    Ok(all
        .into_iter()
        .filter(|p| {
            if let Some(apps_root) = &apps_root {
                if let Ok(c) = std::path::PathBuf::from(&p.root).canonicalize() {
                    return !c.starts_with(apps_root);
                }
            }
            true
        })
        .collect())
}

#[tauri::command]
fn list_apps(app: AppHandle) -> Result<Vec<apps::AppListing>, String> {
    apps::list_apps(&app).map_err(|e| e.to_string())
}

#[tauri::command]
fn read_app_html(app: AppHandle, app_id: String) -> Result<String, String> {
    apps::read_app_html(&app, &app_id).map_err(|e| e.to_string())
}

#[tauri::command]
async fn app_invoke(
    app: AppHandle,
    app_id: String,
    method: String,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    apps_dispatch::dispatch_app_method(&app, &app_id, &method, params).await
}

#[tauri::command]
fn app_status(app: AppHandle, app_id: String) -> Result<serde_json::Value, String> {
    let dir = apps::app_dir(&app, &app_id).map_err(|e| e.to_string())?;
    apps::git_init_if_needed(&dir).map_err(|e| e.to_string())?;
    let manifest = apps::read_manifest(&app, &app_id).ok();
    let entry_exists = manifest
        .as_ref()
        .map(|m| dir.join(&m.entry).exists())
        .unwrap_or(false);
    let mut status = apps::git_status(&dir).map_err(|e| e.to_string())?;
    if status.revision == 0 && entry_exists {
        let _ = apps::git_commit_all(&dir, "initial");
        status = apps::git_status(&dir).map_err(|e| e.to_string())?;
    }
    Ok(serde_json::json!({
        "has_changes": status.has_changes,
        "revision": status.revision,
        "last_commit_message": status.last_commit_message,
        "entry_exists": entry_exists,
    }))
}

#[tauri::command]
fn app_save(
    app: AppHandle,
    app_id: String,
    message: Option<String>,
) -> Result<(), String> {
    let dir = apps::app_dir(&app, &app_id).map_err(|e| e.to_string())?;
    let msg = message
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "revision".to_string());
    apps::git_commit_all(&dir, &msg).map_err(|e| e.to_string())
}

#[tauri::command]
fn app_revert(app: AppHandle, app_id: String) -> Result<(), String> {
    let dir = apps::app_dir(&app, &app_id).map_err(|e| e.to_string())?;
    apps::git_revert_all(&dir).map_err(|e| e.to_string())
}

#[tauri::command]
fn app_diff(app: AppHandle, app_id: String) -> Result<String, String> {
    let dir = apps::app_dir(&app, &app_id).map_err(|e| e.to_string())?;
    apps::git_diff(&dir).map_err(|e| e.to_string())
}

#[tauri::command]
fn app_save_partial(
    app: AppHandle,
    app_id: String,
    patch: String,
    message: Option<String>,
) -> Result<(), String> {
    let dir = apps::app_dir(&app, &app_id).map_err(|e| e.to_string())?;
    let msg = message
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "partial revision".to_string());
    apps::git_apply_partial(&dir, &patch, &msg).map_err(|e| e.to_string())
}

#[tauri::command]
async fn app_server_start(app: AppHandle, app_id: String) -> Result<u16, String> {
    let runtimes = app.state::<app_runtime::AppRuntimes>();
    app_runtime::start(&runtimes, &app, &app_id).await
}

#[tauri::command]
async fn app_server_stop(app: AppHandle, app_id: String) -> Result<(), String> {
    let runtimes = app.state::<app_runtime::AppRuntimes>();
    app_runtime::stop(&runtimes, &app_id).await;
    Ok(())
}

#[tauri::command]
async fn app_server_restart(app: AppHandle, app_id: String) -> Result<u16, String> {
    let runtimes = app.state::<app_runtime::AppRuntimes>();
    app_runtime::restart(&runtimes, &app, &app_id).await
}

#[tauri::command]
async fn app_server_status(
    app: AppHandle,
    app_id: String,
) -> Result<app_runtime::ServerStatus, String> {
    let runtimes = app.state::<app_runtime::AppRuntimes>();
    Ok(app_runtime::status(&runtimes, &app_id).await)
}

#[tauri::command]
async fn app_server_logs(
    app: AppHandle,
    app_id: String,
) -> Result<app_runtime::LogsSnapshot, String> {
    let runtimes = app.state::<app_runtime::AppRuntimes>();
    Ok(app_runtime::logs(&runtimes, &app_id).await)
}

#[tauri::command]
fn app_watch_start(app: AppHandle, app_id: String) -> Result<(), String> {
    let watchers = app.state::<app_watcher::AppWatchers>();
    app_watcher::start(&watchers, &app, &app_id)
}

#[tauri::command]
fn app_watch_stop(app: AppHandle, app_id: String) -> Result<(), String> {
    let watchers = app.state::<app_watcher::AppWatchers>();
    app_watcher::stop(&watchers, &app_id);
    Ok(())
}

#[tauri::command]
fn project_watch_start(app: AppHandle, project_id: String) -> Result<(), String> {
    let watchers = app.state::<project_watcher::ProjectWatchers>();
    project_watcher::start(&watchers, &app, &project_id)
}

#[tauri::command]
fn project_watch_stop(app: AppHandle, project_id: String) -> Result<(), String> {
    let watchers = app.state::<project_watcher::ProjectWatchers>();
    project_watcher::stop(&watchers, &project_id);
    Ok(())
}


#[tauri::command]
fn app_export(app: AppHandle, app_id: String, target_path: String) -> Result<(), String> {
    apps::export_app(&app, &app_id, std::path::Path::new(&target_path))
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_app(app: AppHandle, app_id: String) -> Result<apps::TrashEntry, String> {
    let dir = apps::app_dir(&app, &app_id).map_err(|e| e.to_string())?;
    if !dir.exists() {
        return Err(format!("app not found: {app_id}"));
    }
    if let Ok(threads) = storage::read_all_threads(&dir) {
        for t in threads.iter().filter(|t| !t.meta.done) {
            if let Err(e) = storage::finalize_thread(&dir, &t.meta.id, Some(-3), None) {
                eprintln!("[delete_app] finalize_thread({}) failed: {e}", t.meta.id);
            }
        }
    }
    let entry = apps::move_to_trash(&app, &app_id).map_err(|e| e.to_string())?;
    if let Err(e) = project::deregister_by_root(&app, &dir) {
        eprintln!("[delete_app] deregister_by_root failed: {e}");
    }
    let _ = app.emit("reflex://apps-changed", &serde_json::json!({}));
    Ok(entry)
}

#[tauri::command]
fn list_trashed_apps(app: AppHandle) -> Result<Vec<apps::TrashEntry>, String> {
    apps::list_trash(&app).map_err(|e| e.to_string())
}

#[tauri::command]
fn restore_app(app: AppHandle, trash_id: String) -> Result<String, String> {
    let new_id = apps::restore_from_trash(&app, &trash_id).map_err(|e| e.to_string())?;
    if let Ok(dir) = apps::app_dir(&app, &new_id) {
        if let Ok(proj) = project::read_project_at(&dir) {
            if let Err(e) = project::register(&app, &proj) {
                eprintln!("[restore_app] register failed: {e}");
            }
        }
    }
    let _ = app.emit("reflex://apps-changed", &serde_json::json!({}));
    Ok(new_id)
}

#[tauri::command]
fn purge_trashed_app(app: AppHandle, trash_id: String) -> Result<(), String> {
    apps::purge_trashed(&app, &trash_id).map_err(|e| e.to_string())?;
    let _ = app.emit("reflex://apps-changed", &serde_json::json!({}));
    Ok(())
}

#[tauri::command]
fn app_import(app: AppHandle, zip_path: String) -> Result<apps::AppManifest, String> {
    apps::import_app(&app, std::path::Path::new(&zip_path)).map_err(|e| e.to_string())
}

#[tauri::command]
fn read_app_manifest(app: AppHandle, app_id: String) -> Result<apps::AppManifest, String> {
    apps::read_manifest(&app, &app_id).map_err(|e| e.to_string())
}

struct ConnectedAppSpec {
    app_id: String,
    provider: String,
    name: String,
    icon: String,
    description: String,
    url: String,
    open_url: String,
    capabilities: Vec<String>,
    data_model: serde_json::Value,
    auth: serde_json::Value,
    mcp: serde_json::Value,
    notes: String,
}

#[tauri::command]
fn install_connected_app(
    app: AppHandle,
    provider: String,
    url: Option<String>,
    display_name: Option<String>,
    project_id: Option<String>,
) -> Result<apps::AppManifest, String> {
    let spec = connected_app_spec(provider, url, display_name)?;
    let dir = apps::app_dir(&app, &spec.app_id).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let created_at_ms = apps::read_manifest(&app, &spec.app_id)
        .map(|manifest| manifest.created_at_ms)
        .unwrap_or_else(|_| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        });

    let storage_key = format!("connected.{}.latestVisibleSession", spec.provider);
    let learned_key = format!("connected.{}.learnedInterface", spec.provider);
    let mut learned_capabilities = spec.capabilities.clone();
    if !learned_capabilities
        .iter()
        .any(|capability| capability == "interface.visible_session.learn")
    {
        learned_capabilities.push("interface.visible_session.learn".into());
    }
    let mut manifest = apps::AppManifest {
        id: spec.app_id.clone(),
        name: spec.name.clone(),
        icon: Some(spec.icon.clone()),
        description: Some(spec.description.clone()),
        entry: "index.html".into(),
        permissions: vec!["browser.control".into(), "browser.read".into()],
        kind: "panel".into(),
        created_at_ms,
        runtime: Some("static".into()),
        server: None,
        external: Some(apps::ExternalConfig {
            url: spec.url.clone(),
            title: Some(spec.name.clone()),
            open_url: Some(spec.open_url.clone()),
        }),
        integration: Some(apps::IntegrationConfig {
            provider: spec.provider.clone(),
            display_name: Some(spec.name.clone()),
            capabilities: spec.capabilities.clone(),
            data_model: spec.data_model.clone(),
            auth: spec.auth.clone(),
            mcp: spec.mcp.clone(),
            notes: Some(spec.notes.clone()),
        }),
        network: None,
        schedules: Vec::new(),
        actions: vec![
            apps::ActionDef {
                id: "summarize_visible_session".into(),
                name: "Summarize visible session".into(),
                description: Some(
                    "Open the connected service in the Browser bridge and summarize only visible text."
                        .into(),
                ),
                params_schema: None,
                public: true,
                steps: vec![
                    apps::Step {
                        method: "browser.init".into(),
                        params: serde_json::json!({ "headless": true }),
                        save_as: Some("browser".into()),
                    },
                    apps::Step {
                        method: "browser.open".into(),
                        params: serde_json::json!({ "url": spec.url.clone() }),
                        save_as: Some("opened".into()),
                    },
                    apps::Step {
                        method: "browser.waitFor".into(),
                        params: serde_json::json!({
                            "tabId": "{{steps.opened.tab_id}}",
                            "selector": "body",
                            "timeoutMs": 15000
                        }),
                        save_as: Some("page_ready".into()),
                    },
                    apps::Step {
                        method: "browser.readText".into(),
                        params: serde_json::json!({ "tabId": "{{steps.opened.tab_id}}" }),
                        save_as: Some("visible_text".into()),
                    },
                    apps::Step {
                        method: "agent.task".into(),
                        params: serde_json::json!({
                            "includeContext": false,
                            "prompt": "Summarize the visible web-session text below. Use only content present in the text. If it appears to be a chat app, extract visible chats/messages and names when present. Do not claim access to hidden messages or private data outside this visible session. Return concise JSON with summary, visible_items, and warnings.\n\nVISIBLE_TEXT:\n{{steps.visible_text}}"
                        }),
                        save_as: Some("summary".into()),
                    },
                ],
            },
            apps::ActionDef {
                id: "read_visible_session".into(),
                name: "Read visible session".into(),
                description: Some(
                    "Explicit raw read of the connected service's currently visible browser text."
                        .into(),
                ),
                params_schema: None,
                public: false,
                steps: vec![
                    apps::Step {
                        method: "browser.init".into(),
                        params: serde_json::json!({ "headless": true }),
                        save_as: Some("browser".into()),
                    },
                    apps::Step {
                        method: "browser.open".into(),
                        params: serde_json::json!({ "url": spec.url.clone() }),
                        save_as: Some("opened".into()),
                    },
                    apps::Step {
                        method: "browser.waitFor".into(),
                        params: serde_json::json!({
                            "tabId": "{{steps.opened.tab_id}}",
                            "selector": "body",
                            "timeoutMs": 15000
                        }),
                        save_as: Some("page_ready".into()),
                    },
                    apps::Step {
                        method: "browser.readText".into(),
                        params: serde_json::json!({ "tabId": "{{steps.opened.tab_id}}" }),
                        save_as: Some("visible_text".into()),
                    },
                ],
            },
            apps::ActionDef {
                id: "latest_visible_session".into(),
                name: "Latest visible session".into(),
                description: Some("Return the latest snapshot explicitly saved from the panel.".into()),
                params_schema: None,
                public: true,
                steps: vec![apps::Step {
                    method: "storage.get".into(),
                    params: serde_json::json!({ "key": storage_key }),
                    save_as: Some("snapshot".into()),
                }],
            },
            apps::ActionDef {
                id: "learn_visible_interface".into(),
                name: "Learn visible interface".into(),
                description: Some(
                    "Inspect visible page text/outline and save a learned adapter profile.".into(),
                ),
                params_schema: None,
                public: true,
                steps: vec![
                    apps::Step {
                        method: "browser.init".into(),
                        params: serde_json::json!({ "headless": true }),
                        save_as: Some("browser".into()),
                    },
                    apps::Step {
                        method: "browser.open".into(),
                        params: serde_json::json!({ "url": spec.url.clone() }),
                        save_as: Some("opened".into()),
                    },
                    apps::Step {
                        method: "browser.waitFor".into(),
                        params: serde_json::json!({
                            "tabId": "{{steps.opened.tab_id}}",
                            "selector": "body",
                            "timeoutMs": 15000
                        }),
                        save_as: Some("page_ready".into()),
                    },
                    apps::Step {
                        method: "browser.readText".into(),
                        params: serde_json::json!({ "tabId": "{{steps.opened.tab_id}}" }),
                        save_as: Some("visible_text".into()),
                    },
                    apps::Step {
                        method: "browser.readOutline".into(),
                        params: serde_json::json!({ "tabId": "{{steps.opened.tab_id}}" }),
                        save_as: Some("outline".into()),
                    },
                    apps::Step {
                        method: "agent.task".into(),
                        params: serde_json::json!({
                            "includeContext": false,
                            "prompt": "Build a connected-app adapter profile from the visible web UI below. Use only visible text and outline. Infer data entities, user actions, likely selectors or text anchors, safe automation workflows, and MCP opportunities. Do not claim access to hidden data. Return concise JSON with provider, entities, actions, workflows, selectors, data_access, mcp_bridge, risks, and next_steps.\n\nVISIBLE_TEXT:\n{{steps.visible_text}}\n\nOUTLINE:\n{{steps.outline}}"
                        }),
                        save_as: Some("learned".into()),
                    },
                    apps::Step {
                        method: "storage.set".into(),
                        params: serde_json::json!({
                            "key": learned_key,
                            "value": {
                                "provider": spec.provider.clone(),
                                "service_url": spec.url.clone(),
                                "profile": "{{steps.learned.result}}"
                            }
                        }),
                        save_as: Some("stored".into()),
                    },
                    apps::Step {
                        method: "integration.update".into(),
                        params: serde_json::json!({
                            "integration": {
                                "capabilities": learned_capabilities,
                                "data_model": {
                                    "learned_profile": "{{steps.learned.result}}"
                                }
                            }
                        }),
                        save_as: Some("integration".into()),
                    },
                ],
            },
        ],
        widgets: Vec::new(),
    };

    apps::write_manifest(&app, &spec.app_id, &manifest).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("index.html"), connected_app_index_html(&spec)?)
        .map_err(|e| e.to_string())?;
    let app_project = ensure_app_project(&app, &spec.app_id)?;
    let mcp_read = format!("mcp.read:{}", app_project.id);
    let mcp_write = format!("mcp.write:{}", app_project.id);
    if !manifest.permissions.iter().any(|p| p == &mcp_read) {
        manifest.permissions.push(mcp_read);
    }
    if !manifest.permissions.iter().any(|p| p == &mcp_write) {
        manifest.permissions.push(mcp_write);
    }
    apps::write_manifest(&app, &spec.app_id, &manifest).map_err(|e| e.to_string())?;

    if let Some(project_id) = project_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        link_app_to_project_inner(&app, project_id, &spec.app_id)?;
    }

    let _ = app.emit("reflex://apps-changed", &serde_json::json!({}));
    Ok(manifest)
}

fn ensure_app_project(app: &AppHandle, app_id: &str) -> Result<project::Project, String> {
    let dir = apps::app_dir(app, app_id).map_err(|e| e.to_string())?;
    if let Some(p) = project::find_project_for(&dir) {
        return Ok(p);
    }
    let manifest = apps::read_manifest(app, app_id).ok();
    let proj_name = manifest
        .as_ref()
        .map(|m| format!("App · {}", m.name))
        .unwrap_or_else(|| format!("App · {app_id}"));
    project::create_project(app, &dir, Some(proj_name), None).map_err(|e| e.to_string())
}

fn connected_app_spec(
    provider: String,
    url: Option<String>,
    display_name: Option<String>,
) -> Result<ConnectedAppSpec, String> {
    let provider = provider.trim().to_ascii_lowercase();
    if provider.is_empty() {
        return Err("provider is required".into());
    }
    if provider == "telegram" {
        return Ok(ConnectedAppSpec {
            app_id: "connected_telegram".into(),
            provider,
            name: display_name
                .and_then(non_empty_string)
                .unwrap_or_else(|| "Telegram".into()),
            icon: "TG".into(),
            description:
                "Telegram Web visible-session adapter with Browser bridge actions and MCP plan."
                    .into(),
            url: url
                .and_then(non_empty_string)
                .unwrap_or_else(|| "https://web.telegram.org/a/".into()),
            open_url: "https://web.telegram.org/a/".into(),
            capabilities: vec![
                "messages.visible_session.read".into(),
                "messages.visible_session.summarize".into(),
                "chats.visible_session.list".into(),
                "mcp.telegram.optional".into(),
            ],
            data_model: serde_json::json!({
                "entities": ["visible_session", "visible_chat", "visible_message", "summary"],
                "read_modes": ["browser.visible_session", "mcp.telegram"],
                "storage_policy": {
                    "default": "derived summaries",
                    "raw_text": "explicit user action only"
                }
            }),
            auth: serde_json::json!({
                "type": "user_visible_session",
                "browser_login_required": true,
                "credential_storage": "Reflex does not store Telegram credentials in the app manifest."
            }),
            mcp: serde_json::json!({
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
                "notes": "Personal chat access requires a user-approved Telegram client session such as MTProto/TDLib/MCP. Bot API tokens cannot read arbitrary personal chats."
            }),
            notes:
                "Reads only what the user opens in the visible Telegram Web session unless a user-approved Telegram MCP bridge is configured."
                    .into(),
        });
    }

    let url = url
        .and_then(non_empty_string)
        .ok_or_else(|| "url is required for generic connected apps".to_string())?;
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("url must start with http:// or https://".into());
    }
    let name = display_name
        .and_then(non_empty_string)
        .unwrap_or_else(|| "Connected app".into());
    let app_id = connected_app_id(&provider, &name, &url);
    Ok(ConnectedAppSpec {
        app_id,
        provider,
        name,
        icon: "APP".into(),
        description: "Generic visible-session adapter with Browser bridge actions.".into(),
        open_url: url.clone(),
        url,
        capabilities: vec![
            "visible_session.read".into(),
            "visible_session.summarize".into(),
            "mcp.optional".into(),
        ],
        data_model: serde_json::json!({
            "entities": ["visible_session", "visible_item", "summary"],
            "read_modes": ["browser.visible_session", "mcp.optional"],
            "storage_policy": {
                "default": "derived summaries",
                "raw_text": "explicit user action only"
            }
        }),
        auth: serde_json::json!({
            "type": "user_visible_session",
            "browser_login_required": true
        }),
        mcp: serde_json::json!({
            "recommended": false,
            "notes": "Add a provider-specific MCP server when durable authenticated data access is required."
        }),
        notes: "Reads only visible browser-session content unless a provider MCP bridge is configured."
            .into(),
    })
}

fn non_empty_string(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn sanitize_app_id(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut last_was_sep = false;
    for ch in value.chars() {
        let next = if ch.is_ascii_alphanumeric() {
            last_was_sep = false;
            Some(ch.to_ascii_lowercase())
        } else if !last_was_sep {
            last_was_sep = true;
            Some('_')
        } else {
            None
        };
        if let Some(ch) = next {
            out.push(ch);
        }
    }
    let trimmed = out.trim_matches('_');
    if trimmed.is_empty() {
        "app".into()
    } else {
        trimmed.chars().take(48).collect()
    }
}

fn connected_app_id(provider: &str, name: &str, url: &str) -> String {
    let provider_slug = sanitize_app_id(provider);
    let name_slug = sanitize_app_id(name);
    let host_slug = sanitize_app_id(url_host(url).unwrap_or("custom"));
    let detail = if matches!(name_slug.as_str(), "connected_app" | "app") {
        host_slug
    } else {
        name_slug
    };
    format!("connected_{provider_slug}_{detail}")
        .chars()
        .take(64)
        .collect()
}

fn url_host(url: &str) -> Option<&str> {
    let after_scheme = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let authority = after_scheme.split('/').next()?.split('@').last()?;
    let host = authority.split(':').next()?.trim();
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

fn connected_app_index_html(spec: &ConnectedAppSpec) -> Result<String, String> {
    let config = serde_json::json!({
        "provider": spec.provider.clone(),
        "displayName": spec.name.clone(),
        "serviceUrl": spec.url.clone(),
        "openUrl": spec.open_url.clone(),
        "capabilities": spec.capabilities.clone(),
        "notes": spec.notes.clone(),
        "recommendedMcp": spec.mcp.clone(),
        "storageKey": format!("connected.{}.latestVisibleSession", spec.provider),
        "learnedKey": format!("connected.{}.learnedInterface", spec.provider),
        "eventTopic": format!("connected.{}.visible_session", spec.provider),
    });
    let config_json = serde_json::to_string(&config).map_err(|e| e.to_string())?;
    Ok(CONNECTED_APP_INDEX_HTML.replace("__CONNECTED_APP_CONFIG__", &config_json))
}

const CONNECTED_APP_INDEX_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Connected app</title>
  <style>
    :root {
      color-scheme: dark;
      font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
      background: #0d1117;
      color: #eef2ff;
    }
    * { box-sizing: border-box; }
    body {
      margin: 0;
      min-height: 100vh;
      background: #0d1117;
    }
    main {
      max-width: 980px;
      margin: 0 auto;
      padding: 22px;
      display: grid;
      gap: 14px;
    }
    header {
      display: flex;
      align-items: flex-start;
      justify-content: space-between;
      gap: 16px;
      border-bottom: 1px solid rgba(255,255,255,0.1);
      padding-bottom: 14px;
    }
    h1 {
      margin: 0;
      font-size: 22px;
      letter-spacing: 0;
    }
    p {
      margin: 6px 0 0;
      color: rgba(238,242,255,0.68);
      line-height: 1.5;
      font-size: 13px;
    }
    .status {
      min-width: 180px;
      border: 1px solid rgba(80,140,255,0.25);
      background: rgba(80,140,255,0.12);
      color: #cfe0ff;
      border-radius: 6px;
      padding: 8px 10px;
      font-size: 12px;
      text-align: right;
    }
    .actions {
      display: flex;
      flex-wrap: wrap;
      gap: 8px;
    }
    button {
      border: 1px solid rgba(255,255,255,0.12);
      background: rgba(255,255,255,0.07);
      color: #f8fbff;
      border-radius: 7px;
      padding: 8px 12px;
      font: inherit;
      font-size: 13px;
      cursor: pointer;
    }
    button.primary {
      background: rgba(80,140,255,0.25);
      border-color: rgba(80,140,255,0.55);
      color: #dbe7ff;
    }
    button:disabled {
      opacity: 0.55;
      cursor: not-allowed;
    }
    input, textarea {
      width: 100%;
      min-width: 0;
      border: 1px solid rgba(255,255,255,0.1);
      border-radius: 7px;
      background: rgba(0,0,0,0.24);
      color: #f8fbff;
      padding: 8px 10px;
      font: inherit;
      font-size: 12px;
      outline: none;
    }
    textarea {
      min-height: 92px;
      resize: vertical;
      font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
      line-height: 1.4;
    }
    input:focus, textarea:focus {
      border-color: rgba(80,140,255,0.58);
    }
    section {
      border: 1px solid rgba(255,255,255,0.09);
      background: rgba(255,255,255,0.035);
      border-radius: 8px;
      padding: 14px;
      display: grid;
      gap: 10px;
    }
    h2 {
      margin: 0;
      font-size: 13px;
      letter-spacing: 0;
      text-transform: uppercase;
      color: rgba(238,242,255,0.7);
    }
    pre {
      margin: 0;
      max-height: 380px;
      overflow: auto;
      white-space: pre-wrap;
      word-break: break-word;
      border-radius: 7px;
      border: 1px solid rgba(255,255,255,0.08);
      background: rgba(0,0,0,0.28);
      padding: 12px;
      color: rgba(238,242,255,0.86);
      font: 12px/1.45 ui-monospace, SFMono-Regular, Menlo, monospace;
    }
    .meta {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
      gap: 8px;
    }
    .chip {
      border-radius: 6px;
      border: 1px solid rgba(255,255,255,0.09);
      padding: 8px;
      background: rgba(0,0,0,0.18);
      min-width: 0;
    }
    .chip strong {
      display: block;
      font-size: 11px;
      color: rgba(238,242,255,0.55);
      margin-bottom: 4px;
      text-transform: uppercase;
      letter-spacing: 0;
    }
    .chip span {
      overflow-wrap: anywhere;
      font-size: 12px;
    }
    .form-grid {
      display: grid;
      grid-template-columns: minmax(0, 0.85fr) minmax(0, 1.15fr);
      gap: 8px;
    }
    .field {
      display: grid;
      gap: 5px;
    }
    .field span {
      font-size: 11px;
      color: rgba(238,242,255,0.58);
      text-transform: uppercase;
      letter-spacing: 0;
    }
  </style>
</head>
<body>
  <main>
    <header>
      <div>
        <h1 id="title">Connected app</h1>
        <p id="notes"></p>
      </div>
      <div id="status" class="status">Ready</div>
    </header>

    <div class="actions">
      <button class="primary" id="open">Open in Reflex Browser</button>
      <button id="read">Read visible text</button>
      <button id="summarize">Summarize visible text</button>
      <button id="learn">Learn interface</button>
      <button id="external">Open system browser</button>
    </div>

    <section>
      <h2>Bridge profile</h2>
      <div class="meta">
        <div class="chip"><strong>Provider</strong><span id="provider"></span></div>
        <div class="chip"><strong>Service URL</strong><span id="serviceUrl"></span></div>
        <div class="chip"><strong>Last tab</strong><span id="tabId">none</span></div>
      </div>
    </section>

    <section>
      <h2>MCP bridge</h2>
      <div class="form-grid">
        <label class="field">
          <span>Server name</span>
          <input id="mcpName" autocomplete="off" />
        </label>
        <label class="field">
          <span>Command</span>
          <input id="mcpCommand" autocomplete="off" />
        </label>
      </div>
      <label class="field">
        <span>Args JSON array</span>
        <textarea id="mcpArgs" spellcheck="false"></textarea>
      </label>
      <label class="field">
        <span>Env JSON object</span>
        <textarea id="mcpEnv" spellcheck="false"></textarea>
      </label>
      <div class="actions">
        <button id="saveMcp">Save MCP config</button>
        <button id="refreshMcp">Refresh MCP status</button>
      </div>
      <label class="field">
        <span>MCP agent query</span>
        <textarea id="mcpQuery" spellcheck="false"></textarea>
      </label>
      <div class="actions">
        <button id="runMcpQuery">Run MCP query</button>
      </div>
    </section>

    <section>
      <h2>Latest output</h2>
      <pre id="output">Open the service, log in if needed, then read or summarize the visible session.</pre>
    </section>
  </main>

  <script>
    const config = __CONNECTED_APP_CONFIG__;
    const state = { tabId: null, busy: false };

    const el = (id) => document.getElementById(id);
    const setStatus = (text) => { el("status").textContent = text; };
    const setOutput = (value) => {
      el("output").textContent = typeof value === "string" ? value : JSON.stringify(value, null, 2);
    };
    const setBusy = (busy) => {
      state.busy = busy;
      for (const button of document.querySelectorAll("button")) button.disabled = busy;
    };

    function textFromReadResult(value) {
      if (typeof value === "string") return value;
      if (!value || typeof value !== "object") return String(value ?? "");
      return value.text || value.body || value.content || JSON.stringify(value);
    }

    function normalizeTabs(value) {
      if (Array.isArray(value)) return value;
      if (value && Array.isArray(value.tabs)) return value.tabs;
      return [];
    }

    function recommendedMcpShape() {
      return (config.recommendedMcp && config.recommendedMcp.config_shape) || {
        command: "node",
        args: ["path/to/provider-mcp-server.js"],
        env: {}
      };
    }

    function initializeMcpForm() {
      const shape = recommendedMcpShape();
      el("mcpName").value = (config.recommendedMcp && config.recommendedMcp.server_name) || config.provider;
      el("mcpCommand").value = shape.command || "node";
      el("mcpArgs").value = JSON.stringify(shape.args || [], null, 2);
      el("mcpEnv").value = JSON.stringify(shape.env || {}, null, 2);
      el("mcpQuery").value = config.provider === "telegram"
        ? "Use the configured Telegram MCP server to list recent visible/available chats and summarize the latest messages. Do not claim access to chats or messages the MCP server cannot access. Return concise JSON with chats, latest_messages, summary, and warnings."
        : "Use the configured provider MCP server for this connected app to inspect available recent data. Do not claim access to data the MCP server cannot access. Return concise JSON with items, summary, and warnings.";
    }

    function parseJsonField(id, fallback) {
      const raw = el(id).value.trim();
      if (!raw) return fallback;
      return JSON.parse(raw);
    }

    async function saveMcpConfig() {
      setBusy(true);
      setStatus("Saving MCP config...");
      try {
        const name = el("mcpName").value.trim() || config.provider;
        const command = el("mcpCommand").value.trim();
        if (!command) throw new Error("MCP command is required");
        const args = parseJsonField("mcpArgs", []);
        const env = parseJsonField("mcpEnv", {});
        if (!Array.isArray(args)) throw new Error("Args must be a JSON array");
        if (!env || Array.isArray(env) || typeof env !== "object") {
          throw new Error("Env must be a JSON object");
        }
        const result = await window.reflexProjectMcpUpsert({
          name,
          config: { command, args, env }
        });
        await window.reflexIntegrationUpdate({
          provider: config.provider,
          display_name: config.displayName,
          capabilities: Array.from(new Set([...(config.capabilities || []), "mcp.configured"])),
          mcp: {
            ...(config.recommendedMcp || {}),
            configured: true,
            server_name: name,
            saved_at_ms: Date.now()
          },
          notes: config.notes
        });
        setOutput(result);
        setStatus("MCP config saved");
      } catch (error) {
        setStatus("MCP save failed");
        setOutput(String(error));
      } finally {
        setBusy(false);
      }
    }

    async function refreshMcpStatus() {
      setBusy(true);
      setStatus("Reading MCP status...");
      try {
        const result = await window.reflexMcpServers({ includeConfig: true });
        setOutput(result);
        setStatus("MCP status loaded");
      } catch (error) {
        setStatus("MCP status failed");
        setOutput(String(error));
      } finally {
        setBusy(false);
      }
    }

    async function runMcpAgentQuery() {
      setBusy(true);
      setStatus("Running MCP query...");
      try {
        const query = el("mcpQuery").value.trim();
        if (!query) throw new Error("MCP query is required");
        const result = await window.reflexAgentTask({
          sandbox: "read-only",
          includeContext: true,
          prompt: query
        });
        setOutput(result);
        setStatus("MCP query finished");
      } catch (error) {
        setStatus("MCP query failed");
        setOutput(String(error));
      } finally {
        setBusy(false);
      }
    }

    async function refreshProfile() {
      el("title").textContent = config.displayName;
      el("notes").textContent = config.notes;
      el("provider").textContent = config.provider;
      el("serviceUrl").textContent = config.serviceUrl;
      try {
        await window.reflexIntegrationUpdate({
          provider: config.provider,
          display_name: config.displayName,
          capabilities: config.capabilities,
          notes: config.notes
        }, {
          url: config.serviceUrl,
          open_url: config.openUrl,
          title: config.displayName
        });
      } catch (error) {
        console.warn("[connected-app] profile sync failed", error);
      }
      try {
        const savedTab = await window.reflexStorageGet("lastTabId");
        const saved = savedTab && (savedTab.value || savedTab);
        if (typeof saved === "string" && saved) {
          state.tabId = saved;
          el("tabId").textContent = saved;
        }
      } catch {}
    }

    async function openService() {
      setBusy(true);
      setStatus("Opening service...");
      try {
        await window.reflexBrowserInit({ headless: true });
        await window.reflexSystemOpenPanel({ panel: "browser" });
        const opened = await window.reflexBrowserOpen(config.serviceUrl);
        state.tabId = opened.tab_id || opened.tabId || null;
        el("tabId").textContent = state.tabId || "opened";
        if (state.tabId) await window.reflexStorageSet("lastTabId", state.tabId);
        setOutput(opened);
        setStatus("Service opened");
      } catch (error) {
        setStatus("Open failed");
        setOutput(String(error));
      } finally {
        setBusy(false);
      }
    }

    async function resolveTabId() {
      if (state.tabId) return state.tabId;
      const tabs = normalizeTabs(await window.reflexBrowserTabs());
      const serviceHost = new URL(config.serviceUrl).host;
      const match = tabs.find((tab) => {
        try { return new URL(tab.url || "").host === serviceHost; } catch { return false; }
      }) || tabs[0];
      if (match) {
        state.tabId = match.tab_id || match.tabId;
        el("tabId").textContent = state.tabId || "unknown";
        if (state.tabId) await window.reflexStorageSet("lastTabId", state.tabId);
      }
      return state.tabId;
    }

    async function readVisibleText() {
      setBusy(true);
      setStatus("Reading visible text...");
      try {
        await window.reflexBrowserInit({ headless: true });
        let tabId = await resolveTabId();
        if (!tabId) {
          await openService();
          tabId = await resolveTabId();
        }
        if (!tabId) throw new Error("No browser tab available");
        try {
          await window.reflexBrowserWaitFor({ tabId, selector: "body", timeoutMs: 15000 });
        } catch (error) {
          console.warn("[connected-app] wait for body failed", error);
        }
        const result = await window.reflexBrowserReadText(tabId);
        const text = textFromReadResult(result);
        const snapshot = {
          provider: config.provider,
          service_url: config.serviceUrl,
          tab_id: tabId,
          captured_at_ms: Date.now(),
          text
        };
        await window.reflexStorageSet(config.storageKey, snapshot);
        await window.reflexEventEmit(config.eventTopic, {
          provider: config.provider,
          captured_at_ms: snapshot.captured_at_ms,
          text_length: text.length
        });
        setOutput(text || result);
        setStatus("Visible text captured");
        return snapshot;
      } catch (error) {
        setStatus("Read failed");
        setOutput(String(error));
        throw error;
      } finally {
        setBusy(false);
      }
    }

    async function summarizeVisibleText() {
      setBusy(true);
      setStatus("Summarizing...");
      try {
        const snapshot = await readVisibleText();
        setBusy(true);
        const prompt = "Summarize the visible web-session text below. Only use visible content. If it appears to be a chat app, extract visible chats/messages and names when present. Do not claim access to hidden messages. Return concise JSON with summary, visible_items, and warnings.\n\nVISIBLE_TEXT:\n" + snapshot.text;
        const result = await window.reflexAgentTask({ prompt, includeContext: false });
        setOutput(result);
        setStatus("Summary ready");
      } catch (error) {
        setStatus("Summary failed");
        setOutput(String(error));
      } finally {
        setBusy(false);
      }
    }

    async function learnInterface() {
      setBusy(true);
      setStatus("Learning interface...");
      try {
        const snapshot = await readVisibleText();
        setBusy(true);
        const tabId = snapshot.tab_id || state.tabId;
        const outline = tabId ? await window.reflexBrowserReadOutline(tabId) : null;
        const prompt = "Build a connected-app adapter profile from the visible web UI below. Use only visible text and outline. Infer data entities, user actions, likely selectors or text anchors, safe automation workflows, and MCP opportunities. Do not claim access to hidden data. Return concise JSON with provider, entities, actions, workflows, selectors, data_access, mcp_bridge, risks, and next_steps.\n\nPROVIDER:\n" + config.provider + "\n\nSERVICE_URL:\n" + config.serviceUrl + "\n\nVISIBLE_TEXT:\n" + snapshot.text + "\n\nOUTLINE:\n" + JSON.stringify(outline);
        const learned = await window.reflexAgentTask({ prompt, includeContext: false });
        const profile = {
          provider: config.provider,
          service_url: config.serviceUrl,
          learned_at_ms: Date.now(),
          profile: learned.result || learned
        };
        await window.reflexStorageSet(config.learnedKey, profile);
        await window.reflexIntegrationUpdate({
          provider: config.provider,
          display_name: config.displayName,
          capabilities: Array.from(new Set([...(config.capabilities || []), "interface.visible_session.learn"])),
          data_model: { learned_profile: profile },
          notes: config.notes
        });
        setOutput(profile);
        setStatus("Interface profile saved");
      } catch (error) {
        setStatus("Learning failed");
        setOutput(String(error));
      } finally {
        setBusy(false);
      }
    }

    el("open").addEventListener("click", openService);
    el("read").addEventListener("click", () => void readVisibleText());
    el("summarize").addEventListener("click", summarizeVisibleText);
    el("learn").addEventListener("click", learnInterface);
    el("external").addEventListener("click", () => window.reflexSystemOpenUrl(config.openUrl || config.serviceUrl));
    el("saveMcp").addEventListener("click", saveMcpConfig);
    el("refreshMcp").addEventListener("click", refreshMcpStatus);
    el("runMcpQuery").addEventListener("click", runMcpAgentQuery);
    initializeMcpForm();
    refreshProfile();
  </script>
</body>
</html>
"#;

fn build_empty_app_thread(
    app: &AppHandle,
    app_id: &str,
    project: &project::Project,
) -> Result<ProjectThread, String> {
    let dir = apps::app_dir(app, app_id).map_err(|e| e.to_string())?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis();
    let thread_id = format!("t_{now_ms}");
    let manifest = apps::read_manifest(app, app_id).ok();
    let label = manifest
        .as_ref()
        .map(|m| m.name.clone())
        .unwrap_or_else(|| app_id.to_string());
    let meta = storage::ThreadMeta {
        id: thread_id.clone(),
        project_id: Some(project.id.clone()),
        prompt: format!("App revision: {label}"),
        cwd: project.root.clone(),
        frontmost_app: None,
        finder_target: None,
        created_at_ms: now_ms,
        exit_code: Some(0),
        done: true,
        session_id: None,
        title: Some(format!("App · {label}")),
        goal: Some("Revise the utility".into()),
        plan_mode: true,
        plan_confirmed: false,
        source: "quick".into(),
        browser_tabs: Vec::new(),
    };
    storage::write_meta(&dir, &meta).map_err(|e| e.to_string())?;

    let payload = ThreadCreated {
        id: thread_id.clone(),
        project_id: project.id.clone(),
        project_name: project.name.clone(),
        prompt: meta.prompt.clone(),
        cwd: project.root.clone(),
        ctx: QuickContext::default(),
        created_at_ms: now_ms,
        goal: meta.goal.clone(),
        plan_mode: meta.plan_mode,
        source: meta.source.clone(),
        browser_tabs: meta.browser_tabs.clone(),
    };
    let _ = app.emit(THREAD_CREATED_EVENT, &payload);

    Ok(ProjectThread {
        project: project.clone(),
        thread: storage::StoredThread {
            meta,
            events: vec![],
        },
    })
}

#[tauri::command]
fn create_app_thread(app: AppHandle, app_id: String) -> Result<ProjectThread, String> {
    let project = ensure_app_project(&app, &app_id)?;
    build_empty_app_thread(&app, &app_id, &project)
}

#[tauri::command]
async fn pick_directory(
    app: AppHandle,
    title: Option<String>,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let title = title.unwrap_or_else(|| "Select folder".to_string());
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().set_title(&title).pick_folder(move |p| {
        let _ = tx.send(p);
    });
    let picked = rx.await.map_err(|e| e.to_string())?;
    Ok(picked.map(|p| p.to_string()))
}

#[tauri::command]
async fn pick_open_file(
    app: AppHandle,
    title: Option<String>,
    filter_name: Option<String>,
    filter_extensions: Option<Vec<String>>,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let title = title.unwrap_or_else(|| "Open file".to_string());
    let mut builder = app.dialog().file().set_title(&title);
    if let (Some(name), Some(exts)) = (filter_name, filter_extensions) {
        let exts_ref: Vec<&str> = exts.iter().map(|s| s.as_str()).collect();
        builder = builder.add_filter(&name, &exts_ref);
    }
    let (tx, rx) = tokio::sync::oneshot::channel();
    builder.pick_file(move |p| {
        let _ = tx.send(p);
    });
    let picked = rx.await.map_err(|e| e.to_string())?;
    Ok(picked.map(|p| p.to_string()))
}

#[tauri::command]
async fn pick_save_file(
    app: AppHandle,
    title: Option<String>,
    default_name: Option<String>,
    filter_name: Option<String>,
    filter_extensions: Option<Vec<String>>,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let title = title.unwrap_or_else(|| "Save as".to_string());
    let mut builder = app.dialog().file().set_title(&title);
    if let Some(name) = default_name {
        builder = builder.set_file_name(&name);
    }
    if let (Some(name), Some(exts)) = (filter_name, filter_extensions) {
        let exts_ref: Vec<&str> = exts.iter().map(|s| s.as_str()).collect();
        builder = builder.add_filter(&name, &exts_ref);
    }
    let (tx, rx) = tokio::sync::oneshot::channel();
    builder.save_file(move |p| {
        let _ = tx.send(p);
    });
    let picked = rx.await.map_err(|e| e.to_string())?;
    Ok(picked.map(|p| p.to_string()))
}

#[tauri::command]
async fn read_app_thread(
    app: AppHandle,
    app_id: String,
) -> Result<ProjectThread, String> {
    let project = ensure_app_project(&app, &app_id)?;
    let dir = apps::app_dir(&app, &app_id).map_err(|e| e.to_string())?;

    // Existing thread? Return latest.
    let threads = storage::read_all_threads(&dir).map_err(|e| e.to_string())?;
    if let Some(latest) = threads.into_iter().max_by_key(|t| t.meta.created_at_ms) {
        return Ok(ProjectThread {
            project,
            thread: latest,
        });
    }

    // No thread yet — create an empty one ready for followup. continue_thread
    // will start the codex session lazily on the user's first followup.
    build_empty_app_thread(&app, &app_id, &project)
}

#[tauri::command]
fn app_revise(
    app: AppHandle,
    app_id: String,
    instruction: String,
) -> Result<serde_json::Value, String> {
    let trimmed = instruction.trim();
    if trimmed.is_empty() {
        return Err("empty instruction".into());
    }
    let dir = apps::app_dir(&app, &app_id).map_err(|e| e.to_string())?;
    let project = project::find_project_for(&dir)
        .ok_or_else(|| "app project not found".to_string())?;
    let threads = storage::read_all_threads(&dir).map_err(|e| e.to_string())?;
    let latest = threads
        .into_iter()
        .max_by_key(|t| t.meta.created_at_ms)
        .ok_or_else(|| "no thread for this app".to_string())?;
    let prompt = format!(
        "Revise the Reflex app in the current working directory.\n\n\
REQUESTED CHANGES: {trimmed}\n\n\
CURRENT BRIDGE / RUNTIME NOTES:\n\
- You may use any structure: index.html + style.css + app.js + assets/. Reflex serves files through the reflexapp:// scheme with the correct MIME type.\n\
- Runtime options: static (default), server (manifest.runtime=\"server\" + manifest.server.command; use Node/Python stdlib and listen on process.env.PORT), or external (manifest.runtime=\"external\" + manifest.external.url for embeddable external web apps).\n\
- The runtime overlay is already injected into HTML. Prefer window.reflex* helpers; use raw postMessage only for unusual bridge calls.\n\
- Keep or add multilingual user-facing UI when appropriate. Do not force Russian or English as the only UI language. Keep API names, permissions, ids, paths, and manifest keys as technical tokens.\n\
- Any prompt strings sent to agent.ask/agent.task/agent.stream must be written in English. If user input is in another language, pass it as data inside an English instruction.\n\
- After revising, check empty/loading/error/success states and main control accessibility. The first screen must remain a real usable tool, not a feature description.\n\
- Available bridge methods: bridge.catalog, system.context, system.openPanel, system.openUrl/openPath/revealPath, logs.write/list, manifest.get/update, integration.catalog/profile/update, permissions.*, network.*, widgets.*, actions.*, agent.*, storage.*, fs.*, clipboard.*, projects.*, topics.*, skills.*, mcp.*, project.files.*, browser.*, memory.*, scheduler.*, dialog.*, notify.show, net.fetch, events.*, apps.*.\n\
- Overlay helpers include reflexInvoke, reflexBridgeCatalog, reflexSystemContext, reflexSystemOpenPanel, reflexSystemOpenUrl/OpenPath/RevealPath, reflexLog/LogList, reflexManifestGet/Update, reflexIntegrationCatalog/Profile/Update, reflexPermissions*, reflexNetwork*, reflexWidgets*, reflexActions*, reflexCapabilities, reflexAgent*, reflexStorage*, reflexFs*, reflexClipboard*, reflexNetFetch, reflexDialog*, reflexNotifyShow, reflexProjects*, reflexTopics*, reflexSkills*, reflexMcp*, reflexProjectFiles*, reflexBrowser*, reflexMemory*, reflexScheduler*, reflexApps*, reflexEvent*.\n\
- iframe sandbox=\"allow-scripts allow-forms\"; server runtime also gets allow-same-origin. No external CDNs: use inline or local files.\n\
- Reflex injects an overlay script that captures window.onerror/unhandledrejection for Fix and supports Inspector pick events. Do not override those same event types with your own global handler.\n\
- After edits, the iframe reloads automatically; server runtime processes restart automatically. Do not ask for manual reload.\n\
- Do not touch .reflex/, .git/, or storage.json. You may update manifest.json permissions, network.allowed_hosts, runtime, and server fields."
    );
    continue_thread(app, project.id, latest.meta.id.clone(), prompt, None, None)?;
    Ok(serde_json::json!({"thread_id": latest.meta.id}))
}

#[tauri::command]
async fn create_app(
    app: AppHandle,
    description: String,
    template: Option<String>,
    project_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let trimmed = description.trim();
    if trimmed.is_empty() {
        return Err("empty description".into());
    }
    let (app_description, explicit_goal) = extract_goal_command(trimmed);
    if app_description.trim().is_empty() {
        return Err("empty description".into());
    }
    let template = template.unwrap_or_else(|| "blank".to_string());
    let target_project = match project_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(id) => Some(
            project::get_by_id(&app, id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("project not found: {id}"))?,
        ),
        None => None,
    };

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis();
    let app_id = format!("app_{now_ms}");

    let apps_root = apps::apps_dir(&app).map_err(|e| e.to_string())?;
    let dir = apps_root.join(&app_id);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let label = short_label(&app_description);
    let proj_name = format!("App · {label}");
    let project = project::create_project(&app, &dir, Some(proj_name.clone()), None)
        .map_err(|e| e.to_string())?;

    let manifest = apps::AppManifest {
        id: app_id.clone(),
        name: label.clone(),
        icon: Some("🧩".into()),
        description: Some(app_description.clone()),
        entry: "index.html".into(),
        permissions: vec![],
        kind: "panel".into(),
        created_at_ms: now_ms,
        runtime: None,
        server: None,
        external: None,
        integration: None,
        network: None,
        schedules: Vec::new(),
        actions: Vec::new(),
        widgets: Vec::new(),
    };
    apps::write_manifest(&app, &app_id, &manifest).map_err(|e| e.to_string())?;
    if let Some(target) = &target_project {
        link_app_to_project_inner(&app, &target.id, &app_id)?;
    }
    let _ = app.emit("reflex://apps-changed", &serde_json::json!({}));

    let thread_id = format!("t_{now_ms}");
    let project_root = dir.clone();
    let prompt = build_app_creation_prompt(&app_description, &template, target_project.as_ref());
    let goal = explicit_goal
        .clone()
        .unwrap_or_else(|| format!("Create a Reflex app: {app_description}"));

    let meta = storage::ThreadMeta {
        id: thread_id.clone(),
        project_id: Some(project.id.clone()),
        prompt: prompt.clone(),
        cwd: project.root.clone(),
        frontmost_app: None,
        finder_target: None,
        created_at_ms: now_ms,
        exit_code: None,
        done: false,
        session_id: None,
        title: Some(format!("App creation: {label}")),
        goal: Some(goal.clone()),
        plan_mode: true,
        plan_confirmed: false,
        source: "quick".into(),
        browser_tabs: Vec::new(),
    };
    let _ = storage::write_meta(&project_root, &meta);

    let payload = ThreadCreated {
        id: thread_id.clone(),
        project_id: project.id.clone(),
        project_name: project.name.clone(),
        prompt: prompt.clone(),
        cwd: project.root.clone(),
        ctx: QuickContext::default(),
        created_at_ms: now_ms,
        goal: meta.goal.clone(),
        plan_mode: meta.plan_mode,
        source: meta.source.clone(),
        browser_tabs: meta.browser_tabs.clone(),
    };
    let _ = app.emit(THREAD_CREATED_EVENT, &payload);

    let app_handle = app.clone();
    let reflex_id = thread_id.clone();
    let root_for_task = project_root.clone();
    let project_id_for_task = project.id.clone();
    let prompt_for_task = wrap_with_plan_mode(&prompt);
    tauri::async_runtime::spawn(async move {
        let handle = app_handle.state::<app_server::AppServerHandle>();
        let server = handle.wait().await;
        let proj_now = project::get_by_id(&app_handle, &project_id_for_task)
            .ok()
            .flatten();
        let sandbox = proj_now
            .as_ref()
            .map(|p| p.sandbox.clone())
            .unwrap_or_else(|| "workspace-write".into());
        let mcp = proj_now.as_ref().and_then(|p| p.mcp_servers.clone());
        let app_thread_id = match server
            .thread_start(&root_for_task, &sandbox, mcp.as_ref())
            .await
        {
            Ok(id) => id,
            Err(e) => {
                eprintln!("[reflex] create_app thread_start failed: {e}");
                return;
            }
        };
        server.register_thread(
            app_thread_id.clone(),
            reflex_id.clone(),
            root_for_task.clone(),
            0,
        );
        if let Ok(mut m) = storage::read_meta(&root_for_task, &reflex_id) {
            m.session_id = Some(app_thread_id.clone());
            let _ = storage::write_meta(&root_for_task, &m);
        }
        if let Err(e) = server.turn_start(&app_thread_id, &prompt_for_task).await {
            eprintln!("[reflex] create_app turn_start failed: {e}");
        }
    });

    Ok(serde_json::json!({
        "app_id": app_id,
        "thread_id": thread_id,
        "project_id": project.id,
    }))
}

fn extract_goal_command(input: &str) -> (String, Option<String>) {
    if let Some(rest) = input.strip_prefix("/goal") {
        let has_command_boundary = rest
            .chars()
            .next()
            .map(char::is_whitespace)
            .unwrap_or(true);
        if has_command_boundary {
            let goal = rest.trim().to_string();
            return (
                goal.clone(),
                if goal.is_empty() { None } else { Some(goal) },
            );
        }
    }
    (input.trim().to_string(), None)
}

fn short_label(s: &str) -> String {
    let mut iter = s.chars();
    let truncated: String = iter.by_ref().take(48).collect();
    truncated.trim().trim_end_matches('.').to_string()
}

fn wrap_with_plan_mode(prompt: &str) -> String {
    format!(
        "PLANNING MODE: inspect first, then write the plan.\n\n\
First inspect the repository and relevant context, then write a plan based on what you found. The plan is an execution document, not a description of how you will inspect.\n\n\
ALLOWED IN THIS TURN:\n\
- read files, list directories, search with rg/grep, inspect git log/diff;\n\
- run read-only commands for understanding, such as `cargo check` or `--help`;\n\
- read as much as needed through tools to be confident.\n\n\
NOT ALLOWED:\n\
- modify files, create files, or delete anything;\n\
- install dependencies, run migrations, or run any command with side effects;\n\
- guess behavior when you can inspect and verify it.\n\n\
ORDER:\n\
1. Find and read the relevant files. In the plan, cite concrete paths and lines you inspected.\n\
2. Identify existing patterns, functions, and types to reuse instead of writing from scratch.\n\
3. Only then write the plan.\n\n\
PLAN STRUCTURE, concrete and concise:\n\
- Context: what you understood from the task and code, in 1-3 sentences.\n\
- Files touched: paths marked create/change and why.\n\
- Reusable building blocks: existing functions/types you will call, with file:line.\n\
- Execution steps: the step-by-step list you will follow after `go`.\n\
- Key decisions: structure, API, libraries, and why.\n\
- Verification: tests, manual scenarios, and commands.\n\
- Open questions: only real ambiguities, if any.\n\n\
End with exactly this confirmation request:\n\
`Waiting for confirmation. Type go and I will execute the plan as written, or tell me what to change and I will re-plan.`\n\n\
TASK:\n{prompt}"
    )
}

fn wrap_with_plan_revision(feedback: &str) -> String {
    format!(
        "PLANNING MODE: this is a plan revision, not execution.\n\n\
The user clarified or corrected the previous plan. Do NOT modify files and do NOT run side-effecting commands in this turn. You may read additional context if needed. \
Update the plan according to the feedback and ask for confirmation again before executing.\n\n\
USER FEEDBACK:\n{feedback}"
    )
}

fn project_agent_profile_preface(project: &project::Project) -> String {
    let description = project.description.as_deref().unwrap_or("").trim();
    let instructions = project
        .agent_instructions
        .as_deref()
        .unwrap_or("")
        .trim();
    let skills: Vec<&str> = project
        .skills
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    let mcp_names: Vec<String> = project
        .mcp_servers
        .as_ref()
        .and_then(|v| v.as_object())
        .map(|obj| obj.keys().cloned().collect())
        .unwrap_or_default();
    let linked_apps: Vec<&str> = project
        .apps
        .iter()
        .map(|id| id.trim())
        .filter(|id| !id.is_empty())
        .collect();

    let mut buf = String::from("## Reflex project profile\n");
    buf.push_str(&format!("- Project: {}\n", project.name));
    if !description.is_empty() {
        buf.push_str(&format!("- Description: {description}\n"));
    }
    if !skills.is_empty() {
        buf.push_str("- Preferred skills: ");
        buf.push_str(&skills.join(", "));
        buf.push('\n');
    }
    if !mcp_names.is_empty() {
        buf.push_str("- MCP servers available in this project: ");
        buf.push_str(&mcp_names.join(", "));
        buf.push('\n');
    }
    if !linked_apps.is_empty() {
        buf.push_str("- Linked Reflex apps: ");
        buf.push_str(&linked_apps.join(", "));
        buf.push('\n');
    }
    buf.push_str(
        "\n### Reflex operating context\n\
- Treat this as a project-scoped macOS agent workspace: topics, generated apps, widgets, MCP servers, skills, memory/RAG, and automations can work together.\n\
- If preferred skills are listed and one matches the task, explicitly use that skill/workflow before coding.\n\
- If MCP servers are listed, consider them available for this project and use the relevant one instead of re-implementing that capability.\n\
- Prefer project memory/RAG for durable facts and indexed files; save durable decisions when they should affect future work.\n\
- For repeatable background work, prefer generated Reflex apps with manifest.schedules/actions/widgets over ad-hoc scripts.\n\
- For reusable project tools, prefer generated Reflex apps with a clear bridge API surface and documented permissions.\n",
    );
    if !instructions.is_empty() {
        buf.push_str("\n### Project instructions\n");
        buf.push_str(instructions);
        buf.push('\n');
    }
    buf
}

fn wrap_with_project_agent_profile(profile: &str, prompt: &str) -> String {
    if profile.trim().is_empty() {
        prompt.to_string()
    } else {
        format!("{profile}\n---\n\n{prompt}")
    }
}

fn stored_event_has_agent_output(ev: &storage::StoredEvent) -> bool {
    if ev.stream != "stdout" {
        return false;
    }
    let parsed: serde_json::Value = match serde_json::from_str(&ev.raw) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let msg = parsed.get("msg").unwrap_or(&parsed);
    let msg_type = msg
        .get("type")
        .or_else(|| parsed.get("type"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if msg_type == "item.agentMessage.delta" || msg_type == "agent_message_delta" {
        return true;
    }
    if msg_type == "agent_message" {
        return true;
    }
    if msg_type == "turn.completed" {
        return msg
            .get("turn")
            .or_else(|| parsed.get("turn"))
            .and_then(|turn| {
                turn.get("lastAgentMessage")
                    .or_else(|| turn.get("last_agent_message"))
            })
            .is_some();
    }
    if msg_type == "item.completed" {
        let item = msg.get("item").or_else(|| parsed.get("item"));
        let item_type = item
            .and_then(|it| it.get("type").or_else(|| it.get("kind")))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        return item_type.contains("agentmessage")
            || item_type.contains("agent_message")
            || item_type.contains("assistantmessage")
            || item_type.contains("assistant_message")
            || item_type == "assistant"
            || item_type == "agent";
    }

    false
}

fn template_skeleton(template: &str) -> Option<&'static str> {
    match template {
        "chat" => Some(
            "CHAT-UTILITY TEMPLATE:\n\
- Layout: message list on top, textarea, and a localized action button below.\n\
- Use `window.reflexAgentStream({prompt})` to stream the response. Listen for window 'message' events with {source:'reflex', type:'stream.token', streamId, token} and 'stream.done'.\n\
- Store message history through `window.reflexStorageGet/Set(\"messages\", value)`.\n\
- User and agent messages should be visually distinct. The streaming message grows token by token.\n",
        ),
        "dashboard" => Some(
            "DASHBOARD TEMPLATE:\n\
- A localized refresh action calls `window.reflexAgentTask({prompt: \"Return the requested data as strict JSON.\"})` and parses the JSON response.\n\
- For health/ops dashboards, first check ready-made APIs: `window.reflexSchedulerStats()`, `window.reflexMemoryStats({projectId})`, `window.reflexAppsStatus(appId)`.\n\
- Show data in a table or summary cards; errors should link to run/service detail when an id exists.\n\
- Cache the latest result with `window.reflexStorageSet(\"lastResult\", data)`.\n",
        ),
        "health-dashboard" => Some(
            "HEALTH-DASHBOARD TEMPLATE:\n\
- Add at least `scheduler.read:*` and `apps.manage` to manifest.permissions; for arbitrary projectId outside linked projects, add `memory.project:*`.\n\
- Build the first-screen operational dashboard without agent.task: use `window.reflexSystemContext()`, `window.reflexSchedulerStats({includeAll: true, recentLimit: 200})`, `window.reflexMemoryStats({projectId})`, `window.reflexAppsList()`, and `window.reflexAppsStatus(appId)` for linked apps.\n\
- If the app is linked to a project, get projectId from `system.context().linked_projects[0]?.id`; if there is no project, show scheduler/app health and a soft empty state for RAG.\n\
- Summary cards: active/paused/invalid schedules, next fire, recent run errors, indexed/stale/missing memory docs, linked app health. A run error should open through `window.reflexSchedulerRunDetail(runId)`.\n\
- Add a localized refresh action, autosave the latest snapshot through `window.reflexStorageSet(\"healthSnapshot\", data)`, and restore through `window.reflexStorageGet`.\n\
- Add manifest.widgets with a compact `widgets/health.html` that shows the same key counters and opens the main app.\n",
        ),
        "form" => Some(
            "FORM-TOOL TEMPLATE:\n\
- Put several input fields at the top and a localized submit action below.\n\
- On submit, collect values, call `window.reflexAgentTask({prompt: \"Use these form values to complete the requested task: ...\"})`, and show the result.\n\
- Store the latest submit with `window.reflexStorageSet(\"lastSubmit\", values)` as a preset.\n",
        ),
        "api-client" => Some(
            "API-CLIENT TEMPLATE:\n\
- Use `window.reflexNetFetch(...)` for the target API. Before the first request, call `await window.reflexNetworkAllowHost(\"<host>\")` so the host is added to manifest.network.allowed_hosts without manual merging.\n\
- Provide a localized request action and render the result with JSON pretty-printing.\n\
- If secrets are needed, ask the user through an input field and store them with `window.reflexStorageSet`.\n",
        ),
        "connected-app" => Some(
            "CONNECTED-APP TEMPLATE:\n\
- Build a wrapper around an external product or service. The first screen must be a usable control surface, not only notes.\n\
- Choose the display layer deliberately:\n\
  1) `runtime: \"external\"` with `manifest.external.url` when a web app can be framed and the user benefits from seeing it directly.\n\
  2) `runtime: \"static\"` or `runtime: \"server\"` when the external app blocks framing, needs a local companion UI, or needs a backend adapter.\n\
- Fill `manifest.integration` with provider, display_name, capabilities, auth, data_model, mcp, and notes. Use `integration.catalog`, `integration.profile`, and `integration.update` from the bridge to keep it current.\n\
- Add at least one public manifest.action that exposes normalized data to other Reflex apps, even if the first version returns an empty/auth-required state.\n\
- If the service has a web UI, use the Browser bridge for visible-session workflows: `browser.init`, `browser.open`, `browser.readText`, `browser.readOutline`, and `browser.click/fill` where permitted.\n\
- If durable data access needs credentials or a local protocol, write a clear MCP plan in `manifest.integration.mcp` and expose UI fields/checks for the user to configure it later.\n\
- For Telegram-like apps: do not claim arbitrary personal message access through the Bot API. Personal chats require user-approved Telegram client access such as MTProto/TDLib/MCP, or reading a visible Telegram Web session after the user logs in. Store only derived summaries by default unless the user explicitly chooses raw message storage.\n",
        ),
        "automation" => Some(
            "AUTOMATION TEMPLATE:\n\
- Add at least one manifest.schedules entry with a 6-field cron expression (sec min hour dom month dow, UTC).\n\
- In schedule.steps, use non-UI bridge methods: agent.task, storage.*, fs.*, project.files.*, net.fetch, notify.show, events.*, apps.invoke, memory.*, manifest.*, scheduler.list/runs/stats/runDetail.\n\
- Do not use dialog.*, clipboard.*, system.openPanel/openUrl/openPath/revealPath, apps.create/import/commit/commitPartial/delete/restore/revert/purge, apps.open, projects.open, topics.open, or scheduler.runNow/setPaused/upsert/delete inside schedule.steps.\n\
- Add a normal UI showing state: `window.reflexSchedulerStats()`, `window.reflexSchedulerList()`, `window.reflexSchedulerRuns({limit: 20})`, run detail through `window.reflexSchedulerRunDetail(runId)`, and manual run through `window.reflexSchedulerRunNow(scheduleId)`.\n\
- If the automation produces useful data, store it through `window.reflexStorageSet` or `window.reflexMemorySave`, and add manifest.actions for other apps.\n\
- If the result belongs on a project dashboard, add manifest.widgets with a compact widgets/<id>.html page.\n",
        ),
        "node-server" => Some(
            "NODE-SERVER TEMPLATE:\n\
- Use runtime=server with command=[\"node\", \"server.js\"].\n\
- In server.js, use Node.js stdlib `http` and listen on `process.env.PORT`.\n\
- Basic routes: GET / -> index.html via fs.readFileSync, GET /api/... -> JSON.\n\
- index.html calls /api through fetch; same-origin works because server runtime gets allow-same-origin sandbox.\n",
        ),
        _ => None,
    }
}

fn build_app_creation_prompt(
    description: &str,
    template: &str,
    target_project: Option<&project::Project>,
) -> String {
    let mut p = String::new();
    p.push_str("You are creating a Reflex app in the current working directory.\n\n");
    p.push_str("IMPORTANT CONTEXT:\n");
    p.push_str("- The person describing the task is not necessarily a programmer. They may use everyday language and may not know libraries, edge cases, or implementation details.\n");
    p.push_str("- You are an assistant, not a developer peer asking for technical decisions. Own the technical design. Do not ask which stack to use, how to name functions, or what an API should return unless the product behavior itself is unclear.\n");
    p.push_str("- Choose the most appropriate technical solution for the task. Prefer stdlib and minimal dependencies when suitable, but do not underbuild. Avoid both over-engineering and under-engineering.\n");
    p.push_str("- Think through logic before coding: edge cases, errors, empty states, and what the user sees. It is better to reason carefully than to need a revision.\n");
    p.push_str("- Ask the user only when the actual product task is ambiguous, for example which fields to show, what should happen for an empty list, or which data source to use. Answer technical questions yourself.\n");
    p.push_str("- Do the work only when you understand the task. If there is substantial ambiguity, ask first.\n\n");
    p.push_str("FILES:\n");
    p.push_str("- manifest.json already exists as a stub. Update name, icon (one emoji), description (one sentence), and permissions (array of API permission strings).\n");
    p.push_str("- For external-service wrappers, add manifest.integration with a learned provider/data/MCP profile. If the web surface itself should be embedded, use runtime=\"external\" plus manifest.external.url; otherwise build a local companion UI.\n");
    p.push_str("- You may use any file structure: index.html + style.css + app.js + assets/, modules, and so on. Reflex serves all files from the app directory with automatic MIME types.\n");
    p.push_str("- Use a dark theme with color #f5f5f7 on a transparent background. Keep the UI clean and minimal.\n\n");
    p.push_str("UI/UX:\n");
    p.push_str("- User-facing UI must be multilingual-ready when the app has meaningful labels, states, or messages. At minimum, keep labels centralized so Russian and English can be supported without rewriting business logic.\n");
    p.push_str("- Choose the initial visible language from an explicit user request when provided; otherwise use the host/browser language when detectable. Do not hardcode Russian or English as the only UI language. Keep API names, permissions, paths, ids, and manifest keys as technical tokens.\n");
    p.push_str("- Prompts sent through agent.ask, agent.task, or agent.stream must be written in English. If user-provided content is in another language, include it as data inside an English instruction.\n");
    p.push_str("- The first screen must be a working utility, not a landing page: immediately show data, a form, a dashboard, or action controls.\n");
    p.push_str("- Add clear controls and states: disabled, loading, empty, error, retry/refresh, and last synchronization when relevant.\n");
    p.push_str("- Button labels should be localized action verbs. Do not leave placeholder English labels in a non-English UI, and do not leave untranslated Russian labels in an English UI.\n\n");
    p.push_str("TWO RUNTIMES:\n");
    p.push_str("1) static (default): pure front-end. The iframe points to reflexapp://localhost/<id>/<entry>. There is no app-owned backend.\n");
    p.push_str("   - manifest: { runtime: \"static\", entry: \"index.html\" } or omit runtime.\n");
    p.push_str("   - Do not load external CDNs; use local files or inline assets only.\n");
    p.push_str("2) server: when opened, Reflex starts a local web server from manifest.server.command and passes the port through REFLEX_PORT and PORT. The iframe points to reflexserver://<app-id>/, which proxies the local server and injects the overlay.\n");
    p.push_str("   - manifest: { runtime: \"server\", server: { command: [\"node\", \"server.js\"], ready_timeout_ms: 15000 } }\n");
    p.push_str("   - Process cwd is the app directory. The server MUST listen on process.env.PORT or REFLEX_PORT.\n");
    p.push_str("   - Dependencies must be vendored into the app directory or be stdlib. Do not assume global npm install. Prefer plain Node.js stdlib (http/fs/path) or Python stdlib (http.server/socketserver).\n");
    p.push_str("   - entry can be omitted for server runtime because it is not used.\n\n");
    p.push_str("3) external: the iframe points directly to manifest.external.url. Use this only when the external web app can be framed. The Reflex overlay is not injected into cross-origin pages, so data operations must live in a companion UI, manifest.actions, Browser bridge workflows, or MCP.\n");
    p.push_str("   - manifest: { runtime: \"external\", external: { url: \"https://...\", title: \"...\", open_url: \"https://...\" }, integration: {...} }\n");
    p.push_str("   - Some services block embedding or require a full browser for login; include an open_url fallback and a clear empty/error state.\n\n");
    p.push_str("BRIDGE, communicating with Reflex through window.parent.postMessage:\n");
    p.push_str("Request:  window.parent.postMessage({source:'reflex-app', type:'request', id, method, params}, '*');\n");
    p.push_str("Response: window.addEventListener('message', e => {\n");
    p.push_str("           if (e.data?.source==='reflex' && e.data.type==='response' && e.data.id===id) ...\n");
    p.push_str("         });\n\n");
    if let Some(project) = target_project {
        p.push_str("TARGET PROJECT:\n");
        p.push_str(&format!(
            "- This app will be automatically linked to project `{}` ({}) at path `{}`.\n",
            project.name, project.id, project.root
        ));
        if let Some(description) = project
            .description
            .as_ref()
            .filter(|s| !s.trim().is_empty())
        {
            p.push_str(&format!("- Project description: {}\n", description.trim()));
        }
        if !project.skills.is_empty() {
            p.push_str(&format!(
                "- Project preferred skills: {}.\n",
                project.skills.join(", ")
            ));
        }
        let mcp_names: Vec<String> = project
            .mcp_servers
            .as_ref()
            .and_then(|v| v.as_object())
            .map(|servers| servers.keys().cloned().collect())
            .unwrap_or_default();
        if !mcp_names.is_empty() {
            p.push_str(&format!(
                "- Project MCP servers available to agent tasks when cwd is this project: {}.\n",
                mcp_names.join(", ")
            ));
        }
        p.push_str("- The runtime app will see this project in system.context().linked_projects and can use it as the default project memory scope.\n\n");
    }
    p.push_str("AVAILABLE METHODS:\n");
    p.push_str("  bridge.catalog() -> {methods, helpers, permissions, app, notes}; runtime self-discovery for bridge API, overlay helpers, permission hints, and current grants\n");
    p.push_str("  system.context() -> {app_id, app_root, manifest, app_project, linked_projects, memory_defaults}; current app context; app_project/linked_projects are summaries with skills and mcp_server_names, not raw MCP config\n");
    p.push_str("  system.openPanel({panel, projectId?, threadId?}) -> {ok, panel}; open a Reflex panel: apps|memory|automations|browser|settings; memory may receive context\n");
    p.push_str("  system.openUrl({url}) -> {ok, url}; open http/https/mailto/tel URL in the default system app\n");
    p.push_str("  system.openPath({path}) -> {ok, path}; system.revealPath({path}) -> {ok, path}; open or reveal an existing local file/folder. Relative paths resolve from the app directory.\n");
    p.push_str("  logs.write({level?, source?, message}) -> {ok}; logs.list({limit?, sinceSeq?, source?, level?}) -> {entries, latestSeq}; app-scoped diagnostics in Settings -> Logs. level: trace|debug|info|warn|error\n");
    p.push_str("  manifest.get() -> AppManifest; manifest.update({patch}) -> {ok, manifest}; safely merge-update this app's manifest.json. The id remains the current app id.\n");
    p.push_str("  integration.catalog({provider?}) -> {recipes}; built-in connected-app recipes such as generic_web and telegram, including display URL, data strategy, and MCP config shape.\n");
    p.push_str("  integration.profile() -> {app_id, provider, integration, external, runtime, linked_projects, app_project}; current connected-app profile for external services and MCP/data adapters.\n");
    p.push_str("  integration.update({integration?|patch?, external?}) -> {ok, integration, external}; merge-update only manifest.integration and/or manifest.external without hand-editing the whole manifest.\n");
    p.push_str("  permissions.list() -> {permissions}; permissions.ensure({permission}) or ensure({permissions}) -> {ok, added, permissions}; permissions.revoke(...) -> {ok, removed, permissions}; targeted manifest.permissions updates without manual merging\n");
    p.push_str("  network.hosts() -> {allowed_hosts}; network.allowHost({host}) or allowHost({hosts}) -> {ok, added, allowed_hosts}; network.revokeHost(...) -> {ok, removed, allowed_hosts}; targeted manifest.network.allowed_hosts updates for net.fetch\n");
    p.push_str("  widgets.list() -> {widgets}; widgets.upsert({id, name?, entry?, size?, description?, html?}) or widgets.upsert({widget, html?}) -> {ok, created, widget}; widgets.delete({widgetId, deleteEntry?}) -> {ok, deleted}; manage dashboard widgets without manual manifest merging\n");
    p.push_str("  actions.list() -> {actions}; actions.upsert({id, name?, description?, public?, params_schema?, steps}) or actions.upsert({action}) -> {ok, created, action}; actions.delete({actionId}) -> {ok, deleted}; publish callable API for apps.invoke without manual manifest merging\n");
    p.push_str("  agent.ask({prompt}) -> {answer}; short one-shot question to the agent\n");
    p.push_str("  agent.startTopic({prompt, projectId?}) -> {threadId}; create a full Reflex topic\n");
    p.push_str("  agent.task({prompt, sandbox?, cwd?, memoryThreadId?, includeContext?}) -> {threadId, result}; isolated sub-agent; sandbox: read-only|workspace-write; waits for turn.completed and returns final text. cwd may be app root or linked project; foreign projects require permission \"agent.project:<project>\" / \"agent.project:*\", arbitrary cwd requires \"agent.cwd:*\". Project cwd automatically receives MCP config, preferred skills, project profile, and memory/RAG context. includeContext=false only for a raw prompt; memoryThreadId attaches topic memory.\n");
    p.push_str("  agent.stream({prompt, sandbox?, cwd?, memoryThreadId?, includeContext?}) -> {streamId, threadId}; token stream. Listen for parent window 'message' with {source:'reflex', type:'stream.token', streamId, token} and 'stream.done' with {streamId, result}. Call agent.streamAbort({threadId}) when unmounting. cwd/context rules match agent.task.\n");
    p.push_str("  storage.get({key}) -> {value}; persisted in storage.json\n");
    p.push_str("  storage.set({key, value}) -> {ok}\n");
    p.push_str("  storage.list({prefix?}) -> {keys, entries}; storage.delete({key}) or storage.delete({keys}) -> {ok, deleted, missing}\n");
    p.push_str("  fs.read({path}) -> {content}; read a file inside the app directory\n");
    p.push_str("  fs.list({path?, recursive?, includeHidden?}) -> {entries}; list files inside the app directory\n");
    p.push_str("  fs.write({path, content}) -> {ok}; write a file inside the app directory\n");
    p.push_str("  fs.delete({path, recursive?}) -> {ok, path, kind}; delete an app file/folder. The app root cannot be deleted.\n");
    p.push_str("  clipboard.readText() -> {text}; clipboard.writeText({text}) -> {ok}; macOS clipboard; requires permission \"clipboard.read\"/\"clipboard.write\" or \"clipboard:*\"\n");
    p.push_str("  notify.show({title, body}) -> {ok}                    — macOS push\n");
    p.push_str("  dialog.openDirectory({title?, defaultPath?}) -> {path|null}; native folder picker. path = null when cancelled.\n");
    p.push_str("  dialog.openFile({title?, defaultPath?, filters?, multiple?}) -> {path|null} or {paths:[]}; native file picker. filters: [{name, extensions:[\"txt\",...]}]\n");
    p.push_str("  dialog.saveFile({title?, defaultPath?, filters?, content?}) -> {path|null}; native save dialog. If content is provided as a string, the file is written immediately to the selected path.\n");
    p.push_str("  net.fetch({url, method?, headers?, body?, timeoutMs?}) -> {status, headers, body, encoding}; HTTP request. The host MUST be in manifest.network.allowed_hosts; supports \"*.example.com\". Add hosts through network.allowHost/reflexNetworkAllowHost. Body may be a string or JSON, which is auto-serialized. encoding=\"utf8\"|\"base64\".\n\n");
    p.push_str("PROJECT/TOPIC API: use it for OS dashboards, navigation, and agent work overview.\n");
    p.push_str("  projects.list({includeAll?}) -> ProjectSummary[]; by default returns only linked projects. includeAll requires permission \"projects.read:*\"\n");
    p.push_str("  projects.open({projectId}) -> {ok, project_id}; open a project in the main UI. Access rules match projects.list.\n");
    p.push_str("  project.profile.update({projectId?, description?, agentInstructions?}) -> {ok, changed, project}; updating project description/agent profile requires \"projects.write:<project>\" or \"projects.write:*\". null or empty string clears the field.\n");
    p.push_str("  project.sandbox.set({projectId?, sandbox}) -> {ok, changed, sandbox, project}; sandbox: read-only|workspace-write|danger-full-access; requires \"projects.write:<project>\" or \"projects.write:*\"\n");
    p.push_str("  project.apps.link({projectId?, appId?}) / project.apps.unlink({projectId?, appId?}) -> {ok, linked|unlinked, app_id, project}; link app to project. appId defaults to current app. Requires \"projects.write:<project>\" or \"projects.write:*\"\n");
    p.push_str("  topics.list({projectId?, limit?, includeAll?}) -> TopicSummary[]; topic metadata without raw events. Foreign projects require \"topics.read:<project>\" or \"topics.read:*\"\n");
    p.push_str("  topics.open({threadId, projectId?}) -> {ok, project_id, thread_id}; open topic in the main UI. Access rules match topics.list.\n");
    p.push_str("  project.files.list({projectId?, path?, recursive?, includeHidden?}) -> {project_id, project_name, entries}; read-only list of linked project files. Foreign projects require \"project.files.read:<project>\" or \"project.files.read:*\". .reflex is always hidden.\n");
    p.push_str("  project.files.read({projectId?, path}) -> {project_id, project_name, path, size, content}; read UTF-8 linked project files up to 1 MiB. .reflex is always blocked.\n");
    p.push_str("  project.files.search({projectId?, query, path?, recursive?, includeHidden?, includeContent?, limit?}) -> {project_id, project_name, query, matches, scanned, truncated}; searches path/name, and includeContent=true scans UTF-8 files up to 256 KiB each. Access rules match project.files.read.\n");
    p.push_str("  project.files.write({projectId?, path, content, createDirs?, overwrite?}) -> {ok, project_id, project_name, path, created, size}; project.files.mkdir({projectId?, path, recursive?}); project.files.move({projectId?, from, to, createDirs?, overwrite?}); project.files.copy({projectId?, from, to, createDirs?, overwrite?, recursive?}); project.files.delete({projectId?, path, recursive?}); changing project files requires \"project.files.write:<project>\" or \"project.files.write:*\". The project root and .reflex are protected.\n\n");
    p.push_str("SKILLS/MCP API: use it for project capability panels and workflow selection.\n");
    p.push_str("  skills.list({projectId?, includeAll?}) -> [{project_id, project_name, skills}]; linked projects are available without permission. Foreign projects require \"skills.read:<project>\" or \"skills.read:*\"\n");
    p.push_str("  project.skills.ensure({projectId?, skill}) or ensure({projectId?, skills}) -> {ok, added, skills}; project.skills.revoke(...) -> {ok, removed, skills}; updating project preferred skills requires \"skills.write:<project>\" or \"skills.write:*\"\n");
    p.push_str("  mcp.servers({projectId?, includeAll?, includeConfig?}) -> [{project_id, project_name, server_names, servers}]; names are available for linked projects. includeConfig requires \"mcp.read:<project>\" or \"mcp.read:*\"\n");
    p.push_str("  project.mcp.upsert({projectId?, name, config}) -> {ok, name, replaced, server_names}; project.mcp.delete({projectId?, name|names}) -> {ok, removed, server_names}; adding/removing project MCP servers requires \"mcp.write:<project>\" or \"mcp.write:*\"\n\n");
    p.push_str("BROWSER API: built-in Playwright/browser sidecar for research, QA, and web workflows.\n");
    p.push_str("  browser.init({headless?, projectId?}); project.browser.setEnabled({projectId?, enabled}) -> {ok, enabled, server_names}; browser.tabs.list(); browser.open({url?}); browser.close({tabId}); browser.setActive({tabId}); browser.navigate({tabId, url}); browser.back({tabId}); browser.forward({tabId}); browser.reload({tabId})\n");
    p.push_str("  browser.currentUrl({tabId}); browser.readText({tabId}); browser.readOutline({tabId}); browser.screenshot({tabId, fullPage?})\n");
    p.push_str("  browser.clickText({tabId, text, exact?}); browser.clickSelector({tabId, selector}); browser.fill({tabId, selector, value}); browser.scroll({tabId, dx?, dy?}); browser.waitFor({tabId, selector, timeoutMs?})\n");
    p.push_str("- Requires manifest.permissions: \"browser.read\" for read/currentUrl/waitFor, or \"browser.control\" for init/open/close/setActive/navigate/back/forward/reload/click/fill/scroll. Project browser state requires linked project or \"browser.project:<project>\". Enabling Reflex Browser MCP through project.browser.setEnabled requires \"mcp.write:<project>\" or \"mcp.write:*\".\n\n");
    p.push_str("SCHEDULER API: panels and widgets can show/control automations without manual JSON.\n");
    p.push_str("  scheduler.list({appId?, includeAll?}) -> ScheduleListItem[]; by default returns only schedules owned by this app\n");
    p.push_str("  scheduler.upsert({id, name?, cron, enabled?, catch_up?, steps}) or scheduler.upsert({schedule}) -> {ok, created, schedule_id, schedule}; create/update this app's schedule\n");
    p.push_str("  scheduler.delete({scheduleId}) -> {ok, deleted, schedule_id}; delete this app's schedule\n");
    p.push_str("  scheduler.runNow({scheduleId}) -> {ok, schedule_id}; scheduleId may be a local id or \"app::schedule\"\n");
    p.push_str("  scheduler.setPaused({scheduleId, paused}) -> {ok, schedule_id, paused}\n");
    p.push_str("  scheduler.runs({limit?, beforeTs?, appId?, includeAll?}) -> RunSummary[]\n");
    p.push_str("  scheduler.stats({appId?, includeAll?, recentLimit?}) -> {schedules, recent_runs} — counts, next fire timestamp, recent run counts, last error summary\n");
    p.push_str("  scheduler.runDetail({runId}) -> RunRecord|null\n");
    p.push_str("- Foreign apps/schedules require manifest.permissions: \"scheduler.read:*\", \"scheduler.run:<app>\", \"scheduler.write:<app>::<schedule>\", or \"scheduler:*\".\n\n");
    p.push_str("MEMORY API: use it for durable memory, RAG, and project context instead of custom JSON hacks.\n");
    p.push_str("  memory.save({scope?, kind?, name, description?, body, tags?, projectId?, threadId?}) -> MemoryNote\n");
    p.push_str("  memory.read({scope?, relPath, projectId?, threadId?}) -> MemoryNote\n");
    p.push_str("  memory.update({scope?, relPath, name?, description?, body?, tags?, kind?, projectId?, threadId?}) -> MemoryNote\n");
    p.push_str("  memory.list({scope?, filter?, projectId?, threadId?}) -> MemoryNote[]; filter: {kind?, tag?, query?}\n");
    p.push_str("  memory.delete({scope?, relPath, projectId?, threadId?}) -> {ok}\n");
    p.push_str("  memory.search({query, projectId?, limit?}) -> RagHit[]; search indexed project files and memory notes\n");
    p.push_str("  memory.recall({query, projectId?, threadId?, maxNotes?, maxRag?}) -> {markdown, notes, rag}; ready-to-use context for agents\n");
    p.push_str("  memory.stats({projectId?}) -> {docs, chunks, sources, stale, missing, last_indexed_at_ms, kinds}; RAG index health/coverage for dashboards or schedule monitoring\n");
    p.push_str("  memory.reindex({projectId?}) -> {indexed}; explicit RAG maintenance: reindex supported project files\n");
    p.push_str("  memory.indexPath({path, projectId?}) -> {indexed, skipped}; memory.pathStatus({path, projectId?}); memory.pathStatusBatch({paths, projectId?}); memory.forgetPath({path, projectId?})\n");
    p.push_str("- scope defaults to \"project\". If the app is linked to exactly one project, project scope targets that project memory; otherwise it targets the app's own memory.\n");
    p.push_str("- To choose a project, call system.context() and pass a projectId from linked_projects. For global scope, add permission \"memory.global.read\" or \"memory.global.write\".\n");
    p.push_str("- The overlay already provides helpers: reflexInvoke(method, params), reflexBridgeCatalog(), reflexSystemContext(), reflexSystemOpenPanel(panelOrParams, projectId?, threadId?), reflexSystemOpenUrl(urlOrParams), reflexSystemOpenPath(pathOrParams), reflexSystemRevealPath(pathOrParams), reflexLog(levelOrParams, message?), reflexLogList(params), reflexManifestGet(), reflexManifestUpdate(patch), reflexIntegrationCatalog(providerOrParams?), reflexIntegrationProfile(), reflexIntegrationUpdate(patchOrParams, external?), reflexPermissionsList(), reflexPermissionsEnsure(permissionOrParams), reflexPermissionsRevoke(permissionOrParams), reflexNetworkHosts(), reflexNetworkAllowHost(hostOrParams), reflexNetworkRevokeHost(hostOrParams), reflexWidgetsList(), reflexWidgetsUpsert(widgetOrParams), reflexWidgetsDelete(widgetIdOrParams, deleteEntry?), reflexActionsList(), reflexActionsUpsert(actionOrParams), reflexActionsDelete(actionIdOrParams), reflexCapabilities(), reflexProjectsList(params), reflexProjectsOpen(projectIdOrParams), reflexProjectProfileUpdate(patch), reflexProjectSandboxSet(sandboxOrParams), reflexProjectAppsLink(appIdOrParams?), reflexProjectAppsUnlink(appIdOrParams?), reflexTopicsList(params), reflexTopicsOpen(threadIdOrParams, projectId?), reflexSkillsList(params), reflexProjectSkillsEnsure(skillOrParams), reflexProjectSkillsRevoke(skillOrParams), reflexMcpServers(params), reflexProjectMcpUpsert(nameOrParams, config?), reflexProjectMcpDelete(nameOrParams), reflexProjectFilesList(pathOrParams, recursive?), reflexProjectFilesRead(pathOrParams), reflexProjectFilesSearch(queryOrParams, includeContent?), reflexProjectFilesWrite(pathOrParams, content?), reflexProjectFilesMkdir(pathOrParams), reflexProjectFilesMove(fromOrParams, to?), reflexProjectFilesCopy(fromOrParams, to?), reflexProjectFilesDelete(pathOrParams, recursive?), reflexProjectBrowserSetEnabled(projectIdOrParams, enabled?), reflexSchedulerList(params), reflexSchedulerUpsert(scheduleOrParams), reflexSchedulerDelete(scheduleIdOrParams), reflexSchedulerRunNow(scheduleId), reflexSchedulerSetPaused(scheduleId, paused), reflexSchedulerRuns(params), reflexSchedulerStats(params), reflexSchedulerRunDetail(runIdOrParams), reflexAppsList(params), reflexAppsCreate(descriptionOrParams, template?), reflexAppsExport(appIdOrParams, targetPath?), reflexAppsImport(zipPathOrParams), reflexAppsDelete(appIdOrParams), reflexAppsTrashList(), reflexAppsRestore(trashIdOrParams), reflexAppsPurge(trashIdOrParams), reflexAppsStatus(appIdOrParams), reflexAppsDiff(appIdOrParams), reflexAppsCommit(appIdOrParams, message?), reflexAppsCommitPartial(appIdOrParams, patch?, message?), reflexAppsRevert(appIdOrParams), reflexAppsServerStatus(appIdOrParams), reflexAppsServerLogs(appIdOrParams), reflexAppsServerStart(appIdOrParams), reflexAppsServerStop(appIdOrParams), reflexAppsServerRestart(appIdOrParams), reflexAppsOpen(appIdOrParams), reflexAppsInvoke(appId, actionId, params), reflexAppsListActions(appIdOrParams, includeSteps?), reflexEventOn/Off/Emit/Recent/Subscriptions/ClearSubscriptions.\n");
    p.push_str("  Core helpers: reflexAgentAsk/StartTopic/Task/Stream/StreamAbort(...), reflexStorageGet/Set/List/Delete(...), reflexFsRead/List/Write/Delete(...), reflexClipboardReadText(), reflexClipboardWriteText(textOrParams), reflexNetFetch(...), reflexDialogOpenDirectory/OpenFile/SaveFile(...), reflexNotifyShow(...).\n");
    p.push_str("  Browser helpers: reflexBrowserInit(params), reflexProjectBrowserSetEnabled(projectIdOrParams, enabled?), reflexBrowserTabs(), reflexBrowserOpen(url), reflexBrowserClose(tabIdOrParams), reflexBrowserSetActive(tabIdOrParams), reflexBrowserNavigate(tabId, url), reflexBrowserBack(tabIdOrParams), reflexBrowserForward(tabIdOrParams), reflexBrowserReload(tabIdOrParams), reflexBrowserCurrentUrl(tabIdOrParams), reflexBrowserReadText(tabId), reflexBrowserReadOutline(tabId), reflexBrowserScreenshot(tabIdOrParams, fullPage?), reflexBrowserClickText(tabIdOrParams, text?, exact?), reflexBrowserClickSelector(tabIdOrParams, selector?), reflexBrowserFill(tabIdOrParams, selector?, value?), reflexBrowserScroll(tabIdOrParams, dx?, dy?), reflexBrowserWaitFor(tabIdOrParams, selector?, timeoutMs?).\n");
    p.push_str("  Memory helpers: reflexMemorySave(params), reflexMemoryRead(relPathOrParams), reflexMemoryUpdate(relPathOrParams, patch?), reflexMemoryList(params), reflexMemoryDelete(relPathOrParams), reflexMemorySearch(queryOrParams), reflexMemoryRecall(queryOrParams), reflexMemoryStats(params), reflexMemoryReindex(params), reflexMemoryIndexPath(pathOrParams), reflexMemoryPathStatus(pathOrParams), reflexMemoryPathStatusBatch(pathsOrParams), reflexMemoryForgetPath(pathOrParams).\n\n");
    p.push_str("MANIFEST.network for net.fetch:\n");
    p.push_str("  { \"network\": { \"allowed_hosts\": [\"api.example.com\", \"*.foo.com\"] } }\n\n");
    p.push_str("- Prefer targeted calls such as await reflexNetworkAllowHost(\"api.example.com\") instead of manual manifest.update.\n\n");
    p.push_str("MANIFEST.schedules: recurring tasks. Reflex runs them while it is alive, even when the app window is closed.\n");
    p.push_str("  {\n");
    p.push_str("    \"schedules\": [{\n");
    p.push_str("      \"id\": \"morning-digest\",\n");
    p.push_str("      \"name\": \"Morning digest\",\n");
    p.push_str("      \"cron\": \"0 0 8 * * *\",          // 6 fields: sec min hour dom month dow (UTC). \"0 */5 * * * *\" = every 5 minutes\n");
    p.push_str("      \"enabled\": true,\n");
    p.push_str("      \"catch_up\": \"once\",              // if Reflex was off, run ONCE at startup\n");
    p.push_str("      \"steps\": [\n");
    p.push_str("        { \"method\": \"net.fetch\",  \"params\": {\"url\":\"...\"},                          \"save_as\": \"page\"    },\n");
    p.push_str("        { \"method\": \"agent.task\", \"params\": {\"prompt\":\"Summarize this content: {{steps.page.body}}\"}, \"save_as\": \"summary\" },\n");
    p.push_str("        { \"method\": \"storage.set\",\"params\": {\"key\":\"today\", \"value\":\"{{steps.summary.result}}\"} }\n");
    p.push_str("      ]\n");
    p.push_str("    }]\n");
    p.push_str("  }\n");
    p.push_str("- Steps run sequentially. Templates such as {{steps.X.field}} insert previous step results. If the placeholder is the entire string, the value type is preserved, so objects remain objects.\n");
    p.push_str("- schedule.steps MUST NOT use dialog.openDirectory/openFile/saveFile, clipboard.readText/writeText, system.openPanel/openUrl/openPath/revealPath, apps.create/import/commit/commitPartial/delete/restore/revert/purge, apps.open, projects.open, or topics.open because automations do not have UI.\n");
    p.push_str("- All other methods (agent.*, storage.*, fs.*, project.files.*, net.fetch, notify.show, events.*, apps.invoke, memory.*, manifest.*, scheduler.list/runs/stats/runDetail) work normally. scheduler.runNow/setPaused/upsert/delete are blocked in schedule.steps to avoid recursive unattended loops.\n");
    p.push_str("- If the task sounds like \"do X every N minutes/hours\", use a schedule, not only a UI button.\n\n");

    p.push_str("MANIFEST.actions: public operations OTHER apps can call through apps.invoke.\n");
    p.push_str("  {\n");
    p.push_str("    \"actions\": [{\n");
    p.push_str("      \"id\": \"today-summary\",\n");
    p.push_str("      \"name\": \"Today summary\",\n");
    p.push_str("      \"public\": true,                   // if false, caller must have permission \"apps.invoke:<this_app_id>\"\n");
    p.push_str("      \"params_schema\": {\"type\":\"object\",\"properties\":{}}, // optional JSON Schema for input params\n");
    p.push_str("      \"steps\": [\n");
    p.push_str("        { \"method\": \"storage.get\", \"params\": {\"key\":\"today\"}, \"save_as\": \"output\" }\n");
    p.push_str("      ]\n");
    p.push_str("    }]\n");
    p.push_str("  }\n");
    p.push_str("- Caller params are available as {{input.X}}.\n");
    p.push_str("- The action return value is the last step value, or save_as: \"output\" when you want to be explicit.\n\n");
    p.push_str("- You can create/update an action in one call: reflexActionsUpsert({id:\"today-summary\", public:true, steps:[...]}); then other apps call it through reflexAppsInvoke.\n\n");

    p.push_str("MANIFEST.widgets: mini-pages for the project dashboard. They are compact and read/show data.\n");
    p.push_str("  {\n");
    p.push_str("    \"widgets\": [{\n");
    p.push_str("      \"id\": \"today\",\n");
    p.push_str("      \"name\": \"Today\",\n");
    p.push_str("      \"entry\": \"widgets/today.html\",\n");
    p.push_str("      \"size\": \"small\",         // small (1x1), medium (2x1), wide (3x1), large (2x2). Base cell is about 180px.\n");
    p.push_str("      \"description\": \"what the widget shows\"\n");
    p.push_str("    }]\n");
    p.push_str("  }\n");
    p.push_str("- Each widget.entry is a separate HTML file in the app directory, usually `widgets/<id>.html`.\n");
    p.push_str("- You can create/update a widget in one call: reflexWidgetsUpsert({id:\"today\", name:\"Today\", size:\"small\", html:\"<html>...</html>\"}); by default, entry becomes widgets/<id>.html.\n");
    p.push_str("- Widgets have access to the same bridge and runtime overlay (reflexInvoke, reflexBridgeCatalog, reflexSystemContext, reflexSystemOpenPanel, reflexSystemOpenUrl/OpenPath/RevealPath, reflexLog/LogList, reflexManifestGet/Update, reflexIntegrationCatalog/Profile/Update, reflexPermissions*, reflexNetwork*, reflexWidgets*, reflexActions*, reflexCapabilities, reflexAgent*, reflexStorage*, reflexFs*, reflexClipboard*, reflexNetFetch, reflexDialog*, reflexNotifyShow, reflexProjectsList/Open, reflexProjectProfileUpdate, reflexProjectSandboxSet, reflexProjectAppsLink/Unlink, reflexTopicsList/Open, reflexSkillsList, reflexProjectSkillsEnsure/Revoke, reflexMcpServers, reflexProjectMcpUpsert/Delete, reflexProjectFilesList/Read/Search/Write/Mkdir/Move/Copy/Delete, reflexProjectBrowserSetEnabled, reflexBrowser*, reflexScheduler*, reflexMemory*, reflexEventOn/Off/Emit/Recent/Subscriptions/ClearSubscriptions, reflexAppsList/Create/Export/Import/Delete/TrashList/Restore/Purge/Open/Invoke/ListActions).\n");
    p.push_str("- Keep widgets compact: dark transparent background, background:transparent, html/body height 100%, padding 12-14px, and no own frame because the dashboard grid draws it.\n");
    p.push_str("- If data updates often, add setInterval yourself with a 5-30 second interval.\n");
    p.push_str("- If the widget reads data from another utility, use reflexAppsInvoke('<app>','<action>',{...}); do NOT duplicate data collection.\n\n");

    p.push_str("INTER-APP EVENTS AND CALLS:\n");
    p.push_str("  events.emit({topic, payload}); publish an event to subscribers\n");
    p.push_str("  events.subscribe({topics: [\"...\"]}); subscribe. \"*\" means any topic\n");
    p.push_str("  events.unsubscribe({topics: [...]})\n");
    p.push_str("  events.subscriptions() -> {topics}; events.recent({topic?, limit?}) -> {events}; recent events for this app or subscribed topics\n");
    p.push_str("  events.clearSubscriptions(); clear all subscriptions for this app\n");
    p.push_str("  apps.list() -> AppSummary[]; safe installed app catalog without raw manifest/server command/steps\n");
    p.push_str("  apps.create({description, template?, projectId?}) -> {app_id, thread_id, project_id}; create a new Reflex app through the agent generator. Requires permission \"apps.create\" or \"apps:*\"; projectId also requires \"projects.write:<project>\" or \"projects.write:*\"\n");
    p.push_str("  apps.export({app_id, targetPath}) -> {ok, app_id, path}; apps.import({zipPath}) -> {ok, app}; .reflexapp bundles. Requires \"apps.manage\" or \"apps:*\". Before export, choose a path through dialog.saveFile.\n");
    p.push_str("  apps.delete({app_id}) -> TrashEntry; apps.trashList() -> TrashEntry[]; apps.restore({trash_id}) -> {ok, app_id}; apps.purge({trash_id}) -> {ok}; trash lifecycle. Requires \"apps.manage\" or \"apps:*\". delete cannot delete the current app.\n");
    p.push_str("  apps.status({app_id}) -> {has_changes, revision, last_commit_message, entry_exists}; revision/dirty state. Requires \"apps.manage\" or \"apps:*\"\n");
    p.push_str("  apps.diff({app_id}) -> {app_id, diff}; apps.commit({app_id, message?}) -> {ok}; apps.commitPartial({app_id, patch, message?}) -> {ok}; apps.revert({app_id}) -> {ok}; revision controls. Requires \"apps.manage\" or \"apps:*\"\n");
    p.push_str("  apps.server.status({app_id}); apps.server.logs({app_id}); apps.server.start({app_id}); apps.server.stop({app_id}); apps.server.restart({app_id}); server-runtime app operations. Requires \"apps.manage\" or \"apps:*\". start is idempotent, restart returns a new port.\n");
    p.push_str("  apps.open({app_id}) -> {ok}; ask Reflex to open another app in the main UI\n");
    p.push_str("  apps.invoke({app_id, action_id, params}) -> {ok, run_id, result}\n");
    p.push_str("  apps.list_actions({app_id?, include_steps?}); list callable actions\n");
    p.push_str("The iframe runtime overlay already provides helpers; call them directly without postMessage:\n");
    p.push_str("  window.reflexEventOn(topic, (data, fromApp) => {...})    // subscribes and stores handler\n");
    p.push_str("  window.reflexEventOff(topic)\n");
    p.push_str("  window.reflexEventEmit(topic, payload)\n");
    p.push_str("  window.reflexEventRecent(topicOrParams?, limit?)\n");
    p.push_str("  window.reflexEventSubscriptions()\n");
    p.push_str("  window.reflexEventClearSubscriptions()\n");
    p.push_str("  window.reflexInvoke(method, params)                      // generic bridge call\n");
    p.push_str("  window.reflexBridgeCatalog()                             // methods/helpers/permission hints/current grants\n");
    p.push_str("  window.reflexSystemContext()\n");
    p.push_str("  window.reflexSystemOpenPanel(panelOrParams, projectId?, threadId?)\n");
    p.push_str("  window.reflexSystemOpenUrl(urlOrParams), reflexSystemOpenPath(pathOrParams), reflexSystemRevealPath(pathOrParams)\n");
    p.push_str("  window.reflexLog(levelOrParams, message?), reflexLogList(params)\n");
    p.push_str("  window.reflexManifestGet(), reflexManifestUpdate(patch), reflexIntegrationCatalog(providerOrParams?), reflexIntegrationProfile(), reflexIntegrationUpdate(patchOrParams, external?), reflexPermissionsList(), reflexPermissionsEnsure(permissionOrParams), reflexPermissionsRevoke(permissionOrParams), reflexNetworkHosts(), reflexNetworkAllowHost(hostOrParams), reflexNetworkRevokeHost(hostOrParams), reflexWidgetsList(), reflexWidgetsUpsert(widgetOrParams), reflexWidgetsDelete(widgetIdOrParams, deleteEntry?), reflexActionsList(), reflexActionsUpsert(actionOrParams), reflexActionsDelete(actionIdOrParams), reflexCapabilities() // manifest summary + hasPermission()/hasNetworkHost()\n");
    p.push_str("  window.reflexAgentAsk(promptOrParams), reflexAgentStartTopic(promptOrParams, projectId?), reflexAgentTask(promptOrParams), reflexAgentStream(promptOrParams), reflexAgentStreamAbort(threadIdOrParams)\n");
    p.push_str("  window.reflexStorageGet(keyOrParams), reflexStorageSet(keyOrParams, value?), reflexStorageList(params), reflexStorageDelete(keyOrParams)\n");
    p.push_str("  window.reflexFsRead(pathOrParams), reflexFsList(pathOrParams, recursive?), reflexFsWrite(pathOrParams, content?), reflexFsDelete(pathOrParams, recursive?)\n");
    p.push_str("  window.reflexClipboardReadText(), reflexClipboardWriteText(textOrParams)\n");
    p.push_str("  window.reflexNetFetch(urlOrParams, options?), reflexNotifyShow(titleOrParams, body?)\n");
    p.push_str("  window.reflexDialogOpenDirectory(params), reflexDialogOpenFile(params), reflexDialogSaveFile(params)\n");
    p.push_str("  window.reflexProjectsList(params), reflexProjectsOpen(projectIdOrParams), reflexProjectProfileUpdate(patch), reflexProjectSandboxSet(sandboxOrParams), reflexProjectAppsLink(appIdOrParams?), reflexProjectAppsUnlink(appIdOrParams?), reflexTopicsList(params), reflexTopicsOpen(threadIdOrParams, projectId?), reflexSkillsList(params), reflexProjectSkillsEnsure(skillOrParams), reflexProjectSkillsRevoke(skillOrParams), reflexMcpServers(params), reflexProjectMcpUpsert(nameOrParams, config?), reflexProjectMcpDelete(nameOrParams), reflexProjectFilesList(pathOrParams, recursive?), reflexProjectFilesRead(pathOrParams), reflexProjectFilesSearch(queryOrParams, includeContent?), reflexProjectFilesWrite(pathOrParams, content?), reflexProjectFilesMkdir(pathOrParams), reflexProjectFilesMove(fromOrParams, to?), reflexProjectFilesCopy(fromOrParams, to?), reflexProjectFilesDelete(pathOrParams, recursive?)\n");
    p.push_str("  window.reflexBrowserInit(params), reflexProjectBrowserSetEnabled(projectIdOrParams, enabled?), reflexBrowserTabs(), reflexBrowserOpen(url), reflexBrowserClose(tabIdOrParams), reflexBrowserSetActive(tabIdOrParams), reflexBrowserNavigate(tabId, url)\n");
    p.push_str("  window.reflexBrowserBack(tabIdOrParams), reflexBrowserForward(tabIdOrParams), reflexBrowserReload(tabIdOrParams), reflexBrowserCurrentUrl(tabIdOrParams), reflexBrowserReadText(tabId), reflexBrowserReadOutline(tabId), reflexBrowserScreenshot(tabIdOrParams, fullPage?)\n");
    p.push_str("  window.reflexBrowserClickText(tabIdOrParams, text?, exact?), reflexBrowserClickSelector(tabIdOrParams, selector?), reflexBrowserFill(tabIdOrParams, selector?, value?), reflexBrowserScroll(tabIdOrParams, dx?, dy?), reflexBrowserWaitFor(tabIdOrParams, selector?, timeoutMs?)\n");
    p.push_str("  window.reflexSchedulerList(params), reflexSchedulerUpsert(scheduleOrParams), reflexSchedulerDelete(scheduleIdOrParams), reflexSchedulerRunNow(scheduleId), reflexSchedulerSetPaused(scheduleId, paused), reflexSchedulerRuns(params), reflexSchedulerStats(params), reflexSchedulerRunDetail(runIdOrParams)\n");
    p.push_str("  window.reflexMemorySave(params), reflexMemoryRead(relPathOrParams), reflexMemoryUpdate(relPathOrParams, patch?), reflexMemoryList(params), reflexMemoryDelete(relPathOrParams)\n");
    p.push_str("  window.reflexMemorySearch(queryOrParams), reflexMemoryRecall(queryOrParams), reflexMemoryStats(params), reflexMemoryReindex(params)\n");
    p.push_str("  window.reflexMemoryIndexPath(pathOrParams), reflexMemoryPathStatus(pathOrParams), reflexMemoryPathStatusBatch(pathsOrParams), reflexMemoryForgetPath(pathOrParams)\n");
    p.push_str("  window.reflexAppsList(params), reflexAppsCreate(descriptionOrParams, template?), reflexAppsExport(appIdOrParams, targetPath?), reflexAppsImport(zipPathOrParams), reflexAppsDelete(appIdOrParams), reflexAppsTrashList(), reflexAppsRestore(trashIdOrParams), reflexAppsPurge(trashIdOrParams), reflexAppsStatus(appIdOrParams), reflexAppsDiff(appIdOrParams), reflexAppsCommit(appIdOrParams, message?), reflexAppsCommitPartial(appIdOrParams, patch?, message?), reflexAppsRevert(appIdOrParams), reflexAppsServerStatus(appIdOrParams), reflexAppsServerLogs(appIdOrParams), reflexAppsServerStart(appIdOrParams), reflexAppsServerStop(appIdOrParams), reflexAppsServerRestart(appIdOrParams), reflexAppsOpen(appIdOrParams), reflexAppsInvoke(appId, actionId, params), reflexAppsListActions(appIdOrParams, includeSteps?)\n");
    p.push_str("Permissions for apps.invoke/apps.create/apps.manage are declared in manifest.permissions:\n");
    p.push_str("  [\"apps.create\"]                         -- create new Reflex apps through apps.create\n");
    p.push_str("  [\"apps.manage\"]                         -- export/import/delete/restore/purge/list trash, revision controls, and server runtime ops for installed apps\n");
    p.push_str("  [\"apps.invoke:*\"]                       -- call ANY action in ANY app\n");
    p.push_str("  [\"apps.invoke:health-stats\"]            -- only a specific app\n");
    p.push_str("  [\"apps.invoke:health-stats::today\"]     -- only a specific action\n");
    p.push_str("- If data already exists in another app, do NOT duplicate collection. Call its action through apps.invoke or listen to its events.\n\n");

    p.push_str("LIMITATIONS:\n");
    p.push_str("- iframe sandbox=\"allow-scripts allow-forms\"; server runtime also gets allow-same-origin. Arbitrary external fetch may not work from the iframe, so use agent.ask or your own server runtime for dynamic data.\n");
    p.push_str("- schedule.steps cannot use dialog.*, clipboard.*, system.openPanel/openUrl/openPath/revealPath, apps.create/import/commit/commitPartial/delete/restore/revert/purge, apps.open, projects.open, or topics.open because these steps run without UI. apps.diff/status/server.status/server.logs/export are allowed for monitoring/backup automations when targetPath is explicit.\n\n");
    if let Some(skeleton) = template_skeleton(template) {
        p.push_str("TEMPLATE:\n");
        p.push_str(skeleton);
        p.push('\n');
    }
    p.push_str("TASK: ");
    p.push_str(description);
    p.push_str("\n\nAt the end, leave working files and an updated manifest.json. Do not touch .reflex/.\n");
    p
}

pub(crate) async fn ask_agent_oneshot(app: &AppHandle, prompt: &str) -> std::io::Result<String> {
    use std::process::Stdio;
    use tokio::process::Command as TokioCommand;
    let base = app
        .path()
        .app_data_dir()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
    let scratch = base.join("scratch");
    std::fs::create_dir_all(&scratch)?;
    let out_path = scratch.join(format!("oneshot-{}.txt", uuid_like()));
    let cwd_str = scratch.to_string_lossy().into_owned();
    let out_str = out_path.to_string_lossy().into_owned();

    let result = TokioCommand::new("codex")
        .args([
            "exec",
            "--json",
            "--skip-git-repo-check",
            "-s",
            "read-only",
            "--output-last-message",
            &out_str,
            "-C",
            &cwd_str,
            "--",
            prompt,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .output()
        .await?;
    if !result.status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("codex exit: {}", result.status),
        ));
    }
    let text = std::fs::read_to_string(&out_path)?;
    let _ = std::fs::remove_file(&out_path);
    Ok(text.trim().to_string())
}

pub(crate) fn uuid_like() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{now}")
}

#[tauri::command]
fn create_project(
    app: AppHandle,
    root: String,
    name: Option<String>,
    description: Option<String>,
) -> Result<project::Project, String> {
    let path = PathBuf::from(&root);
    if !path.is_dir() {
        return Err(format!("not a directory: {root}"));
    }
    project::create_project(&app, &path, name, description).map_err(|e| e.to_string())
}

#[tauri::command]
fn update_project_description(
    app: AppHandle,
    project_id: String,
    description: Option<String>,
) -> Result<project::Project, String> {
    let mut p = project::get_by_id(&app, &project_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("project not found: {project_id}"))?;
    p.description = description
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    project::write_project(&PathBuf::from(&p.root), &p).map_err(|e| e.to_string())?;
    project::register(&app, &p).map_err(|e| e.to_string())?;
    Ok(p)
}

#[tauri::command]
fn update_project_agent_profile(
    app: AppHandle,
    project_id: String,
    agent_instructions: Option<String>,
    skills: Vec<String>,
) -> Result<project::Project, String> {
    let mut p = project::get_by_id(&app, &project_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("project not found: {project_id}"))?;
    p.agent_instructions = agent_instructions
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let mut seen = std::collections::HashSet::new();
    p.skills = skills
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .filter(|s| seen.insert(s.to_ascii_lowercase()))
        .collect();

    project::write_project(&PathBuf::from(&p.root), &p).map_err(|e| e.to_string())?;
    project::register(&app, &p).map_err(|e| e.to_string())?;
    Ok(p)
}

fn link_app_to_project_inner(
    app: &AppHandle,
    project_id: &str,
    app_id: &str,
) -> Result<project::Project, String> {
    apps::read_manifest(app, app_id)
        .map_err(|e| format!("app not found or unreadable: {app_id}: {e}"))?;
    let mut p = project::get_by_id(app, project_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("project not found: {project_id}"))?;
    if !p.apps.iter().any(|id| id == app_id) {
        p.apps.push(app_id.to_string());
    }
    project::write_project(&PathBuf::from(&p.root), &p).map_err(|e| e.to_string())?;
    project::register(app, &p).map_err(|e| e.to_string())?;
    Ok(p)
}

#[tauri::command]
fn link_app_to_project(
    app: AppHandle,
    project_id: String,
    app_id: String,
) -> Result<project::Project, String> {
    link_app_to_project_inner(&app, &project_id, &app_id)
}

#[tauri::command]
fn unlink_app_from_project(
    app: AppHandle,
    project_id: String,
    app_id: String,
) -> Result<project::Project, String> {
    let mut p = project::get_by_id(&app, &project_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("project not found: {project_id}"))?;
    p.apps.retain(|a| a != &app_id);
    project::write_project(&PathBuf::from(&p.root), &p).map_err(|e| e.to_string())?;
    project::register(&app, &p).map_err(|e| e.to_string())?;
    Ok(p)
}

#[tauri::command]
fn find_project_for_path(path: String) -> Option<project::Project> {
    project::find_project_for(&PathBuf::from(path))
}

#[tauri::command]
fn update_project_sandbox(
    app: AppHandle,
    project_id: String,
    sandbox: String,
) -> Result<project::Project, String> {
    let mut p = project::get_by_id(&app, &project_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("project not found: {project_id}"))?;
    if !["read-only", "workspace-write", "danger-full-access"].contains(&sandbox.as_str()) {
        return Err(format!("invalid sandbox: {sandbox}"));
    }
    p.sandbox = sandbox;
    project::write_project(&PathBuf::from(&p.root), &p).map_err(|e| e.to_string())?;
    project::register(&app, &p).map_err(|e| e.to_string())?;
    Ok(p)
}

#[tauri::command]
fn update_project_browser(
    app: AppHandle,
    project_id: String,
    enabled: bool,
) -> Result<project::Project, String> {
    let mut p = project::get_by_id(&app, &project_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("project not found: {project_id}"))?;
    let mut servers = p
        .mcp_servers
        .clone()
        .unwrap_or_else(|| serde_json::json!({}));
    if let Some(obj) = servers.as_object_mut() {
        if enabled {
            let bridge = browser::mcp_bridge_path(&app)
                .map_err(|e| format!("bridge path: {e}"))?;
            let node = browser::resolve_node()
                .unwrap_or_else(|_| "node".to_string());
            obj.insert(
                "reflex_browser".to_string(),
                serde_json::json!({
                    "command": node,
                    "args": [bridge.to_string_lossy()],
                }),
            );
            obj.remove("playwright");
        } else {
            obj.remove("reflex_browser");
            obj.remove("playwright");
        }
    }
    p.mcp_servers = if servers
        .as_object()
        .map(|o| o.is_empty())
        .unwrap_or(true)
    {
        None
    } else {
        Some(servers)
    };
    project::write_project(&PathBuf::from(&p.root), &p).map_err(|e| e.to_string())?;
    project::register(&app, &p).map_err(|e| e.to_string())?;
    Ok(p)
}

#[tauri::command]
fn update_project_mcp_servers(
    app: AppHandle,
    project_id: String,
    mcp_servers: Option<serde_json::Value>,
) -> Result<project::Project, String> {
    let mut p = project::get_by_id(&app, &project_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("project not found: {project_id}"))?;

    let next = match mcp_servers {
        Some(serde_json::Value::Object(obj)) if !obj.is_empty() => {
            Some(serde_json::Value::Object(obj))
        }
        Some(serde_json::Value::Object(_)) | Some(serde_json::Value::Null) | None => None,
        Some(_) => return Err("mcp_servers must be a JSON object or null".into()),
    };

    p.mcp_servers = next;
    project::write_project(&PathBuf::from(&p.root), &p).map_err(|e| e.to_string())?;
    project::register(&app, &p).map_err(|e| e.to_string())?;
    Ok(p)
}

#[derive(Serialize)]
struct DirEntry {
    name: String,
    path: String,
    kind: &'static str,
    size: Option<u64>,
    modified_ms: Option<u128>,
    is_hidden: bool,
}

#[derive(Serialize)]
struct ProjectFileEntry {
    name: String,
    path: String,
    relative_path: String,
    kind: &'static str,
    size: Option<u64>,
    modified_ms: Option<u128>,
}

#[tauri::command]
fn reveal_in_finder(path: String) -> Result<(), String> {
    std::process::Command::new("open")
        .arg(&path)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn list_directory(path: String) -> Result<Vec<DirEntry>, String> {
    let p = PathBuf::from(&path);
    if !p.is_dir() {
        return Err(format!("not a directory: {path}"));
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&p).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let file_type = metadata.file_type();
        let kind: &'static str = if file_type.is_symlink() {
            "symlink"
        } else if file_type.is_dir() {
            "directory"
        } else {
            "file"
        };
        let modified_ms = metadata.modified().ok().and_then(|m| {
            m.duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_millis())
        });
        let size = if file_type.is_file() {
            Some(metadata.len())
        } else {
            None
        };
        let is_hidden = name.starts_with('.');
        out.push(DirEntry {
            name,
            path: entry.path().to_string_lossy().into_owned(),
            kind,
            size,
            modified_ms,
            is_hidden,
        });
    }
    out.sort_by(|a, b| {
        let dir_a = a.kind == "directory";
        let dir_b = b.kind == "directory";
        if dir_a != dir_b {
            return if dir_a {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            };
        }
        a.name.to_lowercase().cmp(&b.name.to_lowercase())
    });
    Ok(out)
}

#[tauri::command]
fn list_project_files(
    project_root: String,
    query: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<ProjectFileEntry>, String> {
    const SKIP_DIRS: &[&str] = &[
        ".git",
        ".reflex",
        "node_modules",
        "target",
        "dist",
        "build",
        ".next",
        ".turbo",
        ".venv",
        "venv",
        "__pycache__",
    ];

    fn visit(
        root: &Path,
        dir: &Path,
        query: &str,
        out: &mut Vec<ProjectFileEntry>,
        limit: usize,
        depth: usize,
    ) -> Result<(), String> {
        if out.len() >= limit || depth > 8 {
            return Ok(());
        }
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(_) => return Ok(()),
        };
        for entry in entries {
            if out.len() >= limit {
                break;
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') && name != ".env" {
                continue;
            }
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            let file_type = metadata.file_type();
            let kind: &'static str = if file_type.is_symlink() {
                "symlink"
            } else if file_type.is_dir() {
                "directory"
            } else {
                "file"
            };
            if kind == "directory" && SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            let path = entry.path();
            let relative_path = match path.strip_prefix(root) {
                Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
                Err(_) => continue,
            };
            let haystack = format!("{name} {relative_path}").to_lowercase();
            if query.is_empty() || haystack.contains(query) {
                let modified_ms = metadata.modified().ok().and_then(|m| {
                    m.duration_since(std::time::UNIX_EPOCH)
                        .ok()
                        .map(|d| d.as_millis())
                });
                let size = if file_type.is_file() {
                    Some(metadata.len())
                } else {
                    None
                };
                out.push(ProjectFileEntry {
                    name,
                    path: path.to_string_lossy().into_owned(),
                    relative_path,
                    kind,
                    size,
                    modified_ms,
                });
            }
            if kind == "directory" {
                visit(root, &path, query, out, limit, depth + 1)?;
            }
        }
        Ok(())
    }

    let root = PathBuf::from(&project_root);
    if !root.is_dir() {
        return Err(format!("not a directory: {project_root}"));
    }
    let normalized_query = query.unwrap_or_default().trim().to_lowercase();
    let limit = limit.unwrap_or(120).clamp(1, 300);
    let mut out = Vec::new();
    visit(&root, &root, &normalized_query, &mut out, limit, 0)?;
    out.sort_by(|a, b| {
        let dir_a = a.kind == "directory";
        let dir_b = b.kind == "directory";
        if dir_a != dir_b {
            return if dir_a {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            };
        }
        a.relative_path
            .to_lowercase()
            .cmp(&b.relative_path.to_lowercase())
    });
    Ok(out)
}

#[tauri::command]
fn list_threads(app: AppHandle) -> Result<Vec<ProjectThread>, String> {
    let apps_root = apps::apps_dir(&app)
        .ok()
        .and_then(|p| p.canonicalize().ok());
    let projects = project::list_registered(&app).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for p in projects {
        if let Some(root) = &apps_root {
            if let Ok(c) = std::path::PathBuf::from(&p.root).canonicalize() {
                if c.starts_with(root) {
                    continue;
                }
            }
        }
        let root = PathBuf::from(&p.root);
        let threads = match storage::read_all_threads(&root) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("[reflex] read_all_threads({}): {e}", p.root);
                continue;
            }
        };
        for t in threads {
            out.push(ProjectThread {
                project: p.clone(),
                thread: t,
            });
        }
    }
    out.sort_by_key(|pt| pt.thread.meta.created_at_ms);
    Ok(out)
}

#[tauri::command]
fn respond_to_question(
    app: AppHandle,
    question_id: String,
    decision: String,
    text: Option<String>,
) -> Result<(), String> {
    eprintln!(
        "[reflex] respond_to_question: q={question_id} decision={decision} text_len={}",
        text.as_deref().map(|s| s.len()).unwrap_or(0)
    );
    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let handle = app_handle.state::<app_server::AppServerHandle>();
        let server = handle.wait().await;
        let q = match server.take_question(&question_id) {
            Some(q) => q,
            None => {
                eprintln!("[reflex] respond: question {question_id} not pending");
                return;
            }
        };
        let result = build_response(&q.method, &decision, text.as_deref());
        if let Err(e) = server.send_response(q.request_id, result).await {
            eprintln!("[reflex] send_response err: {e}");
        }
    });
    Ok(())
}

fn build_response(method: &str, decision: &str, text: Option<&str>) -> serde_json::Value {
    let normalized = match decision {
        "approve" | "approved" => "approved",
        "approve_for_session" | "approved_for_session" => "approved_for_session",
        "deny" | "denied" => "denied",
        "abort" => "abort",
        other => other,
    };
    match method {
        // legacy v1 approvals
        "applyPatchApproval" | "execCommandApproval" => serde_json::json!({
            "decision": normalized,
        }),
        // v2 named approvals
        "item/commandExecution/requestApproval"
        | "item/fileChange/requestApproval"
        | "item/permissions/requestApproval" => serde_json::json!({
            "decision": normalized,
        }),
        // free-form text input
        "item/tool/requestUserInput" | "mcpServer/elicitation/request" => serde_json::json!({
            "answer": text.unwrap_or(""),
        }),
        _ => serde_json::json!({
            "decision": normalized,
            "answer": text.unwrap_or(""),
        }),
    }
}

#[tauri::command]
fn stop_thread(app: AppHandle, thread_id: String) -> Result<(), String> {
    eprintln!("[reflex] stop_thread: thread={thread_id}");
    let app_handle = app.clone();
    let id = thread_id.clone();
    tauri::async_runtime::spawn(async move {
        let handle = app_handle.state::<app_server::AppServerHandle>();
        let server = handle.wait().await;
        match server.current_turn_id(&id) {
            Some((app_thread_id, turn_id)) => {
                if let Err(e) = server.turn_interrupt(&app_thread_id, &turn_id).await {
                    eprintln!("[reflex] turn_interrupt err: {e}");
                }
            }
            None => {
                eprintln!("[reflex] stop_thread: no active turn for {id}");
            }
        }
    });
    Ok(())
}

fn candidate_root(ctx: &QuickContext) -> Option<PathBuf> {
    if let Some(target) = &ctx.finder_target {
        let path = PathBuf::from(target);
        if path.is_dir() {
            return Some(path);
        }
        if let Some(parent) = path.parent() {
            return Some(parent.to_path_buf());
        }
    }
    None
}

fn resolve_project(
    app: &AppHandle,
    project_id: Option<&str>,
    ctx: &QuickContext,
) -> Result<project::Project, String> {
    if let Some(id) = project_id {
        return project::get_by_id(app, id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("project not found: {id}"));
    }
    if let Some(root) = candidate_root(ctx) {
        if let Some(p) = project::find_project_for(&root) {
            return Ok(p);
        }
    }
    Err("no project resolved (provide project_id or open Finder in a Reflex project)".into())
}

fn memory_kick_topic(app: &AppHandle, project_root: &Path, thread_id: &str) {
    use crate::memory::agents::envelope::{intents, Envelope};
    use crate::memory::agents::indexer::{self, IndexerConfig};

    let state = app.state::<memory::MemoryState>();
    let bus = state.bus.clone();
    let indexed = state.indexed_threads.clone();

    let id = thread_id.to_string();
    let root = project_root.to_path_buf();
    let bus_for_indexer = bus.clone();

    tauri::async_runtime::spawn(async move {
        let mut guard = indexed.lock().await;
        let already = !guard.insert(id.clone());
        drop(guard);
        if !already {
            let bus_arg = bus_for_indexer.clone();
            let id_arg = id.clone();
            let root_arg = root.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) =
                    indexer::run(bus_arg, root_arg, id_arg.clone(), IndexerConfig::default()).await
                {
                    eprintln!("[reflex] indexer for {id_arg} exited: {e}");
                }
            });
        }
        let env = Envelope::new(
            "core",
            &format!("indexer:{id}"),
            intents::TOPIC_TURN,
            serde_json::json!({ "thread_id": id }),
        );
        if let Err(e) = bus.send(env).await {
            eprintln!("[reflex] bus send TOPIC_TURN failed: {e}");
        }
    });
}

#[tauri::command]
fn submit_quick(
    app: AppHandle,
    prompt: String,
    ctx: QuickContext,
    project_id: Option<String>,
    plan_mode: Option<bool>,
    source: Option<String>,
    browser_tabs: Option<Vec<storage::BrowserTab>>,
    image_paths: Option<Vec<String>>,
    goal: Option<String>,
) -> Result<String, String> {
    submit_quick_impl(
        app,
        prompt,
        ctx,
        project_id,
        plan_mode,
        source,
        browser_tabs,
        image_paths,
        goal,
    )
}

pub(crate) fn submit_quick_impl(
    app: AppHandle,
    prompt: String,
    ctx: QuickContext,
    project_id: Option<String>,
    plan_mode: Option<bool>,
    source: Option<String>,
    browser_tabs: Option<Vec<storage::BrowserTab>>,
    image_paths: Option<Vec<String>>,
    goal: Option<String>,
) -> Result<String, String> {
    let plan_mode = plan_mode.unwrap_or(false);
    let source = source
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "quick".into());
    let browser_tabs = browser_tabs.unwrap_or_default();
    let image_paths = validate_local_image_paths(image_paths)?;
    let goal = goal
        .map(|g| g.trim().to_string())
        .filter(|g| !g.is_empty());
    eprintln!(
        "[reflex] submit_quick: prompt_len={} project_id={:?} source={} tabs={} ctx={:?}/{:?}",
        prompt.len(),
        project_id,
        source,
        browser_tabs.len(),
        ctx.frontmost_app,
        ctx.finder_target
    );

    let project = resolve_project(&app, project_id.as_deref(), &ctx)?;
    let project_root = PathBuf::from(&project.root);

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis();
    let thread_id = format!("t_{now_ms}");
    eprintln!(
        "[reflex] thread_id={thread_id} project={} root={}",
        project.name, project.root
    );

    let meta = storage::ThreadMeta {
        id: thread_id.clone(),
        project_id: Some(project.id.clone()),
        prompt: prompt.clone(),
        cwd: project.root.clone(),
        frontmost_app: ctx.frontmost_app.clone(),
        finder_target: ctx.finder_target.clone(),
        created_at_ms: now_ms,
        exit_code: None,
        done: false,
        session_id: None,
        title: None,
        goal: goal.clone(),
        plan_mode,
        plan_confirmed: false,
        source: source.clone(),
        browser_tabs: browser_tabs.clone(),
    };
    if let Err(e) = storage::write_meta(&project_root, &meta) {
        eprintln!("[reflex] write_meta failed: {e}");
    }

    if let Some(quick) = app.get_webview_window(QUICK_WINDOW) {
        let _ = quick.hide();
    }
    if let Some(main) = app.get_webview_window(MAIN_WINDOW) {
        let _ = main.show();
        let _ = main.unminimize();
        let _ = main.set_focus();
    }

    let payload = ThreadCreated {
        id: thread_id.clone(),
        project_id: project.id.clone(),
        project_name: project.name.clone(),
        prompt: prompt.clone(),
        cwd: project.root.clone(),
        ctx,
        created_at_ms: now_ms,
        goal: goal.clone(),
        plan_mode,
        source: source.clone(),
        browser_tabs: browser_tabs.clone(),
    };
    if let Err(e) = app.emit(THREAD_CREATED_EVENT, &payload) {
        eprintln!("[reflex] emit thread-created failed: {e}");
    }

    memory_kick_topic(&app, &project_root, &thread_id);

    {
        let app_meta = app.clone();
        let root_meta = project_root.clone();
        let id_meta = thread_id.clone();
        let prompt_meta = prompt.clone();
        tauri::async_runtime::spawn(async move {
            codex::generate_topic_meta(app_meta, root_meta, id_meta, prompt_meta).await;
        });
    }

    let prompt_with_browser = if source == "browser" && !browser_tabs.is_empty() {
        let mut buf = String::from("Context from the built-in browser, open tabs at launch time:\n");
        for (i, tab) in browser_tabs.iter().enumerate() {
            let title = if tab.title.trim().is_empty() {
                "(untitled)"
            } else {
                tab.title.trim()
            };
            buf.push_str(&format!("{}. {} — {}\n", i + 1, title, tab.url));
        }
        buf.push_str("\nTASK:\n");
        buf.push_str(&prompt);
        buf
    } else {
        prompt.clone()
    };
    let codex_prompt = if plan_mode {
        wrap_with_plan_mode(&prompt_with_browser)
    } else {
        prompt_with_browser
    };
    let app_handle = app.clone();
    let reflex_id = thread_id.clone();
    let root_for_task = project_root.clone();
    let project_id_for_task = project.id.clone();
    let image_paths_for_task = image_paths.clone();
    tauri::async_runtime::spawn(async move {
        let project_now = project::get_by_id(&app_handle, &project_id_for_task)
            .ok()
            .flatten();
        let profile = project_now
            .as_ref()
            .map(project_agent_profile_preface)
            .unwrap_or_default();
        let codex_prompt = match crate::memory::injection::build_preface(
            &root_for_task,
            &reflex_id,
            &codex_prompt,
        )
        .await
        {
            Ok(r) => crate::memory::injection::wrap_user_prompt(&r.preface, &codex_prompt),
            Err(e) => {
                eprintln!("[reflex] memory inject failed: {e}");
                codex_prompt
            }
        };
        let codex_prompt = wrap_with_project_agent_profile(&profile, &codex_prompt);
        let handle = app_handle.state::<app_server::AppServerHandle>();
        let server = handle.wait().await;
        let sandbox = project_now
            .as_ref()
            .map(|p| p.sandbox.clone())
            .unwrap_or_else(|| "workspace-write".into());
        let mcp_servers = project_now.as_ref().and_then(|p| p.mcp_servers.clone());
        let app_thread_id = match server
            .thread_start(&root_for_task, &sandbox, mcp_servers.as_ref())
            .await
        {
            Ok(id) => id,
            Err(e) => {
                eprintln!("[reflex] thread_start failed: {e}");
                let _ = app_handle.emit(
                    "reflex://codex-event",
                    &serde_json::json!({
                        "thread_id": reflex_id,
                        "seq": 0,
                        "raw": format!("[reflex] thread_start failed: {e}"),
                        "stream": "error",
                    }),
                );
                let _ = storage::finalize_thread(&root_for_task, &reflex_id, Some(-1), None);
                let _ = app_handle.emit(
                    "reflex://codex-end",
                    &serde_json::json!({"thread_id": reflex_id, "exit_code": -1}),
                );
                return;
            }
        };
        eprintln!("[reflex] app_thread_id={app_thread_id} reflex={reflex_id}");
        server.register_thread(
            app_thread_id.clone(),
            reflex_id.clone(),
            root_for_task.clone(),
            0,
        );
        if let Ok(mut meta) = storage::read_meta(&root_for_task, &reflex_id) {
            meta.session_id = Some(app_thread_id.clone());
            let _ = storage::write_meta(&root_for_task, &meta);
        }
        if let Err(e) = server
            .turn_start_with_local_images(&app_thread_id, &codex_prompt, &image_paths_for_task)
            .await
        {
            eprintln!("[reflex] turn_start failed: {e}");
        }
    });

    Ok(thread_id)
}

fn validate_local_image_paths(image_paths: Option<Vec<String>>) -> Result<Vec<String>, String> {
    let Some(paths) = image_paths else {
        return Ok(Vec::new());
    };
    if paths.len() > 8 {
        return Err("too many image attachments; maximum is 8".into());
    }

    const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "heic"];
    let mut out = Vec::new();
    for path in paths {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            continue;
        }
        let path_buf = PathBuf::from(trimmed);
        if !path_buf.is_file() {
            return Err(format!("image attachment is not a file: {trimmed}"));
        }
        let ext = path_buf
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !IMAGE_EXTS.contains(&ext.as_str()) {
            return Err(format!("unsupported image attachment type: {trimmed}"));
        }
        out.push(trimmed.to_string());
    }
    Ok(out)
}

#[tauri::command]
fn set_thread_goal(
    app: AppHandle,
    project_id: String,
    thread_id: String,
    goal: Option<String>,
) -> Result<(), String> {
    let project = project::get_by_id(&app, &project_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("project not found: {project_id}"))?;
    let project_root = PathBuf::from(&project.root);
    let normalized = goal
        .map(|g| g.trim().to_string())
        .filter(|g| !g.is_empty());
    let mut meta = storage::read_meta(&project_root, &thread_id).map_err(|e| e.to_string())?;
    meta.goal = normalized.clone();
    storage::write_meta(&project_root, &meta).map_err(|e| e.to_string())?;
    let _ = app.emit(
        "reflex://thread-meta-updated",
        &serde_json::json!({
            "thread_id": thread_id,
            "goal": normalized,
        }),
    );
    Ok(())
}

#[tauri::command]
fn continue_thread(
    app: AppHandle,
    project_id: String,
    thread_id: String,
    prompt: String,
    plan_confirmed: Option<bool>,
    image_paths: Option<Vec<String>>,
) -> Result<(), String> {
    eprintln!(
        "[reflex] continue_thread: project={project_id} thread={thread_id} prompt_len={}",
        prompt.len()
    );
    let trimmed = prompt.trim();
    if trimmed.is_empty() {
        return Err("empty prompt".into());
    }
    let image_paths = validate_local_image_paths(image_paths)?;

    let project = project::get_by_id(&app, &project_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("project not found: {project_id}"))?;
    let project_root = PathBuf::from(&project.root);

    let mut meta = storage::read_meta(&project_root, &thread_id).map_err(|e| e.to_string())?;
    let stored_events =
        storage::read_stored_events(&project_root, &thread_id).map_err(|e| e.to_string())?;
    let has_agent_output = stored_events.iter().any(stored_event_has_agent_output);
    let requested_plan_confirmation = plan_confirmed.unwrap_or(false);
    let mut meta_dirty = false;
    let mut plan_state_changed = false;
    let prompt_for_model = if meta.plan_mode {
        if requested_plan_confirmation {
            if !meta.plan_confirmed {
                meta.plan_confirmed = true;
                meta_dirty = true;
                plan_state_changed = true;
            }
            trimmed.to_string()
        } else if meta.plan_confirmed {
            meta.plan_confirmed = false;
            meta_dirty = true;
            plan_state_changed = true;
            wrap_with_plan_mode(trimmed)
        } else if has_agent_output {
            wrap_with_plan_revision(trimmed)
        } else {
            wrap_with_plan_mode(trimmed)
        }
    } else {
        trimmed.to_string()
    };
    let app_thread_id_opt: Option<String> = match meta.session_id.clone() {
        Some(sid) => Some(sid),
        None => {
            let extracted = stored_events.iter().find_map(|ev| {
                if ev.stream != "stdout" {
                    return None;
                }
                let parsed: serde_json::Value = serde_json::from_str(&ev.raw).ok()?;
                codex::find_session_id(&parsed)
            });
            if let Some(ref sid) = extracted {
                meta.session_id = Some(sid.clone());
                meta_dirty = true;
            }
            extracted
        }
    };
    if meta_dirty {
        storage::write_meta(&project_root, &meta).map_err(|e| e.to_string())?;
    }

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis();

    let last_seq =
        storage::count_events(&project_root, &thread_id).map_err(|e| e.to_string())?;
    let user_seq = last_seq + 1;
    let user_raw = serde_json::to_string(&serde_json::json!({
        "type": "user_message",
        "text": trimmed,
    }))
    .map_err(|e| e.to_string())?;
    let user_event = storage::StoredEvent {
        seq: user_seq,
        stream: "user".into(),
        ts_ms: now_ms,
        raw: user_raw.clone(),
    };
    storage::append_event_oneshot(&project_root, &thread_id, &user_event)
        .map_err(|e| e.to_string())?;
    if let Err(e) = storage::reopen_thread(&project_root, &thread_id) {
        eprintln!("[reflex] reopen_thread failed: {e}");
    }

    let _ = app.emit(
        "reflex://thread-running",
        &serde_json::json!({ "thread_id": thread_id }),
    );
    if plan_state_changed {
        let _ = app.emit(
            "reflex://thread-meta-updated",
            &serde_json::json!({
                "thread_id": thread_id,
                "plan_confirmed": meta.plan_confirmed,
            }),
        );
    }

    let _ = app.emit(
        "reflex://codex-event",
        &serde_json::json!({
            "thread_id": thread_id,
            "seq": user_seq,
            "raw": user_raw,
            "stream": "user",
        }),
    );

    memory_kick_topic(&app, &project_root, &thread_id);

    let app_handle = app.clone();
    let id_for_task = thread_id.clone();
    let prompt_owned = prompt_for_model;
    let root_for_task = project_root.clone();
    let project_id_for_task = project_id.clone();
    let image_paths_for_task = image_paths.clone();
    tauri::async_runtime::spawn(async move {
        let proj_now = project::get_by_id(&app_handle, &project_id_for_task)
            .ok()
            .flatten();
        let profile = proj_now
            .as_ref()
            .map(project_agent_profile_preface)
            .unwrap_or_default();
        let prompt_owned = match crate::memory::injection::build_preface(
            &root_for_task,
            &id_for_task,
            &prompt_owned,
        )
        .await
        {
            Ok(r) => crate::memory::injection::wrap_user_prompt(&r.preface, &prompt_owned),
            Err(e) => {
                eprintln!("[reflex] memory inject failed (continue): {e}");
                prompt_owned
            }
        };
        let prompt_owned = wrap_with_project_agent_profile(&profile, &prompt_owned);
        let handle = app_handle.state::<app_server::AppServerHandle>();
        let server = handle.wait().await;
        let initial_seq = storage::count_events(&root_for_task, &id_for_task).unwrap_or(0);

        let sandbox = proj_now
            .as_ref()
            .map(|p| p.sandbox.clone())
            .unwrap_or_else(|| "workspace-write".into());
        let mcp = proj_now.as_ref().and_then(|p| p.mcp_servers.clone());

        // Ensure we have an app-server session — start one lazily if needed.
        let mut sid: String = match app_thread_id_opt {
            Some(s) => s,
            None => match server
                .thread_start(&root_for_task, &sandbox, mcp.as_ref())
                .await
            {
                Ok(s) => {
                    if let Ok(mut m) = storage::read_meta(&root_for_task, &id_for_task) {
                        m.session_id = Some(s.clone());
                        let _ = storage::write_meta(&root_for_task, &m);
                    }
                    s
                }
                Err(e) => {
                    eprintln!("[reflex] thread_start (continue) failed: {e}");
                    let _ = app_handle.emit(
                        "reflex://codex-event",
                        &serde_json::json!({
                            "thread_id": id_for_task,
                            "seq": initial_seq + 1,
                            "raw": format!("[reflex] thread_start failed: {e}"),
                            "stream": "error",
                        }),
                    );
                    let _ = storage::finalize_thread(&root_for_task, &id_for_task, Some(-1), None);
                    let _ = app_handle.emit(
                        "reflex://codex-end",
                        &serde_json::json!({"thread_id": id_for_task, "exit_code": -1}),
                    );
                    return;
                }
            },
        };

        server.register_thread(
            sid.clone(),
            id_for_task.clone(),
            root_for_task.clone(),
            initial_seq,
        );

        let mut turn_result = server
            .turn_start_with_local_images(&sid, &prompt_owned, &image_paths_for_task)
            .await;

        // If app-server forgot the thread (e.g. fresh process), start a new session and retry.
        let lost = matches!(&turn_result, Err(e) if e.get("message").and_then(|m| m.as_str()).map(|s| s.contains("thread not found")).unwrap_or(false));
        if lost {
            eprintln!("[reflex] turn_start said thread not found — starting fresh session");
            match server
                .thread_start(&root_for_task, &sandbox, mcp.as_ref())
                .await
            {
                Ok(new_sid) => {
                    if let Ok(mut m) = storage::read_meta(&root_for_task, &id_for_task) {
                        m.session_id = Some(new_sid.clone());
                        let _ = storage::write_meta(&root_for_task, &m);
                    }
                    server.register_thread(
                        new_sid.clone(),
                        id_for_task.clone(),
                        root_for_task.clone(),
                        initial_seq,
                    );
                    sid = new_sid;
                    turn_result = server
                        .turn_start_with_local_images(
                            &sid,
                            &prompt_owned,
                            &image_paths_for_task,
                        )
                        .await;
                }
                Err(e) => {
                    eprintln!("[reflex] thread_start retry failed: {e}");
                }
            }
        }
        let _ = sid;

        if let Err(e) = turn_result {
            eprintln!("[reflex] turn_start (continue) failed: {e}");
            let _ = app_handle.emit(
                "reflex://codex-event",
                &serde_json::json!({
                    "thread_id": id_for_task,
                    "seq": initial_seq + 1,
                    "raw": format!("[reflex] turn_start failed: {e}"),
                    "stream": "error",
                }),
            );
            let _ = storage::finalize_thread(&root_for_task, &id_for_task, Some(-1), None);
            let _ = app_handle.emit(
                "reflex://codex-end",
                &serde_json::json!({"thread_id": id_for_task, "exit_code": -1}),
            );
        }
    });

    Ok(())
}

async fn show_quick_panel(app: &AppHandle) {
    let ctx = context::capture(app).await;
    let candidate = candidate_root(&ctx);
    let project = candidate
        .as_deref()
        .and_then(|p: &Path| project::find_project_for(p));
    let nearest = if project.is_none() {
        if let Some(root) = candidate.as_deref() {
            project::nearest_registered(app, root).unwrap_or_default()
        } else {
            project::list_registered(app).unwrap_or_default()
        }
    } else {
        Vec::new()
    };
    let candidate_root_str = candidate
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned());

    let payload = QuickOpenPayload {
        ctx,
        project,
        candidate_root: candidate_root_str,
        nearest,
    };

    let Some(window) = app.get_webview_window(QUICK_WINDOW) else {
        return;
    };
    let _ = window.emit(QUICK_OPEN_EVENT, &payload);
    let _ = window.show();
    let _ = window.set_focus();
}

fn show_main_window(app: &AppHandle) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        return;
    };
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
}

fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    use tauri::menu::{MenuBuilder, MenuItem};
    use tauri::tray::TrayIconBuilder;

    let open_item = MenuItem::with_id(app, "open", "Open Reflex", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit Reflex", true, Some("Cmd+Q"))?;
    let menu = MenuBuilder::new(app)
        .items(&[&open_item, &quit_item])
        .build()?;

    let mut builder = TrayIconBuilder::with_id("reflex-tray")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .tooltip("Reflex")
        .on_menu_event(|app, event| match event.id().as_ref() {
            "quit" => app.exit(0),
            "open" => show_main_window(app),
            _ => {}
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder.build(app)?;
    Ok(())
}

fn prune_orphan_threads(app: &AppHandle) {
    let projects = match project::list_registered(app) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[reflex] prune: list_registered failed: {e}");
            return;
        }
    };
    let valid_ids: std::collections::HashSet<String> =
        projects.iter().map(|p| p.id.clone()).collect();
    for p in &projects {
        let root = PathBuf::from(&p.root);
        let threads = match storage::read_all_threads(&root) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("[reflex] prune: read_all_threads({}) failed: {e}", p.root);
                continue;
            }
        };
        for t in threads {
            let belongs = match &t.meta.project_id {
                Some(pid) => valid_ids.contains(pid) && pid == &p.id,
                None => false,
            };
            if !belongs {
                eprintln!(
                    "[reflex] prune orphan thread {} (project_id={:?}, project_root={})",
                    t.meta.id, t.meta.project_id, p.root
                );
                if let Err(e) = storage::delete_thread(&root, &t.meta.id) {
                    eprintln!("[reflex] prune: delete_thread {} failed: {e}", t.meta.id);
                }
            }
        }
    }
}

async fn resume_interrupted_threads(app: AppHandle, server: app_server::AppServerClient) {
    let projects = match project::list_registered(&app) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[reflex] resume scan: list_registered err: {e}");
            return;
        }
    };

    const RESUME_PROMPT: &str = "Reflex was restarted. Continue from where you stopped. If the task is already complete, briefly say so.";

    for p in projects {
        let root = PathBuf::from(&p.root);
        let stored = match storage::read_all_threads(&root) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[reflex] read_all_threads({}): {e}", p.root);
                continue;
            }
        };
        for st in stored {
            if st.meta.done {
                continue;
            }
            let reflex_id = st.meta.id.clone();
            let Some(session_id) = st.meta.session_id.clone() else {
                eprintln!(
                    "[reflex] cannot resume {reflex_id}: no session_id; finalizing as failed"
                );
                let _ =
                    storage::finalize_thread(&root, &reflex_id, Some(-2), None);
                let _ = app.emit(
                    "reflex://codex-end",
                    &serde_json::json!({
                        "thread_id": reflex_id,
                        "exit_code": -2,
                    }),
                );
                continue;
            };

            eprintln!("[reflex] auto-resume thread={reflex_id} session={session_id}");

            memory_kick_topic(&app, &root, &reflex_id);
            let resume_prompt = match crate::memory::injection::build_preface(
                &root,
                &reflex_id,
                RESUME_PROMPT,
            )
            .await
            {
                Ok(r) => crate::memory::injection::wrap_user_prompt(&r.preface, RESUME_PROMPT),
                Err(e) => {
                    eprintln!("[reflex] memory inject failed (resume): {e}");
                    RESUME_PROMPT.to_string()
                }
            };
            let profile = project_agent_profile_preface(&p);
            let resume_prompt = wrap_with_project_agent_profile(&profile, &resume_prompt);

            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            let last_seq = storage::count_events(&root, &reflex_id).unwrap_or(0);
            let user_seq = last_seq + 1;
            let user_raw = match serde_json::to_string(&serde_json::json!({
                "type": "user_message",
                "text": RESUME_PROMPT,
                "auto_resumed": true,
            })) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("[reflex] resume json err: {e}");
                    continue;
                }
            };
            let user_event = storage::StoredEvent {
                seq: user_seq,
                stream: "user".into(),
                ts_ms: now_ms,
                raw: user_raw.clone(),
            };
            let _ = storage::append_event_oneshot(&root, &reflex_id, &user_event);
            let _ = storage::reopen_thread(&root, &reflex_id);

            let _ = app.emit(
                "reflex://thread-running",
                &serde_json::json!({"thread_id": reflex_id}),
            );
            let _ = app.emit(
                "reflex://codex-event",
                &serde_json::json!({
                    "thread_id": reflex_id,
                    "seq": user_seq,
                    "raw": user_raw,
                    "stream": "user",
                }),
            );

            // load thread on app-server first
            if let Err(e) = server
                .thread_resume(&session_id, &p.sandbox, p.mcp_servers.as_ref())
                .await
            {
                eprintln!("[reflex] thread_resume failed for {reflex_id}: {e}");
                let _ = storage::finalize_thread(&root, &reflex_id, Some(-1), None);
                let _ = app.emit(
                    "reflex://codex-end",
                    &serde_json::json!({"thread_id": reflex_id, "exit_code": -1}),
                );
                continue;
            }

            server.register_thread(
                session_id.clone(),
                reflex_id.clone(),
                root.clone(),
                user_seq,
            );

            if let Err(e) = server.turn_start(&session_id, &resume_prompt).await {
                eprintln!("[reflex] auto-resume turn_start failed for {reflex_id}: {e}");
                let _ = storage::finalize_thread(&root, &reflex_id, Some(-1), None);
                let _ = app.emit(
                    "reflex://codex-end",
                    &serde_json::json!({"thread_id": reflex_id, "exit_code": -1}),
                );
            }
        }
    }
}

#[cfg(desktop)]
fn quick_shortcut() -> tauri_plugin_global_shortcut::Shortcut {
    use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};
    Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::Space)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    std::panic::set_hook(Box::new(|info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "<unknown>".into());
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| format!("{info}"));
        let line = format!("REFLEX PANIC at {location}: {payload}");
        eprintln!("{line}");
        if let Ok(home) = std::env::var("HOME") {
            let path = std::path::PathBuf::from(home)
                .join("Library")
                .join("Logs")
                .join("reflex-panic.log");
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            use std::io::Write as _;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
            {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0);
                let _ = writeln!(f, "[{ts}] {line}");
            }
        }
    }));

    let builder = tauri::Builder::default()
        .register_uri_scheme_protocol("reflexapp", |ctx, request| -> tauri::http::Response<std::borrow::Cow<'static, [u8]>> {
            let app = ctx.app_handle();
            let uri = request.uri();
            let path = uri.path().trim_start_matches('/').to_string();
            let mut parts = path.splitn(2, '/');
            let id = parts.next().unwrap_or("");
            let rel = parts.next().unwrap_or("index.html");
            eprintln!("[reflexapp] uri={uri} host={:?} path={path} id={id} rel={rel}", uri.host());
            if id.is_empty() {
                return tauri::http::Response::builder()
                    .status(400)
                    .body(std::borrow::Cow::Owned(Vec::new()))
                    .unwrap();
            }
            match apps::read_app_file(app, id, rel) {
                Ok(bytes) => {
                    eprintln!("[reflexapp] OK {id}/{rel} ({} bytes)", bytes.len());
                    let mime = apps::guess_mime(rel);
                    let final_bytes = if mime.starts_with("text/html") {
                        apps::inject_overlay_into_html(&bytes)
                    } else {
                        bytes
                    };
                    tauri::http::Response::builder()
                        .header("content-type", mime)
                        .body(std::borrow::Cow::Owned(final_bytes))
                        .unwrap()
                }
                Err(e) => {
                    eprintln!("[reflexapp] ERR {id}/{rel}: {e}");
                    tauri::http::Response::builder()
                        .status(404)
                        .body(std::borrow::Cow::Owned(Vec::new()))
                        .unwrap()
                }
            }
        })
        .register_uri_scheme_protocol("reflexserver", |ctx, request| -> tauri::http::Response<std::borrow::Cow<'static, [u8]>> {
            let app = ctx.app_handle();
            let uri = request.uri();
            let id = uri.host().unwrap_or("");
            if id.is_empty() {
                return tauri::http::Response::builder()
                    .status(400)
                    .body(std::borrow::Cow::Owned(Vec::new()))
                    .unwrap();
            }
            let port = tauri::async_runtime::block_on(async {
                let runtimes = app.state::<app_runtime::AppRuntimes>();
                app_runtime::running_port(runtimes.inner(), id).await
            });
            let Some(port) = port else {
                return tauri::http::Response::builder()
                    .status(503)
                    .header("content-type", "text/plain; charset=utf-8")
                    .body(std::borrow::Cow::Owned(b"server runtime is not running".to_vec()))
                    .unwrap();
            };
            match apps::proxy_server_runtime_request(port, &request) {
                Ok(proxied) => {
                    let mut builder = tauri::http::Response::builder().status(proxied.status);
                    for (name, value) in proxied.headers {
                        builder = builder.header(name, value);
                    }
                    builder
                        .body(std::borrow::Cow::Owned(proxied.body))
                        .unwrap()
                }
                Err(e) => tauri::http::Response::builder()
                    .status(502)
                    .header("content-type", "text/plain; charset=utf-8")
                    .body(std::borrow::Cow::Owned(e.into_bytes()))
                    .unwrap(),
            }
        })
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(app_server::AppServerHandle::default())
        .manage(app_runtime::AppRuntimes::default())
        .manage(app_watcher::AppWatchers::default())
        .manage(project_watcher::ProjectWatchers::default())
        .manage(memory::MemoryState::default())
        .manage(scheduler::SchedulerHandle::default())
        .manage(app_bus::AppBusBridge::default())
        .manage(browser::BrowserSidecar::default())
        .manage(logs::LogStore::default())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init());

    #[cfg(desktop)]
    let builder = builder.plugin(
        tauri_plugin_global_shortcut::Builder::new()
            .with_handler(|app, shortcut, event| {
                use tauri_plugin_global_shortcut::ShortcutState;
                if event.state() != ShortcutState::Pressed {
                    return;
                }
                if shortcut != &quick_shortcut() {
                    return;
                }
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    show_quick_panel(&app).await;
                });
            })
            .build(),
    );

    builder
        .setup(|app| {
            setup_tray(app.handle())?;
            for label in [MAIN_WINDOW, QUICK_WINDOW] {
                if let Some(window) = app.get_webview_window(label) {
                    let win_clone = window.clone();
                    window.on_window_event(move |event| {
                        if let WindowEvent::CloseRequested { api, .. } = event {
                            api.prevent_close();
                            let _ = win_clone.hide();
                        }
                    });
                }
            }
            #[cfg(desktop)]
            {
                use tauri_plugin_global_shortcut::GlobalShortcutExt;
                app.global_shortcut().register(quick_shortcut())?;
            }

            if let Err(e) = apps::ensure_sample_app(app.handle()) {
                eprintln!("[reflex] ensure_sample_app failed: {e}");
            }

            prune_orphan_threads(app.handle());

            let app_handle_for_server = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                match app_server::AppServerClient::start(app_handle_for_server.clone()).await {
                    Ok(client) => {
                        match client.initialize().await {
                            Ok(info) => eprintln!("[app-server] initialized: {info:?}"),
                            Err(e) => eprintln!("[app-server] initialize failed: {e}"),
                        }
                        let handle = app_handle_for_server.state::<app_server::AppServerHandle>();
                        handle.set(client.clone()).await;
                        resume_interrupted_threads(app_handle_for_server.clone(), client).await;
                    }
                    Err(e) => eprintln!("[app-server] failed to start: {e}"),
                }
            });

            let app_for_scheduler = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let h: scheduler::SchedulerHandle = app_for_scheduler
                    .state::<scheduler::SchedulerHandle>()
                    .inner()
                    .clone();
                scheduler::engine::run(app_for_scheduler, h).await;
            });

            let app_for_bus = app.handle().clone();
            let bridge: app_bus::AppBusBridge = app_for_bus
                .state::<app_bus::AppBusBridge>()
                .inner()
                .clone();
            let bus = app_for_bus.state::<memory::MemoryState>().bus.clone();
            bus_log::start(bus.clone(), app_for_bus.clone());
            app_bus::start(bridge, bus, app_for_bus);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            capture_context,
            submit_quick,
            list_threads,
            list_projects,
            get_active_project,
            set_active_project,
            create_project,
            update_project_description,
            update_project_agent_profile,
            link_app_to_project,
            unlink_app_from_project,
            suggester::suggest_apps_for_project,
            find_project_for_path,
            update_project_sandbox,
            update_project_browser,
            update_project_mcp_servers,
            list_apps,
            read_app_html,
            app_invoke,
            create_app,
            install_connected_app,
            app_status,
            app_save,
            app_revert,
            app_diff,
            app_save_partial,
            app_revise,
            read_app_thread,
            create_app_thread,
            pick_directory,
            pick_open_file,
            pick_save_file,
            read_app_manifest,
            app_export,
            app_import,
            delete_app,
            list_trashed_apps,
            restore_app,
            purge_trashed_app,
            app_server_start,
            app_server_stop,
            app_server_restart,
            app_server_status,
            app_server_logs,
            app_watch_start,
            app_watch_stop,
            project_watch_start,
            project_watch_stop,
            list_directory,
            list_project_files,
            reveal_in_finder,
            set_thread_goal,
            continue_thread,
            stop_thread,
            respond_to_question,
            memory::tools::memory_save,
            memory::tools::memory_list,
            memory::tools::memory_delete,
            memory::tools::memory_search,
            memory::tools::memory_recall,
            memory::tools::memory_stats,
            memory::tools::memory_reindex,
            memory::tools::memory_index_path,
            memory::tools::memory_path_status,
            memory::tools::memory_path_status_batch,
            memory::tools::memory_forget_path,
            scheduler::commands::scheduler_list,
            scheduler::commands::scheduler_set_paused,
            scheduler::commands::scheduler_run_now,
            scheduler::commands::scheduler_runs,
            scheduler::commands::scheduler_stats,
            scheduler::commands::scheduler_run_detail,
            browser::browser_init,
            browser::browser_switch_project,
            browser::browser_shutdown,
            browser::browser_tabs_list,
            browser::browser_tab_open,
            browser::browser_tab_close,
            browser::browser_navigate,
            browser::browser_back,
            browser::browser_forward,
            browser::browser_reload,
            browser::browser_current_url,
            browser::browser_read_text,
            browser::browser_read_outline,
            browser::browser_click_text,
            browser::browser_click_selector,
            browser::browser_fill,
            browser::browser_scroll,
            browser::browser_wait_for,
            browser::browser_screenshot,
            browser::browser_state_save,
            browser::browser_screencast_start,
            browser::browser_screencast_stop,
            browser::browser_set_viewport,
            browser::browser_mouse_move,
            browser::browser_mouse_down,
            browser::browser_mouse_up,
            browser::browser_mouse_click,
            browser::browser_mouse_wheel,
            browser::browser_keyboard_type,
            browser::browser_keyboard_press,
            logs::logs_get,
            logs::log_push,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::ExitRequested { .. }) {
                let scheduler_h: scheduler::SchedulerHandle =
                    app.state::<scheduler::SchedulerHandle>().inner().clone();
                scheduler_h.shutdown();
                let runtimes = app.state::<app_runtime::AppRuntimes>();
                let runtimes_arc = runtimes.servers.clone();
                tauri::async_runtime::block_on(async move {
                    let mut map = runtimes_arc.lock().await;
                    for (_id, mut entry) in map.drain() {
                        let _ = entry.child.kill().await;
                    }
                });
            }
        });
}
