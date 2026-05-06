use crate::apps;
use std::io;
use tauri::AppHandle;

pub fn ensure(app: &AppHandle) -> io::Result<()> {
    apps::remove_legacy_builtin_apps(app)?;
    apps::ensure_system_app_folder(app)?;
    Ok(())
}
