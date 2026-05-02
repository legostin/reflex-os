use crate::apps::Step;
use crate::apps_dispatch;
use crate::scheduler::state::{
    self, append_run, build_step_preview, output_preview_limit, write_full_run, RunRecord, StepTrace,
};
use crate::scheduler::{now_ms, templating, SchedulerHandle};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

const MAX_RECURSION_DEPTH: usize = 8;
const SCHEDULE_STEP_METHOD_BLACKLIST: &[&str] = &[
    "apps.open",
    "dialog.openDirectory",
    "dialog.openFile",
    "dialog.saveFile",
    "scheduler.runNow",
    "scheduler.run_now",
    "scheduler.setPaused",
    "scheduler.set_paused",
    "system.openPath",
    "system.openUrl",
    "system.open_path",
    "system.open_url",
    "system.revealPath",
    "system.reveal_path",
];

tokio::task_local! {
    static INVOKE_DEPTH: usize;
}

#[derive(Clone, Debug)]
pub enum WorkflowCaller {
    Scheduler { schedule_id: String },
    InterApp { from: String, action_id: String },
    Manual { source: String },
}

impl WorkflowCaller {
    pub fn tag(&self) -> &'static str {
        match self {
            WorkflowCaller::Scheduler { .. } => "scheduler",
            WorkflowCaller::InterApp { .. } => "inter_app",
            WorkflowCaller::Manual { .. } => "manual",
        }
    }
    pub fn schedule_id(&self) -> Option<String> {
        match self {
            WorkflowCaller::Scheduler { schedule_id } => Some(schedule_id.clone()),
            _ => None,
        }
    }
    pub fn action_id(&self) -> Option<String> {
        match self {
            WorkflowCaller::InterApp { action_id, .. } => Some(action_id.clone()),
            _ => None,
        }
    }
}

pub fn current_depth() -> usize {
    INVOKE_DEPTH.try_with(|d| *d).unwrap_or(0)
}

pub async fn run_workflow(
    app: AppHandle,
    handle: SchedulerHandle,
    invoker_app_id: String,
    caller: WorkflowCaller,
    steps: Vec<Step>,
    initial_input: Option<Value>,
) -> RunRecord {
    let depth = current_depth();
    if depth >= MAX_RECURSION_DEPTH {
        let now = now_ms();
        let r = RunRecord {
            run_id: format!("run_{}", uuid::Uuid::new_v4().simple()),
            app_id: invoker_app_id,
            schedule_id: caller.schedule_id(),
            action_id: caller.action_id(),
            caller: caller.tag().to_string(),
            started_ms: now,
            ended_ms: Some(now),
            status: "error".into(),
            steps: Vec::new(),
            error: Some(format!(
                "recursion guard tripped at depth {depth}"
            )),
        };
        finalize(&app, &handle, &r);
        return r;
    }

    let next_depth = depth + 1;
    INVOKE_DEPTH
        .scope(next_depth, async move {
            run_workflow_inner(app, handle, invoker_app_id, caller, steps, initial_input).await
        })
        .await
}

async fn run_workflow_inner(
    app: AppHandle,
    handle: SchedulerHandle,
    invoker_app_id: String,
    caller: WorkflowCaller,
    steps: Vec<Step>,
    initial_input: Option<Value>,
) -> RunRecord {
    let run_id = format!("run_{}", uuid::Uuid::new_v4().simple());
    let started_ms = now_ms();

    let mut record = RunRecord {
        run_id: run_id.clone(),
        app_id: invoker_app_id.clone(),
        schedule_id: caller.schedule_id(),
        action_id: caller.action_id(),
        caller: caller.tag().to_string(),
        started_ms,
        ended_ms: None,
        status: "started".into(),
        steps: Vec::new(),
        error: None,
    };

    let _ = app.emit(
        "reflex://scheduler-fire-started",
        &json!({
            "run_id": run_id,
            "app_id": invoker_app_id,
            "schedule_id": caller.schedule_id(),
            "action_id": caller.action_id(),
            "caller": caller.tag(),
            "started_ms": started_ms,
        }),
    );

    let mut ctx = json!({
        "steps": {},
        "input": initial_input.unwrap_or(Value::Null),
    });

    let block_unattended_methods = matches!(caller, WorkflowCaller::Scheduler { .. });
    let mut last_output: Value = Value::Null;
    let mut errored = false;

    for (idx, step) in steps.iter().enumerate() {
        let step_name = step
            .save_as
            .clone()
            .unwrap_or_else(|| format!("step_{idx}"));

        if block_unattended_methods
            && SCHEDULE_STEP_METHOD_BLACKLIST.contains(&step.method.as_str())
        {
            let now = now_ms();
            record.steps.push(StepTrace {
                name: step_name.clone(),
                method: step.method.clone(),
                status: "error".into(),
                started_ms: now,
                ended_ms: now,
                output_preview: None,
                output_size: 0,
                error: Some(format!(
                    "method '{}' not allowed in scheduler workflows",
                    step.method
                )),
            });
            record.status = "error".into();
            record.error = Some("method blocked".into());
            errored = true;
            break;
        }

        let rendered = templating::render(&step.params, &ctx);
        let started = now_ms();
        let result = apps_dispatch::dispatch_app_method(
            &app,
            &invoker_app_id,
            &step.method,
            rendered.clone(),
        )
        .await;
        let ended = now_ms();

        match result {
            Ok(value) => {
                let (preview, size) = build_step_preview(&value);
                record.steps.push(StepTrace {
                    name: step_name.clone(),
                    method: step.method.clone(),
                    status: "ok".into(),
                    started_ms: started,
                    ended_ms: ended,
                    output_preview: preview,
                    output_size: size,
                    error: None,
                });
                if let Some(name) = &step.save_as {
                    if let Some(map) = ctx
                        .get_mut("steps")
                        .and_then(|s| s.as_object_mut())
                    {
                        map.insert(name.clone(), value.clone());
                    }
                }
                last_output = value;
            }
            Err(e) => {
                record.steps.push(StepTrace {
                    name: step_name.clone(),
                    method: step.method.clone(),
                    status: "error".into(),
                    started_ms: started,
                    ended_ms: ended,
                    output_preview: None,
                    output_size: 0,
                    error: Some(e.clone()),
                });
                record.status = "error".into();
                record.error = Some(e);
                errored = true;
                break;
            }
        }
    }

    record.ended_ms = Some(now_ms());
    if !errored {
        record.status = "ok".into();
    }

    finalize(&app, &handle, &record);

    if let WorkflowCaller::Scheduler { schedule_id } = &caller {
        let _ = update_state_after_fire(&app, &handle, schedule_id, &record).await;
    }

    let _ = app.emit(
        "reflex://scheduler-fire-finished",
        &json!({
            "run_id": record.run_id,
            "app_id": record.app_id,
            "schedule_id": record.schedule_id,
            "action_id": record.action_id,
            "status": record.status,
            "ended_ms": record.ended_ms,
            "duration_ms": record.ended_ms.map(|e| e.saturating_sub(record.started_ms)),
            "error": record.error,
        }),
    );

    let _ = last_output;
    record
}

fn finalize(app: &AppHandle, _handle: &SchedulerHandle, record: &RunRecord) {
    let total_size: usize = record.steps.iter().map(|s| s.output_size).sum();
    if total_size > output_preview_limit() * 2 {
        let _ = write_full_run(app, record);
    }
    if let Err(e) = append_run(app, record) {
        eprintln!("[scheduler] append_run failed: {e}");
    }
}

async fn update_state_after_fire(
    app: &AppHandle,
    handle: &SchedulerHandle,
    schedule_id: &str,
    record: &RunRecord,
) -> std::io::Result<()> {
    let _guard = handle.inner.state_lock.lock().await;
    let mut s = state::load_state(app)?;
    let entry = s.schedules.entry(schedule_id.to_string()).or_default();
    entry.last_fire_at_ms = record.ended_ms.unwrap_or(record.started_ms);
    entry.last_run_id = Some(record.run_id.clone());
    state::save_state(app, &s)
}

pub fn last_step_value(record: &RunRecord) -> Value {
    record
        .steps
        .last()
        .and_then(|s| s.output_preview.as_ref())
        .map(|p| serde_json::from_str(p).unwrap_or_else(|_| Value::String(p.clone())))
        .unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheduler_blacklist_blocks_ui_only_methods() {
        for method in [
            "apps.open",
            "dialog.openDirectory",
            "dialog.openFile",
            "dialog.saveFile",
            "scheduler.runNow",
            "scheduler.run_now",
            "scheduler.setPaused",
            "scheduler.set_paused",
            "system.openPath",
            "system.openUrl",
            "system.open_path",
            "system.open_url",
            "system.revealPath",
            "system.reveal_path",
        ] {
            assert!(
                SCHEDULE_STEP_METHOD_BLACKLIST.contains(&method),
                "{method} should be blocked in unattended scheduler workflows"
            );
        }
    }
}
