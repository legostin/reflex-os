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
        prompt: format!("Доработка app: {label}"),
        cwd: project.root.clone(),
        frontmost_app: None,
        finder_target: None,
        created_at_ms: now_ms,
        exit_code: Some(0),
        done: true,
        session_id: None,
        title: Some(format!("App · {label}")),
        goal: Some("Доработка утилиты".into()),
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
    let title = title.unwrap_or_else(|| "Выбор папки".to_string());
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
    let title = title.unwrap_or_else(|| "Открыть файл".to_string());
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
    let title = title.unwrap_or_else(|| "Сохранить как".to_string());
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
        "Доработай Reflex app в текущей рабочей папке.\n\n\
ИЗМЕНЕНИЯ: {trimmed}\n\n\
ПАМЯТКА (актуальный bridge / runtime):\n\
- Можно использовать любую структуру: index.html + style.css + app.js + assets/. Reflex отдаёт всё через reflexapp:// scheme c правильным mime.\n\
- Два runtime: static (default) или server (manifest.runtime=\"server\" + manifest.server.command — node/python stdlib, listen на process.env.PORT).\n\
- Bridge через window.parent.postMessage({{source:'reflex-app', type:'request', id, method, params}}). Доступные методы:\n\
  • system.context() → {{app_id, app_root, manifest, app_project, linked_projects, memory_defaults}}\n\
  • manifest.get() / manifest.update({{patch}}) — безопасно читать/обновлять собственный manifest.json\n\
  • agent.ask({{prompt}}) → {{answer}}\n\
  • agent.task({{prompt, sandbox?, cwd?}}) → {{threadId, result}} — изолированный sub-агент\n\
  • agent.stream({{prompt}}) → {{streamId}} — стриминг токенов; слушай parent message {{source:'reflex', type:'stream.token'|'stream.done'}}\n\
  • storage.get/set, fs.read/write (в app-папке)\n\
  • projects.list / topics.list — read-only обзор доступных проектов и топиков; чужие требуют permission projects.read/topics.read\n\
  • browser.init/tabs.list/open/navigate/readText/readOutline/screenshot/clickText/clickSelector/fill — встроенный browser sidecar; требует browser.read/control\n\
  • memory.save/list/delete/search/recall/indexPath/pathStatus/forgetPath — память Reflex; project scope по умолчанию, global требует permission memory.global.read/write\n\
  • scheduler.list/runNow/setPaused/runs/runDetail — читать и управлять своими расписаниями; чужие требуют permission scheduler.read/run/write\n\
  • dialog.openDirectory/openFile/saveFile — нативные диалоги\n\
  • notify.show — macOS push\n\
  • net.fetch({{url, method?, headers?, body?}}) — требует manifest.network.allowed_hosts (поддержка \"*.foo.com\")\n\
- iframe sandbox=\"allow-scripts allow-forms\" (для server runtime + allow-same-origin). Никаких внешних CDN — только inline или локальные файлы.\n\
- Reflex автоматически инжектит overlay-скрипт в HTML: ловит window.onerror/unhandledrejection (юзер увидит ✨Fix), и режим Inspector (юзер кликает → ты получишь selector + outerHTML). Не пиши свой обработчик с теми же типами событий.\n\
- После твоих правок iframe перезагрузится сам (file watcher), для server runtime — процесс перезапустится. Не требуй ручного reload.\n\
- Не трогай .reflex/, .git/, storage.json. Manifest можно обновлять (permissions, network.allowed_hosts, runtime, server)."
    );
    continue_thread(app, project.id, latest.meta.id.clone(), prompt, None)?;
    Ok(serde_json::json!({"thread_id": latest.meta.id}))
}

#[tauri::command]
async fn create_app(
    app: AppHandle,
    description: String,
    template: Option<String>,
) -> Result<serde_json::Value, String> {
    let trimmed = description.trim();
    if trimmed.is_empty() {
        return Err("empty description".into());
    }
    let template = template.unwrap_or_else(|| "blank".to_string());

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis();
    let app_id = format!("app_{now_ms}");

    let apps_root = apps::apps_dir(&app).map_err(|e| e.to_string())?;
    let dir = apps_root.join(&app_id);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let label = short_label(trimmed);
    let proj_name = format!("App · {label}");
    let project = project::create_project(&app, &dir, Some(proj_name.clone()), None)
        .map_err(|e| e.to_string())?;

    let manifest = apps::AppManifest {
        id: app_id.clone(),
        name: label.clone(),
        icon: Some("🧩".into()),
        description: Some(trimmed.to_string()),
        entry: "index.html".into(),
        permissions: vec![],
        kind: "panel".into(),
        created_at_ms: now_ms,
        runtime: None,
        server: None,
        network: None,
        schedules: Vec::new(),
        actions: Vec::new(),
        widgets: Vec::new(),
    };
    apps::write_manifest(&app, &app_id, &manifest).map_err(|e| e.to_string())?;
    let _ = app.emit("reflex://apps-changed", &serde_json::json!({}));

    let thread_id = format!("t_{now_ms}");
    let project_root = dir.clone();
    let prompt = build_app_creation_prompt(trimmed, &template);

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
        title: Some(format!("Создание app: {label}")),
        goal: Some(format!("Написать Reflex app: {trimmed}")),
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

fn short_label(s: &str) -> String {
    let mut iter = s.chars();
    let truncated: String = iter.by_ref().take(48).collect();
    truncated.trim().trim_end_matches('.').to_string()
}

fn wrap_with_plan_mode(prompt: &str) -> String {
    format!(
        "⚠️ РЕЖИМ ПЛАНИРОВАНИЯ — ИЗУЧИ, ПОТОМ СОСТАВЬ ПЛАН\n\n\
Сначала ИЗУЧИ репозиторий и контекст, потом составь план по итогам изучения. План — это документ для исполнения, а не описание того, как ты будешь изучать.\n\n\
ЧТО МОЖНО НА ЭТОМ ХОДУ:\n\
- читать файлы (cat/read), листать директории (ls/find/tree), грепать (rg/grep), смотреть git log/diff;\n\
- запускать любые read-only команды для понимания (например `cargo check`, `--help`);\n\
- задавать тулзовым вызовам столько чтения сколько нужно для уверенности.\n\n\
ЧТО НЕЛЬЗЯ:\n\
- модифицировать файлы, создавать/удалять что-либо;\n\
- ставить зависимости, запускать миграции, любые команды с сайд-эффектами;\n\
- угадывать поведение если можно прочитать и убедиться.\n\n\
ПОРЯДОК:\n\
1) Найди и прочитай релевантные файлы. Укажи в плане конкретные пути и строки которые ты посмотрел.\n\
2) Найди существующие паттерны/функции/типы которые можно переиспользовать вместо того чтобы писать с нуля.\n\
3) Только после этого — пиши план.\n\n\
СТРУКТУРА ПЛАНА (всё конкретное, без воды):\n\
- Контекст: что я понял из задачи и из кода (1-3 предложения).\n\
- Затронутые файлы: список путей с пометкой создать/изменить и зачем.\n\
- Реиспользуемые building blocks: какие функции/типы уже есть и я их буду звать (с file:line).\n\
- Шаги исполнения: пошаговый список действий по которому ты пойдёшь после `go`.\n\
- Ключевые решения: структура, API, библиотеки — с обоснованием почему именно так.\n\
- Верификация: как проверим что работает (тесты, ручные сценарии, команды).\n\
- Открытые вопросы (если есть): только реальные неоднозначности, не выдуманные.\n\n\
В конце ОБЯЗАТЕЛЬНО:\n\
«Жду подтверждения. Напиши `go` чтобы я выполнил план как есть, или скажи что поправить — я перепланирую и снова покажу.»\n\n\
ЗАДАЧА:\n{prompt}"
    )
}

fn wrap_with_plan_revision(feedback: &str) -> String {
    format!(
        "⚠️ РЕЖИМ ПЛАНИРОВАНИЯ — ЭТО ПРАВКА ПЛАНА, НЕ ВЫПОЛНЕНИЕ\n\n\
Пользователь уточнил или поправил предыдущий план. НЕ модифицируй файлы и не запускай команды с сайд-эффектами на этом ходу. Дочитать что нужно для правки — можно (read-only). \
Обнови план с учетом замечания и снова попроси подтверждение перед выполнением.\n\n\
ПРАВКА ПОЛЬЗОВАТЕЛЯ:\n{feedback}"
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
            "Шаблон CHAT-UTILITY:\n\
- Layout: список сообщений сверху + textarea + кнопка Send снизу.\n\
- Использовать `agent.stream({prompt})` для стриминга ответа. Ловить window 'message' с {source:'reflex', type:'stream.token', streamId, token} и …'stream.done'.\n\
- Хранить историю сообщений в storage.json (key=\"messages\").\n\
- Сообщения user/agent визуально разные. Streaming-сообщение растёт по токенам.\n",
        ),
        "dashboard" => Some(
            "Шаблон DASHBOARD:\n\
- Кнопка \"Refresh\" вызывает `agent.task({prompt: \"...запрос за данными...\"})` и парсит JSON-ответ.\n\
- Показывать данные в таблице или как summary-карточки.\n\
- Кэшировать последний результат в storage.json.\n",
        ),
        "form" => Some(
            "Шаблон FORM-TOOL:\n\
- Несколько input-полей сверху, кнопка \"Run\" снизу.\n\
- На submit собрать значения, дёрнуть `agent.task({prompt: \"...на основе значений...\"})`, показать результат.\n\
- Сохранять последний submit в storage.json для preset'а.\n",
        ),
        "api-client" => Some(
            "Шаблон API-CLIENT:\n\
- Используй `net.fetch` к указанному API. ОБЯЗАТЕЛЬНО добавь в manifest:\n  \"network\": { \"allowed_hosts\": [\"<host>\"] }\n\
- Кнопка для запроса, отображение результата (JSON pretty-print).\n\
- Если нужны секреты — спроси у пользователя через input-field, храни в storage.json.\n",
        ),
        "automation" => Some(
            "Шаблон AUTOMATION:\n\
- Обязательно добавь в manifest.schedules хотя бы одно расписание с 6-польным cron (sec min hour dom month dow, UTC).\n\
- В schedule.steps используй bridge methods без UI: agent.task, storage.*, fs.*, net.fetch, notify.show, events.*, apps.invoke, memory.*, manifest.*, scheduler.list/runs/runDetail.\n\
- Не используй dialog.* и scheduler.runNow/setPaused внутри schedule.steps.\n\
- Добавь обычный UI, где видно состояние: scheduler.list(), scheduler.runs({limit: 20}), кнопка ручного запуска через scheduler.runNow({scheduleId}).\n\
- Если автоматизация производит полезные данные, сохраняй их в storage или memory.save и добавь manifest.actions для других apps.\n\
- Если результат нужен на проектном дашборде, добавь manifest.widgets с компактной страницей widgets/<id>.html.\n",
        ),
        "node-server" => Some(
            "Шаблон NODE-SERVER:\n\
- runtime=server, command=[\"node\", \"server.js\"].\n\
- В server.js — Node.js stdlib `http`, слушать `process.env.PORT`.\n\
- Базовые маршруты: GET / → index.html (через fs.readFileSync), GET /api/... → JSON.\n\
- index.html обращается к /api через fetch (same-origin благодаря allow-same-origin sandbox).\n",
        ),
        _ => None,
    }
}

fn build_app_creation_prompt(description: &str, template: &str) -> String {
    let mut p = String::new();
    p.push_str("Ты создаёшь Reflex app в текущей рабочей папке.\n\n");
    p.push_str("ВАЖНО — КОНТЕКСТ:\n");
    p.push_str("- Тот, кто описывает задачу, НЕ программист. Он не знает терминов, библиотек, edge-cases. Он формулирует на бытовом языке.\n");
    p.push_str("- Ты — помощник, а не коллега-разработчик. Вся техническая логика — на тебе. Не задавай вопросов «какой стек выбрать» / «как назвать функцию» / «что вернуть из API» — это твои решения.\n");
    p.push_str("- Технические решения выбирай самые оптимальные и подходящие для задачи: предпочитай stdlib и минимум зависимостей где это уместно, но не упрощай в ущерб результату. Никакой over-engineering и никакого недо-engineering.\n");
    p.push_str("- Досконально продумывай логику ДО написания кода: edge-cases, ошибки, пустые состояния, кому что показывать. Лучше 5 минут думать чем переписывать после ревизии.\n");
    p.push_str("- ЗАДАВАЙ ВОПРОСЫ юзеру если что-то про САМУ задачу неясно: \"какие именно поля нужно показывать\", \"что должно происходить при пустом списке\", \"какой источник данных брать\". НЕ задавай технические вопросы — на них отвечай сам.\n");
    p.push_str("- Делай работу только когда уверен, что понял задачу правильно. Если есть существенная неоднозначность — лучше спроси.\n\n");
    p.push_str("ФАЙЛЫ:\n");
    p.push_str("- manifest.json уже есть с заглушкой. Обнови поля: name, icon (один emoji), description (1 предложение), permissions (массив API-методов).\n");
    p.push_str("- Можно использовать любую файловую структуру: index.html + style.css + app.js + assets/, modules, и т.д. Reflex отдаёт все файлы из папки app по mime-type автоматически.\n");
    p.push_str("- Тёмная тема: color #f5f5f7 на transparent background. Чистый минимальный UI.\n\n");
    p.push_str("ДВА RUNTIME:\n");
    p.push_str("1) static (по умолчанию): чистый front-end. iframe смотрит на reflexapp://localhost/<id>/<entry>. Нет своего бэкенда.\n");
    p.push_str("   - manifest: { runtime: \"static\", entry: \"index.html\" }  (либо просто опусти runtime).\n");
    p.push_str("   - Не подключай внешние CDN — только локальные файлы или inline.\n");
    p.push_str("2) server: при открытии Reflex поднимает локальный веб-сервер из manifest.server.command, передавая порт через env REFLEX_PORT и PORT. iframe смотрит на http://localhost:PORT/.\n");
    p.push_str("   - manifest: { runtime: \"server\", server: { command: [\"node\", \"server.js\"], ready_timeout_ms: 15000 } }\n");
    p.push_str("   - cwd процесса = папка app. Сервер ОБЯЗАН слушать на process.env.PORT (или REFLEX_PORT).\n");
    p.push_str("   - Все зависимости (npm/pip и т.д.) должны быть либо vendored в app-папке, либо stdlib. Не предполагай глобальные npm install — пиши на чистом Node.js stdlib (http/fs/path) или Python stdlib (http.server/socketserver).\n");
    p.push_str("   - entry в манифесте можно не задавать — это для server-режима не используется.\n\n");
    p.push_str("BRIDGE (общение с Reflex через window.parent.postMessage):\n");
    p.push_str("Запрос:  window.parent.postMessage({source:'reflex-app', type:'request', id, method, params}, '*');\n");
    p.push_str("Ответ:   window.addEventListener('message', e => {\n");
    p.push_str("           if (e.data?.source==='reflex' && e.data.type==='response' && e.data.id===id) ...\n");
    p.push_str("         });\n\n");
    p.push_str("ДОСТУПНЫЕ МЕТОДЫ:\n");
    p.push_str("  system.context() -> {app_id, app_root, manifest, app_project, linked_projects, memory_defaults} — контекст текущей утилиты и привязанных проектов\n");
    p.push_str("  manifest.get() -> AppManifest; manifest.update({patch}) -> {ok, manifest} — безопасно обновить собственный manifest.json (id всегда остаётся текущим app)\n");
    p.push_str("  agent.ask({prompt}) -> {answer}                       — короткий one-shot вопрос агенту\n");
    p.push_str("  agent.startTopic({prompt, projectId?}) -> {threadId}   — создать полноценный тред\n");
    p.push_str("  agent.task({prompt, sandbox?, cwd?}) -> {threadId, result}  — sub-агент изолированно; sandbox: read-only|workspace-write; ждёт turn.completed и возвращает финальный текст\n");
    p.push_str("  agent.stream({prompt, sandbox?, cwd?}) -> {streamId, threadId}  — стрим токенов: app слушает window 'message' от parent с {source:'reflex', type:'stream.token', streamId, token} и …'stream.done' с {streamId, result}. По завершении вызывай agent.streamAbort({threadId}) при размонтаже.\n");
    p.push_str("  storage.get({key}) -> {value}                         — persist в storage.json\n");
    p.push_str("  storage.set({key, value}) -> {ok}\n");
    p.push_str("  fs.read({path}) -> {content}                          — читать файл в app-папке\n");
    p.push_str("  fs.write({path, content}) -> {ok}                     — писать файл в app-папке\n");
    p.push_str("  notify.show({title, body}) -> {ok}                    — macOS push\n");
    p.push_str("  dialog.openDirectory({title?, defaultPath?}) -> {path|null}                          — нативное окно выбора папки (path = null если отмена)\n");
    p.push_str("  dialog.openFile({title?, defaultPath?, filters?, multiple?}) -> {path|null} или {paths:[]}  — нативное окно выбора файла. filters: [{name, extensions:[\"txt\",...]}]\n");
    p.push_str("  dialog.saveFile({title?, defaultPath?, filters?, content?}) -> {path|null}            — окно \"сохранить как\". Если передан content (string) — файл сразу записывается на выбранный путь\n");
    p.push_str("  net.fetch({url, method?, headers?, body?, timeoutMs?}) -> {status, headers, body, encoding}  — HTTP-запрос. Хост ОБЯЗАН быть в manifest.network.allowed_hosts (поддержка \"*.example.com\"). Body — string, либо JSON (auto-serialize). encoding=\"utf8\"|\"base64\".\n\n");
    p.push_str("PROJECT/TOPIC API — используй для OS-dashboard, навигации и обзора работы агента.\n");
    p.push_str("  projects.list({includeAll?}) -> ProjectSummary[] — по умолчанию только linked projects; includeAll требует permission \"projects.read:*\"\n");
    p.push_str("  topics.list({projectId?, limit?, includeAll?}) -> TopicSummary[] — метаданные топиков без raw events; чужие проекты требуют permission \"topics.read:<project>\" или \"topics.read:*\"\n\n");
    p.push_str("BROWSER API — встроенный Playwright/browser sidecar для research, QA и web workflows.\n");
    p.push_str("  browser.init({headless?, projectId?}); browser.tabs.list(); browser.open({url?}); browser.navigate({tabId, url})\n");
    p.push_str("  browser.readText({tabId}); browser.readOutline({tabId}); browser.screenshot({tabId, fullPage?})\n");
    p.push_str("  browser.clickText({tabId, text, exact?}); browser.clickSelector({tabId, selector}); browser.fill({tabId, selector, value})\n");
    p.push_str("- Требует manifest.permissions: \"browser.read\" для чтения или \"browser.control\" для init/open/navigate/click/fill. Project browser state требует linked project или \"browser.project:<project>\".\n\n");
    p.push_str("SCHEDULER API — панель или widget могут показывать и контролировать автоматизации без ручного JSON.\n");
    p.push_str("  scheduler.list({appId?, includeAll?}) -> ScheduleListItem[] — по умолчанию только расписания текущего app\n");
    p.push_str("  scheduler.runNow({scheduleId}) -> {ok, schedule_id} — scheduleId может быть local id или \"app::schedule\"\n");
    p.push_str("  scheduler.setPaused({scheduleId, paused}) -> {ok, schedule_id, paused}\n");
    p.push_str("  scheduler.runs({limit?, beforeTs?, appId?, includeAll?}) -> RunSummary[]\n");
    p.push_str("  scheduler.runDetail({runId}) -> RunRecord|null\n");
    p.push_str("- Чужие app/schedule требуют manifest.permissions: \"scheduler.read:*\", \"scheduler.run:<app>\", \"scheduler.write:<app>::<schedule>\" или \"scheduler:*\".\n\n");
    p.push_str("MEMORY API — используй для долгой памяти, RAG и проектного контекста вместо собственного JSON-хака.\n");
    p.push_str("  memory.save({scope?, kind?, name, description?, body, tags?, projectId?, threadId?}) -> MemoryNote\n");
    p.push_str("  memory.list({scope?, filter?, projectId?, threadId?}) -> MemoryNote[]; filter: {kind?, tag?, query?}\n");
    p.push_str("  memory.delete({scope?, relPath, projectId?, threadId?}) -> {ok}\n");
    p.push_str("  memory.search({query, projectId?, limit?}) -> RagHit[] — поиск по индексированным файлам и заметкам проекта\n");
    p.push_str("  memory.recall({query, projectId?, threadId?, maxNotes?, maxRag?}) -> {markdown, notes, rag} — готовый контекст для агента\n");
    p.push_str("  memory.indexPath({path, projectId?}) -> {indexed, skipped}; memory.pathStatus({path, projectId?}); memory.forgetPath({path, projectId?})\n");
    p.push_str("- scope: \"project\" по умолчанию. Если app привязан ровно к одному проекту, project scope попадёт в память этого проекта; иначе — в память самого app.\n");
    p.push_str("- Для выбора проекта вызови system.context() и передай projectId из linked_projects. Для global scope добавь permission \"memory.global.read\" или \"memory.global.write\".\n");
    p.push_str("- В overlay уже есть helpers: reflexInvoke(method, params), reflexSystemContext(), reflexManifestGet(), reflexManifestUpdate(patch), reflexProjectsList(params), reflexTopicsList(params), reflexSchedulerList(params), reflexSchedulerRunNow(scheduleId), reflexSchedulerSetPaused(scheduleId, paused), reflexSchedulerRuns(params), reflexAppsInvoke(appId, actionId, params), reflexAppsListActions(appIdOrParams, includeSteps?), reflexEventOn/Off/Emit.\n");
    p.push_str("  Core helpers: reflexAgentAsk/Task/Stream/StreamAbort(...), reflexStorageGet/Set(...), reflexFsRead/Write(...), reflexNetFetch(...), reflexDialogOpenDirectory/OpenFile/SaveFile(...), reflexNotifyShow(...).\n");
    p.push_str("  Browser helpers: reflexBrowserInit(params), reflexBrowserTabs(), reflexBrowserOpen(url), reflexBrowserNavigate(tabId, url), reflexBrowserReadText(tabId), reflexBrowserReadOutline(tabId), reflexBrowserScreenshot(tabIdOrParams, fullPage?), reflexBrowserClickText(tabIdOrParams, text?, exact?), reflexBrowserClickSelector(tabIdOrParams, selector?), reflexBrowserFill(tabIdOrParams, selector?, value?).\n");
    p.push_str("  Memory helpers: reflexMemorySave(params), reflexMemoryList(params), reflexMemoryDelete(relPathOrParams), reflexMemorySearch(queryOrParams), reflexMemoryRecall(queryOrParams), reflexMemoryIndexPath(pathOrParams), reflexMemoryPathStatus(pathOrParams), reflexMemoryForgetPath(pathOrParams).\n\n");
    p.push_str("MANIFEST.network (для net.fetch):\n");
    p.push_str("  { \"network\": { \"allowed_hosts\": [\"api.example.com\", \"*.foo.com\"] } }\n\n");
    p.push_str("MANIFEST.schedules — повторяемые задачи. Reflex запускает их сам, даже когда окно app закрыто (Reflex живёт в трее).\n");
    p.push_str("  {\n");
    p.push_str("    \"schedules\": [{\n");
    p.push_str("      \"id\": \"morning-digest\",\n");
    p.push_str("      \"name\": \"Утренний дайджест\",\n");
    p.push_str("      \"cron\": \"0 0 8 * * *\",          // 6 полей: sec min hour dom month dow (UTC). \"0 */5 * * * *\" = каждые 5 минут\n");
    p.push_str("      \"enabled\": true,\n");
    p.push_str("      \"catch_up\": \"once\",              // если Reflex был выключен — выполнить ОДИН раз при старте\n");
    p.push_str("      \"steps\": [\n");
    p.push_str("        { \"method\": \"net.fetch\",  \"params\": {\"url\":\"...\"},                          \"save_as\": \"page\"    },\n");
    p.push_str("        { \"method\": \"agent.task\", \"params\": {\"prompt\":\"Суммируй: {{steps.page.body}}\"}, \"save_as\": \"summary\" },\n");
    p.push_str("        { \"method\": \"storage.set\",\"params\": {\"key\":\"today\", \"value\":\"{{steps.summary.result}}\"} }\n");
    p.push_str("      ]\n");
    p.push_str("    }]\n");
    p.push_str("  }\n");
    p.push_str("- Шаги исполняются по очереди. Шаблоны {{steps.X.field}} подставляют результаты предыдущих шагов. Если плейсхолдер занимает всю строку — тип значения сохраняется (объект остаётся объектом).\n");
    p.push_str("- В steps НЕЛЬЗЯ использовать dialog.openDirectory/openFile/saveFile — у автоматизаций нет UI.\n");
    p.push_str("- Все остальные методы (agent.*, storage.*, fs.*, net.fetch, notify.show, events.*, apps.invoke, memory.*, manifest.*, scheduler.list/runs/runDetail) работают как обычно. scheduler.runNow/setPaused в schedule.steps заблокированы, чтобы не запускать рекурсивные unattended-циклы.\n");
    p.push_str("- Если задача звучит как «раз в N минут/часов делать X» — это schedule, не кнопка в UI.\n\n");

    p.push_str("MANIFEST.actions — публичные операции, которые могут вызывать ДРУГИЕ apps через apps.invoke.\n");
    p.push_str("  {\n");
    p.push_str("    \"actions\": [{\n");
    p.push_str("      \"id\": \"today-summary\",\n");
    p.push_str("      \"name\": \"Сводка за сегодня\",\n");
    p.push_str("      \"public\": true,                   // если false — caller должен иметь permission \"apps.invoke:<this_app_id>\"\n");
    p.push_str("      \"steps\": [\n");
    p.push_str("        { \"method\": \"storage.get\", \"params\": {\"key\":\"today\"}, \"save_as\": \"output\" }\n");
    p.push_str("      ]\n");
    p.push_str("    }]\n");
    p.push_str("  }\n");
    p.push_str("- Параметры от вызывающего доступны как {{input.X}}.\n");
    p.push_str("- Возврат action — значение последнего шага (или save_as: \"output\" если хочешь явно).\n\n");

    p.push_str("MANIFEST.widgets — мини-страницы для дашборда проекта (компактные, читают/показывают данные).\n");
    p.push_str("  {\n");
    p.push_str("    \"widgets\": [{\n");
    p.push_str("      \"id\": \"today\",\n");
    p.push_str("      \"name\": \"Сегодня\",\n");
    p.push_str("      \"entry\": \"widgets/today.html\",\n");
    p.push_str("      \"size\": \"small\",         // small (1x1), medium (2x1), wide (3x1), large (2x2). Базовая клетка ~180px.\n");
    p.push_str("      \"description\": \"что показывает виджет\"\n");
    p.push_str("    }]\n");
    p.push_str("  }\n");
    p.push_str("- Каждый widget.entry — отдельный HTML-файл в папке app, обычно `widgets/<id>.html`.\n");
    p.push_str("- Внутри виджета доступен тот же bridge и runtime overlay (reflexInvoke, reflexAgent*/Storage/Fs/Net/Dialog/Notify helpers, reflexSystemContext, reflexManifestGet, reflexProjectsList, reflexTopicsList, reflexBrowser* helpers, reflexSchedulerList/RunNow/SetPaused/Runs, reflexMemorySave/List/Search/Recall/PathStatus helpers, reflexEventOn/Emit, reflexAppsInvoke, reflexAppsListActions).\n");
    p.push_str("- Виджет компактный: тёмная прозрачная подложка (background:transparent), html/body высотой 100%, padding 12-14px, без своих рамок (рамки рисует grid).\n");
    p.push_str("- Если данные обновляются часто — сам ставь setInterval на 5-30 сек.\n");
    p.push_str("- Если виджет читает данные другой утилиты — используй reflexAppsInvoke('<app>','<action>',{...}); НЕ дублируй сбор данных.\n\n");

    p.push_str("INTER-APP EVENTS И ВЫЗОВЫ:\n");
    p.push_str("  events.emit({topic, payload})            — публикация события всем подписчикам\n");
    p.push_str("  events.subscribe({topics: [\"...\"]})       — подписка. \"*\" = любой топик\n");
    p.push_str("  events.unsubscribe({topics: [...]})\n");
    p.push_str("  apps.invoke({app_id, action_id, params}) -> {ok, run_id, result}\n");
    p.push_str("  apps.list_actions({app_id?, include_steps?}) — что можно вызвать\n");
    p.push_str("В iframe runtime overlay уже есть helpers, можно звать напрямую (без postMessage):\n");
    p.push_str("  window.reflexEventOn(topic, (data, fromApp) => {...})    // подпишется и сохранит handler\n");
    p.push_str("  window.reflexEventOff(topic)\n");
    p.push_str("  window.reflexEventEmit(topic, payload)\n");
    p.push_str("  window.reflexInvoke(method, params)                      // универсальный вызов bridge\n");
    p.push_str("  window.reflexSystemContext()\n");
    p.push_str("  window.reflexManifestGet(), reflexManifestUpdate(patch)\n");
    p.push_str("  window.reflexAgentAsk(promptOrParams), reflexAgentTask(promptOrParams), reflexAgentStream(promptOrParams), reflexAgentStreamAbort(threadIdOrParams)\n");
    p.push_str("  window.reflexStorageGet(keyOrParams), reflexStorageSet(keyOrParams, value?)\n");
    p.push_str("  window.reflexFsRead(pathOrParams), reflexFsWrite(pathOrParams, content?)\n");
    p.push_str("  window.reflexNetFetch(urlOrParams, options?), reflexNotifyShow(titleOrParams, body?)\n");
    p.push_str("  window.reflexDialogOpenDirectory(params), reflexDialogOpenFile(params), reflexDialogSaveFile(params)\n");
    p.push_str("  window.reflexProjectsList(params), reflexTopicsList(params)\n");
    p.push_str("  window.reflexBrowserInit(params), reflexBrowserTabs(), reflexBrowserOpen(url), reflexBrowserNavigate(tabId, url)\n");
    p.push_str("  window.reflexBrowserReadText(tabId), reflexBrowserReadOutline(tabId), reflexBrowserScreenshot(tabIdOrParams, fullPage?)\n");
    p.push_str("  window.reflexBrowserClickText(tabIdOrParams, text?, exact?), reflexBrowserClickSelector(tabIdOrParams, selector?), reflexBrowserFill(tabIdOrParams, selector?, value?)\n");
    p.push_str("  window.reflexSchedulerList(params), reflexSchedulerRunNow(scheduleId), reflexSchedulerSetPaused(scheduleId, paused), reflexSchedulerRuns(params)\n");
    p.push_str("  window.reflexMemorySave(params), reflexMemoryList(params), reflexMemoryDelete(relPathOrParams)\n");
    p.push_str("  window.reflexMemorySearch(queryOrParams), reflexMemoryRecall(queryOrParams)\n");
    p.push_str("  window.reflexMemoryIndexPath(pathOrParams), reflexMemoryPathStatus(pathOrParams), reflexMemoryForgetPath(pathOrParams)\n");
    p.push_str("  window.reflexAppsInvoke(appId, actionId, params), reflexAppsListActions(appIdOrParams, includeSteps?)\n");
    p.push_str("Permissions для apps.invoke декларируется в manifest.permissions:\n");
    p.push_str("  [\"apps.invoke:*\"]                       — звать ЛЮБОЕ action ЛЮБОГО app\n");
    p.push_str("  [\"apps.invoke:health-stats\"]            — только конкретный app\n");
    p.push_str("  [\"apps.invoke:health-stats::today\"]     — только конкретный action\n");
    p.push_str("- Если данные уже есть в другом app — НЕ дублируй их сбор. Вызывай его action через apps.invoke, либо слушай его события.\n\n");

    p.push_str("ОГРАНИЧЕНИЯ:\n");
    p.push_str("- iframe sandbox=\"allow-scripts allow-forms\" (для server-runtime добавляется allow-same-origin). Сетевые fetch к произвольным внешним URL могут не работать — для динамических данных используй agent.ask или свой server-runtime.\n");
    p.push_str("- В schedule.steps нельзя использовать dialog.*: эти шаги бегут без UI.\n\n");
    if let Some(skeleton) = template_skeleton(template) {
        p.push_str("ШАБЛОН:\n");
        p.push_str(skeleton);
        p.push('\n');
    }
    p.push_str("ЗАДАЧА: ");
    p.push_str(description);
    p.push_str("\n\nВ конце: рабочие файлы + обновлённый manifest.json. Не трогай .reflex/.\n");
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

#[tauri::command]
fn link_app_to_project(
    app: AppHandle,
    project_id: String,
    app_id: String,
) -> Result<project::Project, String> {
    let mut p = project::get_by_id(&app, &project_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("project not found: {project_id}"))?;
    if !p.apps.contains(&app_id) {
        p.apps.push(app_id);
    }
    project::write_project(&PathBuf::from(&p.root), &p).map_err(|e| e.to_string())?;
    project::register(&app, &p).map_err(|e| e.to_string())?;
    Ok(p)
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
) -> Result<String, String> {
    submit_quick_impl(app, prompt, ctx, project_id, plan_mode, source, browser_tabs)
}

pub(crate) fn submit_quick_impl(
    app: AppHandle,
    prompt: String,
    ctx: QuickContext,
    project_id: Option<String>,
    plan_mode: Option<bool>,
    source: Option<String>,
    browser_tabs: Option<Vec<storage::BrowserTab>>,
) -> Result<String, String> {
    let plan_mode = plan_mode.unwrap_or(false);
    let source = source
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "quick".into());
    let browser_tabs = browser_tabs.unwrap_or_default();
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
        goal: None,
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
        let mut buf = String::from("Контекст из встроенного браузера (открытые вкладки на момент запуска):\n");
        for (i, tab) in browser_tabs.iter().enumerate() {
            let title = if tab.title.trim().is_empty() {
                "(без заголовка)"
            } else {
                tab.title.trim()
            };
            buf.push_str(&format!("{}. {} — {}\n", i + 1, title, tab.url));
        }
        buf.push_str("\nЗАДАЧА:\n");
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
        if let Err(e) = server.turn_start(&app_thread_id, &codex_prompt).await {
            eprintln!("[reflex] turn_start failed: {e}");
        }
    });

    Ok(thread_id)
}

#[tauri::command]
fn continue_thread(
    app: AppHandle,
    project_id: String,
    thread_id: String,
    prompt: String,
    plan_confirmed: Option<bool>,
) -> Result<(), String> {
    eprintln!(
        "[reflex] continue_thread: project={project_id} thread={thread_id} prompt_len={}",
        prompt.len()
    );
    let trimmed = prompt.trim();
    if trimmed.is_empty() {
        return Err("empty prompt".into());
    }

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

        let mut turn_result = server.turn_start(&sid, &prompt_owned).await;

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
                    turn_result = server.turn_start(&sid, &prompt_owned).await;
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

    const RESUME_PROMPT: &str = "⟲ Reflex was restarted. Продолжай с того места, на котором остановился. Если задача уже выполнена — кратко сообщи об этом.";

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
            reveal_in_finder,
            continue_thread,
            stop_thread,
            respond_to_question,
            memory::tools::memory_save,
            memory::tools::memory_list,
            memory::tools::memory_delete,
            memory::tools::memory_search,
            memory::tools::memory_recall,
            memory::tools::memory_reindex,
            memory::tools::memory_index_path,
            memory::tools::memory_path_status,
            memory::tools::memory_path_status_batch,
            memory::tools::memory_forget_path,
            scheduler::commands::scheduler_list,
            scheduler::commands::scheduler_set_paused,
            scheduler::commands::scheduler_run_now,
            scheduler::commands::scheduler_runs,
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
