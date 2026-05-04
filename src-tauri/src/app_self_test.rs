use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::logs::LogLevel;
use crate::{app_runtime, apps, logs, storage};

pub fn spawn_after_successful_turn(app: AppHandle, project_root: PathBuf, thread_id: String) {
    tauri::async_runtime::spawn(async move {
        match should_self_test(&project_root, &thread_id) {
            Ok(Some(app_id)) => {
                if let Err(e) = run_for_app_id(app.clone(), &app_id).await {
                    eprintln!("[self-test] {app_id}: {e}");
                    logs::log_with(
                        &app,
                        LogLevel::Warn,
                        "app-self-test",
                        format!("{app_id}: {e}"),
                    );
                }
            }
            Ok(None) => {}
            Err(e) => eprintln!("[self-test] preflight failed: {e}"),
        }
    });
}

fn should_self_test(project_root: &Path, thread_id: &str) -> Result<Option<String>, String> {
    let manifest_path = project_root.join("manifest.json");
    if !manifest_path.is_file() {
        return Ok(None);
    }

    if let Ok(meta) = storage::read_meta(project_root, thread_id) {
        if meta.plan_mode && !meta.plan_confirmed {
            return Ok(None);
        }
    }

    let raw = std::fs::read_to_string(&manifest_path).map_err(|e| e.to_string())?;
    let manifest: apps::AppManifest = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    if manifest.id.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(manifest.id))
}

pub async fn run_for_app_id(
    app: AppHandle,
    app_id: &str,
) -> Result<apps::AppSelfTestStatus, String> {
    let started_at_ms = apps::timestamp_ms();
    let manifest = apps::read_manifest(&app, app_id).map_err(|e| e.to_string())?;
    let running = apps::AppSelfTestStatus {
        status: "running".into(),
        message: Some("Self-test is running".into()),
        started_at_ms: Some(started_at_ms),
        finished_at_ms: None,
        checks: Vec::new(),
    };
    write_status(&app, app_id, running.clone())?;

    let dir = apps::app_dir(&app, app_id).map_err(|e| e.to_string())?;
    let mut checks = Vec::new();
    checks.extend(check_manifest_shape(&dir, &manifest));

    match manifest.runtime.as_deref().unwrap_or("static") {
        "server" => checks.extend(check_server_runtime(&app, app_id, &manifest).await),
        "external" => checks.extend(check_external_runtime(&manifest)),
        _ => checks.extend(check_static_runtime(&dir, &manifest)),
    }

    let status = final_status(&checks);
    let message = final_message(&status, &checks);
    let finished = apps::AppSelfTestStatus {
        status,
        message: Some(message),
        started_at_ms: Some(started_at_ms),
        finished_at_ms: Some(apps::timestamp_ms()),
        checks,
    };

    write_status(&app, app_id, finished.clone())?;
    logs::log_with(
        &app,
        if finished.status == "passed" {
            LogLevel::Info
        } else {
            LogLevel::Warn
        },
        "app-self-test",
        format!(
            "{}: {}",
            app_id,
            finished.message.clone().unwrap_or_else(|| finished.status.clone())
        ),
    );
    Ok(finished)
}

fn write_status(
    app: &AppHandle,
    app_id: &str,
    status: apps::AppSelfTestStatus,
) -> Result<(), String> {
    apps::write_self_test(app, app_id, &status).map_err(|e| e.to_string())?;
    app.emit(
        "reflex://app-self-test",
        &serde_json::json!({
            "app_id": app_id,
            "self_test": status,
        }),
    )
    .map_err(|e| e.to_string())?;
    app.emit("reflex://apps-changed", &serde_json::json!({ "app_id": app_id }))
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn check_manifest_shape(dir: &Path, manifest: &apps::AppManifest) -> Vec<apps::AppSelfTestCheck> {
    let mut checks = Vec::new();
    let dir_name = dir.file_name().and_then(|s| s.to_str()).unwrap_or_default();
    if manifest.id.trim().is_empty() {
        checks.push(check("manifest.id", "failed", "manifest.id is empty"));
    } else if manifest.id != dir_name {
        checks.push(check(
            "manifest.id",
            "failed",
            format!("manifest.id is {}, but directory is {}", manifest.id, dir_name),
        ));
    } else {
        checks.push(check("manifest.id", "passed", "manifest id matches app directory"));
    }

    let runtime = manifest.runtime.as_deref().unwrap_or("static");
    if matches!(runtime, "static" | "server" | "external") {
        checks.push(check("manifest.runtime", "passed", format!("runtime={runtime}")));
    } else {
        checks.push(check(
            "manifest.runtime",
            "failed",
            format!("unsupported runtime: {runtime}"),
        ));
    }
    checks
}

fn check_static_runtime(dir: &Path, manifest: &apps::AppManifest) -> Vec<apps::AppSelfTestCheck> {
    let mut checks = Vec::new();
    let entry_path = match relative_app_path(dir, &manifest.entry) {
        Ok(path) => path,
        Err(e) => {
            checks.push(check("static.entry", "failed", e));
            return checks;
        }
    };

    if !entry_path.is_file() {
        checks.push(check(
            "static.entry",
            "failed",
            format!("entry file does not exist: {}", manifest.entry),
        ));
        return checks;
    }

    let raw = match std::fs::read_to_string(&entry_path) {
        Ok(raw) => raw,
        Err(e) => {
            checks.push(check(
                "static.entry",
                "failed",
                format!("cannot read entry file: {e}"),
            ));
            return checks;
        }
    };

    if raw.trim().is_empty() {
        checks.push(check("static.entry", "failed", "entry file is empty"));
    } else {
        checks.push(check(
            "static.entry",
            "passed",
            format!("entry exists and has {} bytes", raw.len()),
        ));
    }

    let lower = raw.to_ascii_lowercase();
    if lower.contains("<html") || lower.contains("<body") || lower.contains("<script") {
        checks.push(check("static.html", "passed", "entry looks like runnable HTML"));
    } else {
        checks.push(check(
            "static.html",
            "warning",
            "entry does not look like HTML; verify the iframe can render it",
        ));
    }

    if raw.contains("localStorage") || raw.contains("sessionStorage") {
        checks.push(check(
            "static.storage",
            "warning",
            "entry references browser storage; Reflex iframes should use reflexStorage helpers or wrap browser storage in try/catch",
        ));
    }

    checks
}

async fn check_server_runtime(
    app: &AppHandle,
    app_id: &str,
    manifest: &apps::AppManifest,
) -> Vec<apps::AppSelfTestCheck> {
    let mut checks = Vec::new();
    let Some(server) = manifest.server.as_ref() else {
        checks.push(check("server.config", "failed", "manifest.server is missing"));
        return checks;
    };
    if server.command.is_empty() {
        checks.push(check(
            "server.command",
            "failed",
            "manifest.server.command is empty",
        ));
        return checks;
    }
    checks.push(check(
        "server.command",
        "passed",
        format!("command: {}", server.command.join(" ")),
    ));

    if !apps::manifest_has_permission(manifest, "runtime.server.listen") {
        checks.push(check(
            "server.permission",
            "blocked",
            "runtime.server.listen permission is required before the server can be smoke-tested",
        ));
        return checks;
    }

    let runtimes = app.state::<app_runtime::AppRuntimes>();
    let already_running = app_runtime::running_port(&runtimes, app_id).await.is_some();
    let port = match app_runtime::ensure_started(&runtimes, app, app_id).await {
        Ok(port) => port,
        Err(e) => {
            checks.push(check("server.start", "failed", e));
            return checks;
        }
    };
    checks.push(check("server.start", "passed", format!("listening on :{port}")));

    checks.push(check_server_http(port).await);

    if !already_running {
        app_runtime::stop(&runtimes, app_id).await;
    }

    checks
}

async fn check_server_http(port: u16) -> apps::AppSelfTestCheck {
    let url = format!("http://127.0.0.1:{port}/");
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_millis(3_000))
        .build()
    {
        Ok(client) => client,
        Err(e) => return check("server.http", "failed", format!("http client: {e}")),
    };
    match client.get(&url).send().await {
        Ok(resp) if resp.status().is_success() || resp.status().is_redirection() => check(
            "server.http",
            "passed",
            format!("GET / returned {}", resp.status()),
        ),
        Ok(resp) => check(
            "server.http",
            "failed",
            format!("GET / returned {}", resp.status()),
        ),
        Err(e) => check("server.http", "failed", format!("GET / failed: {e}")),
    }
}

fn check_external_runtime(manifest: &apps::AppManifest) -> Vec<apps::AppSelfTestCheck> {
    let mut checks = Vec::new();
    let Some(external) = manifest.external.as_ref() else {
        checks.push(check("external.config", "failed", "manifest.external is missing"));
        return checks;
    };
    let url = external.url.trim();
    if url.is_empty() {
        checks.push(check("external.url", "failed", "manifest.external.url is empty"));
        return checks;
    }
    match reqwest::Url::parse(url) {
        Ok(parsed) if matches!(parsed.scheme(), "http" | "https") => {
            checks.push(check("external.url", "passed", format!("url={url}")));
        }
        Ok(parsed) => checks.push(check(
            "external.url",
            "failed",
            format!("unsupported URL scheme: {}", parsed.scheme()),
        )),
        Err(e) => checks.push(check("external.url", "failed", format!("invalid URL: {e}"))),
    }
    checks
}

fn relative_app_path(dir: &Path, rel: &str) -> Result<PathBuf, String> {
    let rel_path = Path::new(rel);
    if rel.trim().is_empty() {
        return Err("entry path is empty".into());
    }
    if rel_path.is_absolute() {
        return Err("entry path must be relative to the app directory".into());
    }
    if rel_path
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return Err("entry path must not contain ..".into());
    }
    Ok(dir.join(rel_path))
}

fn check(
    name: impl Into<String>,
    status: impl Into<String>,
    message: impl Into<String>,
) -> apps::AppSelfTestCheck {
    apps::AppSelfTestCheck {
        name: name.into(),
        status: status.into(),
        message: Some(message.into()),
    }
}

fn final_status(checks: &[apps::AppSelfTestCheck]) -> String {
    if checks.iter().any(|c| c.status == "failed") {
        "failed".into()
    } else if checks.iter().any(|c| c.status == "blocked") {
        "blocked".into()
    } else {
        "passed".into()
    }
}

fn final_message(status: &str, checks: &[apps::AppSelfTestCheck]) -> String {
    let failed = checks.iter().filter(|c| c.status == "failed").count();
    let blocked = checks.iter().filter(|c| c.status == "blocked").count();
    let warnings = checks.iter().filter(|c| c.status == "warning").count();
    match status {
        "passed" if warnings > 0 => {
            format!("Self-test passed with {warnings} warning(s)")
        }
        "passed" => "Self-test passed".into(),
        "blocked" => format!("Self-test blocked by {blocked} required setup item(s)"),
        _ => format!("Self-test failed with {failed} failure(s)"),
    }
}
