use crate::scheduler::manifest::{collect_app_schedules, AppSchedule};
use crate::scheduler::runner::{run_workflow, WorkflowCaller};
use crate::scheduler::state::{self, SchedulerState};
use crate::scheduler::{make_full_id, now_ms, SchedulerHandle};
use chrono::{DateTime, TimeZone, Utc};
use cron::Schedule;
use std::str::FromStr;
use std::time::Duration;
use tauri::AppHandle;
use tokio::time::{sleep, sleep_until, Instant};

const HEARTBEAT_SECS: u64 = 60;

pub async fn run(app: AppHandle, handle: SchedulerHandle) {
    eprintln!("[scheduler] starting");
    let mut s = match state::load_state(&app) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[scheduler] load_state failed: {e}");
            SchedulerState::default()
        }
    };

    let schedules = collect_app_schedules(&app);
    catch_up_pass(&app, &handle, &mut s, &schedules).await;
    let _ = state::save_state(&app, &s);

    loop {
        let mut schedules = collect_app_schedules(&app);
        let state = state::load_state(&app).unwrap_or_default();
        let now_dt = Utc::now();
        let next = next_due(&schedules, &state, now_dt);

        let inner = handle.inner.clone();
        match next {
            Some((idx, due_at)) => {
                let wait_ms = (due_at.timestamp_millis() - now_ms() as i64).max(0) as u64;
                let deadline = Instant::now() + Duration::from_millis(wait_ms);
                tokio::select! {
                    _ = sleep_until(deadline) => {
                        let target = schedules.swap_remove(idx);
                        spawn_fire(app.clone(), handle.clone(), target);
                    }
                    _ = inner.rescan.notified() => continue,
                    _ = inner.cancel.notified() => {
                        eprintln!("[scheduler] cancel received, exiting");
                        return;
                    }
                }
            }
            None => {
                tokio::select! {
                    _ = sleep(Duration::from_secs(HEARTBEAT_SECS)) => {}
                    _ = inner.rescan.notified() => {}
                    _ = inner.cancel.notified() => {
                        eprintln!("[scheduler] cancel received, exiting");
                        return;
                    }
                }
            }
        }
    }
}

async fn catch_up_pass(
    app: &AppHandle,
    handle: &SchedulerHandle,
    state: &mut SchedulerState,
    schedules: &[AppSchedule],
) {
    let now = Utc::now();
    let now_ms_v = now_ms();
    for sched in schedules {
        if !sched.def.enabled {
            continue;
        }
        let full_id = make_full_id(&sched.app_id, &sched.def.id);
        let entry = state.schedules.entry(full_id.clone()).or_default();
        if entry.paused {
            continue;
        }
        let cron = match Schedule::from_str(&sched.def.cron) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let base = if entry.last_fire_at_ms == 0 {
            ms_to_dt(now_ms_v.saturating_sub(60_000))
        } else {
            ms_to_dt(entry.last_fire_at_ms)
        };
        let mut iter = cron.after(&base);
        let next_due = iter.next();
        if let Some(t) = next_due {
            if t <= now {
                eprintln!(
                    "[scheduler] catch-up fire: {} (was due {})",
                    full_id,
                    t.to_rfc3339()
                );
                entry.last_fire_at_ms = now_ms_v;
                spawn_fire(app.clone(), handle.clone(), sched.clone());
            }
        }
    }
}

fn ms_to_dt(ms: u64) -> DateTime<Utc> {
    Utc.timestamp_millis_opt(ms as i64).single().unwrap_or_else(Utc::now)
}

fn next_due(
    schedules: &[AppSchedule],
    state: &SchedulerState,
    now: DateTime<Utc>,
) -> Option<(usize, DateTime<Utc>)> {
    let mut best: Option<(usize, DateTime<Utc>)> = None;
    for (idx, s) in schedules.iter().enumerate() {
        if !s.def.enabled {
            continue;
        }
        let full_id = make_full_id(&s.app_id, &s.def.id);
        if state.schedules.get(&full_id).map(|e| e.paused).unwrap_or(false) {
            continue;
        }
        let cron = match Schedule::from_str(&s.def.cron) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let mut iter = cron.after(&now);
        let next = match iter.next() {
            Some(t) => t,
            None => continue,
        };
        if let Some((_, cur)) = best {
            if next < cur {
                best = Some((idx, next));
            }
        } else {
            best = Some((idx, next));
        }
    }
    best
}

pub fn spawn_fire(app: AppHandle, handle: SchedulerHandle, target: AppSchedule) {
    let full_id = make_full_id(&target.app_id, &target.def.id);
    let app_for_state = app.clone();
    let handle_for_state = handle.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = bump_last_fire(&app_for_state, &handle_for_state, &full_id).await {
            eprintln!("[scheduler] bump_last_fire failed: {e}");
        }
        run_workflow(
            app,
            handle,
            target.app_id.clone(),
            WorkflowCaller::Scheduler {
                schedule_id: full_id,
            },
            target.def.steps.clone(),
            None,
        )
        .await;
    });
}

async fn bump_last_fire(
    app: &AppHandle,
    handle: &SchedulerHandle,
    full_id: &str,
) -> std::io::Result<()> {
    let _guard = handle.inner.state_lock.lock().await;
    let mut s = state::load_state(app)?;
    let entry = s.schedules.entry(full_id.to_string()).or_default();
    entry.last_fire_at_ms = now_ms();
    state::save_state(app, &s)
}
