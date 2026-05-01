use crate::QuickContext;
use tauri::AppHandle;
use tauri_plugin_shell::ShellExt;

const FRONTMOST_SCRIPT: &str = r#"tell application "System Events"
    return name of first process whose frontmost is true
end tell"#;

const FINDER_TARGET_SCRIPT: &str = r#"tell application "System Events"
    set frontApp to name of first process whose frontmost is true
end tell
if frontApp is not "Finder" then
    return ""
end if
tell application "Finder"
    set sel to selection
    if (count of sel) > 0 then
        return POSIX path of (item 1 of sel as alias)
    end if
    try
        return POSIX path of (target of front window as alias)
    end try
end tell
return ""
"#;

pub async fn capture(app: &AppHandle) -> QuickContext {
    let frontmost_app = run_osascript(app, FRONTMOST_SCRIPT)
        .await
        .ok()
        .filter(|s| !s.is_empty());
    let finder_target = run_osascript(app, FINDER_TARGET_SCRIPT)
        .await
        .ok()
        .filter(|s| !s.is_empty());
    QuickContext {
        frontmost_app,
        finder_target,
    }
}

async fn run_osascript(app: &AppHandle, script: &str) -> Result<String, String> {
    let output = app
        .shell()
        .command("osascript")
        .args(["-e", script])
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
