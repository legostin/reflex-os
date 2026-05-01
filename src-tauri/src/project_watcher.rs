use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use tauri::{AppHandle, Emitter};

use crate::project;

const IGNORED_DIRS: &[&str] = &[
    ".reflex",
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    ".turbo",
];
const IGNORED_FILE_NAMES: &[&str] = &[".DS_Store"];
const IGNORED_FILE_EXTS: &[&str] = &["log", "tmp", "swp"];

pub struct WatcherEntry {
    #[allow(dead_code)]
    pub debouncer: Debouncer<notify::RecommendedWatcher>,
    pub ref_count: usize,
}

#[derive(Default)]
pub struct ProjectWatchers {
    pub map: Arc<Mutex<HashMap<String, WatcherEntry>>>,
}

fn is_ignored(path: &Path, root: &Path) -> bool {
    if let Ok(rel) = path.strip_prefix(root) {
        for comp in rel.components() {
            let s = comp.as_os_str().to_string_lossy();
            if IGNORED_DIRS.iter().any(|n| s == *n) {
                return true;
            }
        }
    }
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if IGNORED_FILE_NAMES.iter().any(|n| name == *n) {
            return true;
        }
    }
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        let ext = ext.to_lowercase();
        if IGNORED_FILE_EXTS.iter().any(|e| ext == *e) {
            return true;
        }
    }
    false
}

pub fn start(
    watchers: &ProjectWatchers,
    app: &AppHandle,
    project_id: &str,
) -> Result<(), String> {
    let mut map = watchers.map.lock().map_err(|e| e.to_string())?;
    if let Some(entry) = map.get_mut(project_id) {
        entry.ref_count += 1;
        return Ok(());
    }

    let proj = project::get_by_id(app, project_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("project not found: {project_id}"))?;
    let root = PathBuf::from(&proj.root);
    if !root.exists() {
        return Err(format!("project root missing: {}", root.display()));
    }

    let app_handle = app.clone();
    let id_for_cb = project_id.to_string();
    let root_for_cb = root.clone();

    let mut debouncer = new_debouncer(
        Duration::from_millis(300),
        move |result: DebounceEventResult| match result {
            Ok(events) => {
                let mut paths: Vec<String> = Vec::new();
                for ev in events {
                    if is_ignored(&ev.path, &root_for_cb) {
                        continue;
                    }
                    paths.push(ev.path.to_string_lossy().into_owned());
                }
                if paths.is_empty() {
                    return;
                }
                let _ = app_handle.emit(
                    "reflex://project-files-changed",
                    &serde_json::json!({
                        "project_id": id_for_cb,
                        "paths": paths,
                    }),
                );
            }
            Err(errors) => {
                eprintln!("[reflex] project watcher errors for {id_for_cb}: {errors:?}");
            }
        },
    )
    .map_err(|e| format!("watcher init failed: {e}"))?;

    debouncer
        .watcher()
        .watch(&root, RecursiveMode::Recursive)
        .map_err(|e| format!("watcher watch failed: {e}"))?;

    map.insert(
        project_id.to_string(),
        WatcherEntry {
            debouncer,
            ref_count: 1,
        },
    );
    Ok(())
}

pub fn stop(watchers: &ProjectWatchers, project_id: &str) {
    let mut map = match watchers.map.lock() {
        Ok(m) => m,
        Err(_) => return,
    };
    let drop_now = match map.get_mut(project_id) {
        Some(entry) => {
            entry.ref_count = entry.ref_count.saturating_sub(1);
            entry.ref_count == 0
        }
        None => false,
    };
    if drop_now {
        map.remove(project_id);
    }
}
