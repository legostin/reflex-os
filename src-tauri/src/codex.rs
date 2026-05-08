use crate::storage;
use serde::Deserialize;
use serde_json::Value;
use std::env;
use std::path::PathBuf;
use std::process::{Command as StdCommand, Stdio};
use std::sync::OnceLock;
#[cfg(target_os = "macos")]
use tauri::Manager;
use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;
use tokio::process::Command as TokioCommand;

static CODEX_BINARY: OnceLock<String> = OnceLock::new();

pub(crate) fn command() -> TokioCommand {
    let mut command = TokioCommand::new(binary_path());
    if let Some(path) = augmented_path() {
        command.env("PATH", path);
    }
    command
}

pub(crate) fn binary_path() -> &'static str {
    CODEX_BINARY.get_or_init(resolve_codex_binary).as_str()
}

fn resolve_codex_binary() -> String {
    if let Ok(path) = env::var("REFLEX_CODEX_BIN") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            eprintln!("[reflex] using REFLEX_CODEX_BIN={trimmed}");
            return trimmed.to_string();
        }
    }

    let mut candidates = Vec::new();
    if let Ok(path) = env::var("PATH") {
        for dir in env::split_paths(&path) {
            push_candidate(&mut candidates, dir.join("codex"));
        }
    }
    if let Ok(home) = env::var("HOME") {
        push_candidate(
            &mut candidates,
            PathBuf::from(&home).join(".npm-global/bin/codex"),
        );
        push_candidate(
            &mut candidates,
            PathBuf::from(&home).join(".local/bin/codex"),
        );
        push_candidate(&mut candidates, PathBuf::from(&home).join(".bun/bin/codex"));
    }
    push_candidate(&mut candidates, PathBuf::from("/opt/homebrew/bin/codex"));
    push_candidate(&mut candidates, PathBuf::from("/usr/local/bin/codex"));

    let selected = candidates
        .iter()
        .filter_map(|candidate| {
            codex_version(candidate).map(|version| (candidate.clone(), version))
        })
        .max_by(|(_, left), (_, right)| left.cmp(right));

    if let Some((path, version)) = selected {
        eprintln!(
            "[reflex] using codex {}.{}.{} at {}",
            version.0, version.1, version.2, path
        );
        return path;
    }

    candidates
        .into_iter()
        .next()
        .unwrap_or_else(|| "codex".to_string())
}

fn push_candidate(candidates: &mut Vec<String>, path: PathBuf) {
    if !path.is_file() {
        return;
    }
    let display = path.to_string_lossy().into_owned();
    if !candidates.iter().any(|existing| existing == &display) {
        candidates.push(display);
    }
}

fn codex_version(path: &str) -> Option<(u32, u32, u32)> {
    let output = StdCommand::new(path).arg("--version").output().ok()?;
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    parse_semver_like(&text)
}

fn parse_semver_like(text: &str) -> Option<(u32, u32, u32)> {
    text.split(|c: char| !c.is_ascii_digit() && c != '.')
        .filter_map(|chunk| {
            let mut parts = chunk.split('.');
            let major = parts.next()?.parse::<u32>().ok()?;
            let minor = parts.next()?.parse::<u32>().ok()?;
            let patch = parts
                .next()
                .and_then(|value| value.parse::<u32>().ok())
                .unwrap_or(0);
            Some((major, minor, patch))
        })
        .max()
}

fn augmented_path() -> Option<String> {
    let mut paths: Vec<PathBuf> = Vec::new();
    if let Ok(home) = env::var("HOME") {
        paths.push(PathBuf::from(&home).join(".npm-global/bin"));
        paths.push(PathBuf::from(&home).join(".local/bin"));
        paths.push(PathBuf::from(&home).join(".bun/bin"));
    }
    paths.push(PathBuf::from("/opt/homebrew/bin"));
    paths.push(PathBuf::from("/usr/local/bin"));
    if let Ok(current) = env::var("PATH") {
        paths.extend(env::split_paths(&current));
    }

    let mut deduped: Vec<PathBuf> = Vec::new();
    for path in paths {
        if !deduped.iter().any(|existing| existing == &path) {
            deduped.push(path);
        }
    }
    env::join_paths(deduped)
        .ok()
        .and_then(|value| value.into_string().ok())
}

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

    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        eprintln!("[reflex] notification failed: {e}");
    }
}

#[cfg(target_os = "macos")]
fn open_thread_from_notification(app: &AppHandle, project_id: Option<String>, thread_id: String) {
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

    let overrides = crate::system_settings::thread_overrides(
        &app,
        crate::system_settings::RequestKind::Instant,
    );
    let mut args = vec![
        "exec".to_string(),
        "--json".to_string(),
        "--skip-git-repo-check".to_string(),
        "-s".to_string(),
        "read-only".to_string(),
        "--output-last-message".to_string(),
        out_str,
        "-C".to_string(),
        cwd_str,
    ];
    append_cli_overrides(&mut args, &overrides);
    args.push("--".to_string());
    args.push(meta_prompt);

    let result = command()
        .args(args)
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

pub(crate) fn append_cli_overrides(
    args: &mut Vec<String>,
    overrides: &crate::app_server::ThreadOverrides,
) {
    if let Some(model) = overrides.model.as_deref().filter(|s| !s.trim().is_empty()) {
        args.push("-m".into());
        args.push(model.to_string());
    }
    if let Some(effort) = overrides
        .reasoning_effort
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        args.push("-c".into());
        let value = serde_json::to_string(effort).unwrap_or_else(|_| format!("\"{effort}\""));
        args.push(format!("model_reasoning_effort={value}"));
    }
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

#[cfg(test)]
mod tests {
    use super::parse_semver_like;

    #[test]
    fn parses_codex_cli_version_with_warning() {
        let text = "WARNING: could not update PATH\ncodex-cli 0.128.0";
        assert_eq!(parse_semver_like(text), Some((0, 128, 0)));
    }

    #[test]
    fn keeps_highest_version_in_text() {
        let text = "old 0.101.0 new 0.128.0";
        assert_eq!(parse_semver_like(text), Some((0, 128, 0)));
    }
}
