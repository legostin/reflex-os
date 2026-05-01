use crate::apps::{self, ActionDef, ScheduleDef};
use tauri::AppHandle;

#[derive(Clone, Debug)]
pub struct AppSchedule {
    pub app_id: String,
    pub def: ScheduleDef,
}

#[derive(Clone, Debug)]
pub struct AppAction {
    pub app_id: String,
    pub def: ActionDef,
}

pub fn collect_app_schedules(app: &AppHandle) -> Vec<AppSchedule> {
    let mut out = Vec::new();
    let listings = match apps::list_apps(app) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[scheduler] list_apps failed: {e}");
            return out;
        }
    };
    for listing in listings {
        let manifest = match apps::read_manifest(app, &listing.manifest.id) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("[scheduler] read_manifest({}): {e}", listing.manifest.id);
                continue;
            }
        };
        for def in manifest.schedules {
            out.push(AppSchedule {
                app_id: listing.manifest.id.clone(),
                def,
            });
        }
    }
    out
}

pub fn collect_app_actions(app: &AppHandle) -> Vec<AppAction> {
    let mut out = Vec::new();
    let listings = match apps::list_apps(app) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[scheduler] list_apps failed: {e}");
            return out;
        }
    };
    for listing in listings {
        let manifest = match apps::read_manifest(app, &listing.manifest.id) {
            Ok(m) => m,
            Err(_) => continue,
        };
        for def in manifest.actions {
            out.push(AppAction {
                app_id: listing.manifest.id.clone(),
                def,
            });
        }
    }
    out
}

pub fn find_action(app: &AppHandle, target_app_id: &str, action_id: &str) -> Option<AppAction> {
    let manifest = apps::read_manifest(app, target_app_id).ok()?;
    let def = manifest.actions.into_iter().find(|a| a.id == action_id)?;
    Some(AppAction {
        app_id: target_app_id.to_string(),
        def,
    })
}

pub fn find_schedule(app: &AppHandle, target_app_id: &str, schedule_id: &str) -> Option<AppSchedule> {
    let manifest = apps::read_manifest(app, target_app_id).ok()?;
    let def = manifest.schedules.into_iter().find(|s| s.id == schedule_id)?;
    Some(AppSchedule {
        app_id: target_app_id.to_string(),
        def,
    })
}
