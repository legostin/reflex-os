use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use notify::RecursiveMode;
use tauri::{AppHandle, Emitter};

use crate::apps;

const IGNORED_DIR_NAMES: &[&str] = &[".reflex", ".git", "node_modules"];
const IGNORED_FILE_NAMES: &[&str] = &[
    "manifest.json",
    "storage.json",
    "meta-llm.txt",
    ".DS_Store",
];
const IGNORED_FILE_EXTS: &[&str] = &["log", "tmp", "swp"];

pub struct WatcherEntry {
    // Holds the debouncer alive — drop releases the underlying watcher.
    #[allow(dead_code)]
    pub debouncer: Debouncer<notify::RecommendedWatcher>,
    pub ref_count: usize,
}

#[derive(Default)]
pub struct AppWatchers {
    pub map: Arc<Mutex<HashMap<String, WatcherEntry>>>,
}

fn is_ignored(path: &Path, app_dir: &Path) -> bool {
    if let Ok(rel) = path.strip_prefix(app_dir) {
        for comp in rel.components() {
            let s = comp.as_os_str().to_string_lossy();
            if IGNORED_DIR_NAMES.iter().any(|n| s == *n) {
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

/// Start watching app_dir. Refcount-based; safe to call multiple times.
pub fn start(
    watchers: &AppWatchers,
    app: &AppHandle,
    app_id: &str,
) -> Result<(), String> {
    let mut map = watchers.map.lock().map_err(|e| e.to_string())?;
    if let Some(entry) = map.get_mut(app_id) {
        entry.ref_count += 1;
        return Ok(());
    }

    let dir = apps::app_dir(app, app_id).map_err(|e| e.to_string())?;
    if !dir.exists() {
        return Err(format!("app dir does not exist: {}", dir.display()));
    }

    let app_handle = app.clone();
    let id_for_cb = app_id.to_string();
    let dir_for_cb = dir.clone();

    let mut debouncer = new_debouncer(
        Duration::from_millis(200),
        move |result: DebounceEventResult| match result {
            Ok(events) => {
                let mut paths: Vec<String> = Vec::new();
                for ev in events {
                    if is_ignored(&ev.path, &dir_for_cb) {
                        continue;
                    }
                    paths.push(ev.path.to_string_lossy().into_owned());
                }
                if paths.is_empty() {
                    return;
                }
                let _ = app_handle.emit(
                    "reflex://app-files-changed",
                    &serde_json::json!({
                        "app_id": id_for_cb,
                        "paths": paths,
                    }),
                );
            }
            Err(errors) => {
                eprintln!("[reflex] watcher errors for {id_for_cb}: {errors:?}");
            }
        },
    )
    .map_err(|e| format!("watcher init failed: {e}"))?;

    debouncer
        .watcher()
        .watch(&dir, RecursiveMode::Recursive)
        .map_err(|e| format!("watcher watch failed: {e}"))?;

    map.insert(
        app_id.to_string(),
        WatcherEntry {
            debouncer,
            ref_count: 1,
        },
    );
    Ok(())
}

pub fn stop(watchers: &AppWatchers, app_id: &str) {
    let mut map = match watchers.map.lock() {
        Ok(m) => m,
        Err(_) => return,
    };
    let drop_now = match map.get_mut(app_id) {
        Some(entry) => {
            entry.ref_count = entry.ref_count.saturating_sub(1);
            entry.ref_count == 0
        }
        None => false,
    };
    if drop_now {
        // Drop releases the watcher.
        map.remove(app_id);
    }
}

#[cfg(test)]
mod tests {
    use super::is_ignored;
    use std::path::Path;

    #[test]
    fn watcher_ignores_manifest_changes() {
        let app_dir = Path::new("/tmp/reflex-app");

        assert!(is_ignored(&app_dir.join("manifest.json"), app_dir));
        assert!(is_ignored(&app_dir.join("storage.json"), app_dir));
        assert!(!is_ignored(&app_dir.join("server.js"), app_dir));
    }
}
