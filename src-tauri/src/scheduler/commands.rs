use crate::scheduler::engine::spawn_fire;
use crate::scheduler::manifest::{collect_app_schedules, find_schedule};
use crate::scheduler::state::{self, RunRecord};
use crate::scheduler::{make_full_id, split_full_id, SchedulerHandle};
use chrono::Utc;
use cron::Schedule;
use serde::Serialize;
use std::str::FromStr;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Serialize, Clone, Debug)]
pub struct ScheduleListItem {
    pub schedule_id: String,
    pub app_id: String,
    pub app_name: String,
    pub name: String,
    pub cron: String,
    pub enabled: bool,
    pub paused: bool,
    pub valid: bool,
    pub next_fire_ms: Option<i64>,
    pub last_fire_at_ms: u64,
    pub last_run_id: Option<String>,
    pub steps_count: usize,
}

#[derive(Serialize, Clone, Debug)]
pub struct RunSummary {
    pub run_id: String,
    pub app_id: String,
    pub schedule_id: Option<String>,
    pub action_id: Option<String>,
    pub caller: String,
    pub status: String,
    pub started_ms: u64,
    pub ended_ms: Option<u64>,
    pub error_preview: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct ScheduleStats {
    pub total: usize,
    pub enabled: usize,
    pub active: usize,
    pub paused: usize,
    pub invalid: usize,
    pub next_fire_ms: Option<i64>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct LastRunErrorSummary {
    pub run_id: String,
    pub app_id: String,
    pub schedule_id: Option<String>,
    pub action_id: Option<String>,
    pub started_ms: u64,
    pub error_preview: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct RecentRunStats {
    pub sample: usize,
    pub ok: usize,
    pub error: usize,
    pub last_error: Option<LastRunErrorSummary>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct SchedulerStats {
    pub schedules: ScheduleStats,
    pub recent_runs: RecentRunStats,
}

#[tauri::command]
pub fn scheduler_list(app: AppHandle) -> Result<Vec<ScheduleListItem>, String> {
    let schedules = collect_app_schedules(&app);
    let state = state::load_state(&app).map_err(|e| e.to_string())?;
    let now = Utc::now();

    let listings = crate::apps::list_apps(&app).unwrap_or_default();
    let app_name_for = |id: &str| -> String {
        listings
            .iter()
            .find(|l| l.manifest.id == id)
            .map(|l| l.manifest.name.clone())
            .unwrap_or_else(|| id.to_string())
    };

    let mut out = Vec::with_capacity(schedules.len());
    for s in schedules {
        let full_id = make_full_id(&s.app_id, &s.def.id);
        let entry = state.schedules.get(&full_id);
        let cron_ok = Schedule::from_str(&s.def.cron).ok();
        let next_ms = cron_ok
            .as_ref()
            .and_then(|c| c.after(&now).next())
            .map(|t| t.timestamp_millis());
        out.push(ScheduleListItem {
            schedule_id: full_id,
            app_id: s.app_id.clone(),
            app_name: app_name_for(&s.app_id),
            name: s.def.name,
            cron: s.def.cron.clone(),
            enabled: s.def.enabled,
            paused: entry.map(|e| e.paused).unwrap_or(false),
            valid: cron_ok.is_some(),
            next_fire_ms: next_ms,
            last_fire_at_ms: entry.map(|e| e.last_fire_at_ms).unwrap_or(0),
            last_run_id: entry.and_then(|e| e.last_run_id.clone()),
            steps_count: s.def.steps.len(),
        });
    }
    Ok(out)
}

pub fn summarize_scheduler_stats(
    schedules: &[ScheduleListItem],
    runs: &[RunSummary],
) -> SchedulerStats {
    let total = schedules.len();
    let enabled = schedules.iter().filter(|item| item.enabled).count();
    let paused = schedules.iter().filter(|item| item.paused).count();
    let invalid = schedules.iter().filter(|item| !item.valid).count();
    let active = schedules
        .iter()
        .filter(|item| item.enabled && item.valid && !item.paused)
        .count();
    let next_fire_ms = schedules
        .iter()
        .filter(|item| item.enabled && item.valid && !item.paused)
        .filter_map(|item| item.next_fire_ms)
        .min();
    let ok_runs = runs.iter().filter(|run| run.status == "ok").count();
    let error_runs = runs.iter().filter(|run| run.status == "error").count();
    let last_error = runs
        .iter()
        .find(|run| run.status == "error")
        .map(|run| LastRunErrorSummary {
            run_id: run.run_id.clone(),
            app_id: run.app_id.clone(),
            schedule_id: run.schedule_id.clone(),
            action_id: run.action_id.clone(),
            started_ms: run.started_ms,
            error_preview: run.error_preview.clone(),
        });

    SchedulerStats {
        schedules: ScheduleStats {
            total,
            enabled,
            active,
            paused,
            invalid,
            next_fire_ms,
        },
        recent_runs: RecentRunStats {
            sample: runs.len(),
            ok: ok_runs,
            error: error_runs,
            last_error,
        },
    }
}

#[tauri::command]
pub async fn scheduler_set_paused(
    app: AppHandle,
    schedule_id: String,
    paused: bool,
) -> Result<(), String> {
    let h: SchedulerHandle = app.state::<SchedulerHandle>().inner().clone();
    let _guard = h.inner.state_lock.lock().await;
    let mut s = state::load_state(&app).map_err(|e| e.to_string())?;
    let entry = s.schedules.entry(schedule_id.clone()).or_default();
    entry.paused = paused;
    state::save_state(&app, &s).map_err(|e| e.to_string())?;
    drop(_guard);
    h.rescan();
    let _ = app.emit(
        "reflex://scheduler-state-changed",
        &serde_json::json!({ "schedule_id": schedule_id, "paused": paused }),
    );
    Ok(())
}

#[tauri::command]
pub async fn scheduler_run_now(
    app: AppHandle,
    schedule_id: String,
) -> Result<String, String> {
    let (app_id, local_id) =
        split_full_id(&schedule_id).ok_or_else(|| "schedule_id must be <app>::<id>".to_string())?;
    let target = find_schedule(&app, app_id, local_id)
        .ok_or_else(|| format!("schedule not found: {schedule_id}"))?;
    let handle: SchedulerHandle = app.state::<SchedulerHandle>().inner().clone();
    spawn_fire(app.clone(), handle, target);
    Ok(schedule_id)
}

#[tauri::command]
pub fn scheduler_runs(
    app: AppHandle,
    limit: Option<usize>,
    before_ts: Option<u64>,
) -> Result<Vec<RunSummary>, String> {
    let limit = limit.unwrap_or(50).min(500);
    let recent = state::read_recent_runs(&app, limit, before_ts).map_err(|e| e.to_string())?;
    let out: Vec<RunSummary> = recent
        .into_iter()
        .map(|r| RunSummary {
            run_id: r.run_id,
            app_id: r.app_id,
            schedule_id: r.schedule_id,
            action_id: r.action_id,
            caller: r.caller,
            status: r.status,
            started_ms: r.started_ms,
            ended_ms: r.ended_ms,
            error_preview: r.error,
        })
        .collect();
    Ok(out)
}

#[tauri::command]
pub fn scheduler_stats(
    app: AppHandle,
    recent_limit: Option<usize>,
) -> Result<SchedulerStats, String> {
    let schedules = scheduler_list(app.clone())?;
    let runs = scheduler_runs(app, recent_limit, None)?;
    Ok(summarize_scheduler_stats(&schedules, &runs))
}

#[tauri::command]
pub fn scheduler_run_detail(app: AppHandle, run_id: String) -> Result<Option<RunRecord>, String> {
    state::read_run_by_id(&app, &run_id).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schedule(
        schedule_id: &str,
        enabled: bool,
        paused: bool,
        valid: bool,
        next_fire_ms: Option<i64>,
    ) -> ScheduleListItem {
        ScheduleListItem {
            schedule_id: schedule_id.to_string(),
            app_id: "app".to_string(),
            app_name: "App".to_string(),
            name: schedule_id.to_string(),
            cron: "* * * * * *".to_string(),
            enabled,
            paused,
            valid,
            next_fire_ms,
            last_fire_at_ms: 0,
            last_run_id: None,
            steps_count: 1,
        }
    }

    fn run(run_id: &str, status: &str, started_ms: u64) -> RunSummary {
        RunSummary {
            run_id: run_id.to_string(),
            app_id: "app".to_string(),
            schedule_id: Some("app::daily".to_string()),
            action_id: None,
            caller: "scheduler".to_string(),
            status: status.to_string(),
            started_ms,
            ended_ms: Some(started_ms + 10),
            error_preview: (status == "error").then(|| "failed".to_string()),
        }
    }

    #[test]
    fn summarize_scheduler_stats_counts_health_and_last_error() {
        let schedules = vec![
            schedule("active-later", true, false, true, Some(2_000)),
            schedule("active-sooner", true, false, true, Some(1_000)),
            schedule("paused", true, true, true, Some(500)),
            schedule("disabled", false, false, true, Some(250)),
            schedule("invalid", true, false, false, None),
        ];
        let runs = vec![
            run("err-new", "error", 40),
            run("ok", "ok", 30),
            run("err-old", "error", 20),
        ];

        let stats = summarize_scheduler_stats(&schedules, &runs);

        assert_eq!(stats.schedules.total, 5);
        assert_eq!(stats.schedules.enabled, 4);
        assert_eq!(stats.schedules.active, 2);
        assert_eq!(stats.schedules.paused, 1);
        assert_eq!(stats.schedules.invalid, 1);
        assert_eq!(stats.schedules.next_fire_ms, Some(1_000));
        assert_eq!(stats.recent_runs.sample, 3);
        assert_eq!(stats.recent_runs.ok, 1);
        assert_eq!(stats.recent_runs.error, 2);
        assert_eq!(
            stats
                .recent_runs
                .last_error
                .as_ref()
                .map(|run| run.run_id.as_str()),
            Some("err-new"),
        );
    }
}
