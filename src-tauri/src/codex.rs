use crate::storage;
use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;
use std::process::Stdio;
use tauri::{AppHandle, Emitter};
#[cfg(target_os = "macos")]
use tauri::Manager;
use tauri_plugin_notification::NotificationExt;
use tokio::process::Command as TokioCommand;

/// Notify the user that a thread finished. Public so app-server module can call it.
pub fn notify_thread_done_external(
    app: &AppHandle,
    project_root: &std::path::Path,
    thread_id: &str,
    exit_code: Option<i32>,
    cancelled: bool,
) {
    let meta = storage::read_meta(project_root, thread_id).ok();
    let label = match &meta {
        Some(m) => m
            .title
            .clone()
            .unwrap_or_else(|| m.prompt.chars().take(48).collect::<String>()),
        None => format!("Topic {thread_id}"),
    };
    let plan_ready = !cancelled
        && exit_code == Some(0)
        && meta
            .as_ref()
            .map(|m| m.plan_mode && !m.plan_confirmed)
            .unwrap_or(false);
    let title = if plan_ready {
        "Reflex - plan ready"
    } else {
        "Reflex"
    };
    let body = if plan_ready {
        format!("Plan ready · {label}")
    } else if cancelled {
        format!("✗ Cancelled · {label}")
    } else {
        match exit_code {
            Some(0) => format!("✓ Done · {label}"),
            Some(c) => format!("✗ Exit {c} · {label}"),
            None => format!("? Terminated · {label}"),
        }
    };

    #[cfg(target_os = "macos")]
    if plan_ready {
        let app_handle = app.clone();
        let title_owned = title.to_string();
        let body_owned = body.clone();
        let thread_id_owned = thread_id.to_string();
        let project_id = meta.and_then(|m| m.project_id);
        std::thread::spawn(move || {
            let mut notification = mac_notification_sys::Notification::new();
            let response = notification
                .title(&title_owned)
                .message(&body_owned)
                .wait_for_click(true)
                .send();
            match response {
                Ok(mac_notification_sys::NotificationResponse::Click)
                | Ok(mac_notification_sys::NotificationResponse::ActionButton(_)) => {
                    open_thread_from_notification(&app_handle, project_id, thread_id_owned);
                }
                Ok(_) => {}
                Err(e) => eprintln!("[reflex] plan notification failed: {e}"),
            }
        });
        return;
    }

    if let Err(e) = app
        .notification()
        .builder()
        .title(title)
        .body(body)
        .show()
    {
        eprintln!("[reflex] notification failed: {e}");
    }
}

#[cfg(target_os = "macos")]
fn open_thread_from_notification(
    app: &AppHandle,
    project_id: Option<String>,
    thread_id: String,
) {
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.show();
        let _ = main.unminimize();
        let _ = main.set_focus();
    }
    let _ = app.emit(
        "reflex://topic-open-request",
        &serde_json::json!({
            "project_id": project_id,
            "thread_id": thread_id,
            "from_app": "notification",
        }),
    );
}

/// Recursive lookup for codex session id in a parsed JSONL event.
pub fn find_session_id(v: &Value) -> Option<String> {
    match v {
        Value::Object(obj) => {
            // codex CLI new format: {"type":"thread.started","thread_id":"<uuid>"}
            if obj.get("type").and_then(|t| t.as_str()) == Some("thread.started") {
                if let Some(Value::String(s)) = obj.get("thread_id") {
                    return Some(s.clone());
                }
            }
            // legacy / alternative keys
            if let Some(Value::String(s)) = obj.get("session_id") {
                return Some(s.clone());
            }
            if let Some(Value::String(s)) = obj.get("sessionId") {
                return Some(s.clone());
            }
            for val in obj.values() {
                if let Some(found) = find_session_id(val) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(arr) => arr.iter().find_map(find_session_id),
        _ => None,
    }
}

#[derive(Deserialize)]
struct GeneratedMeta {
    title: String,
    goal: String,
}

const META_PROMPT_TEMPLATE: &str = r#"You are a quick metadata generator. You MUST NOT run commands, read files, or modify files. Return STRICTLY valid JSON only, with no markdown fences and no explanation, in this shape:
{"title": "...", "goal": "..."}

title: 3-7 words in the source request language, no trailing period.
goal: 1 short sentence describing what the agent should do, in the source request language.

User request:
---
{prompt}
---"#;

/// Spawn a separate cheap codex run that returns `{title, goal}` for the topic.
/// Updates the topic's `meta.json` and emits `reflex://thread-meta-updated`.
pub async fn generate_topic_meta(
    app: AppHandle,
    project_root: PathBuf,
    thread_id: String,
    prompt: String,
) {
    let meta_prompt = META_PROMPT_TEMPLATE.replace("{prompt}", &prompt);

    let out_path = match storage::thread_dir(&project_root, &thread_id) {
        Ok(d) => d.join("meta-llm.txt"),
        Err(e) => {
            eprintln!("[reflex] meta gen dir err: {e}");
            return;
        }
    };
    let cwd_str = project_root.to_string_lossy().into_owned();
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
            &meta_prompt,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .output()
        .await;

    let output = match result {
        Ok(o) => o,
        Err(e) => {
            eprintln!("[reflex] meta gen spawn err: {e}");
            return;
        }
    };
    if !output.status.success() {
        eprintln!("[reflex] meta gen non-zero: {}", output.status);
        return;
    }

    let last_msg = match std::fs::read_to_string(&out_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[reflex] meta gen read failed: {e}");
            return;
        }
    };

    let parsed = match extract_meta_json(&last_msg) {
        Some(m) => m,
        None => {
            eprintln!("[reflex] meta gen parse failed: raw={last_msg}");
            return;
        }
    };

    let mut emitted_goal = parsed.goal.clone();
    if let Ok(mut meta) = storage::read_meta(&project_root, &thread_id) {
        meta.title = Some(parsed.title.clone());
        if meta.goal.is_none() {
            meta.goal = Some(parsed.goal.clone());
        }
        emitted_goal = meta.goal.clone().unwrap_or_else(|| parsed.goal.clone());
        if let Err(e) = storage::write_meta(&project_root, &meta) {
            eprintln!("[reflex] meta write failed: {e}");
        }
    }

    let _ = app.emit(
        "reflex://thread-meta-updated",
        &serde_json::json!({
            "thread_id": thread_id,
            "title": parsed.title,
            "goal": emitted_goal,
        }),
    );

    let _ = std::fs::remove_file(&out_path);
}

fn extract_meta_json(s: &str) -> Option<GeneratedMeta> {
    if let Ok(m) = serde_json::from_str::<GeneratedMeta>(s.trim()) {
        return Some(m);
    }
    let start = s.find('{')?;
    let end = s.rfind('}')?;
    if end <= start {
        return None;
    }
    serde_json::from_str(&s[start..=end]).ok()
}
