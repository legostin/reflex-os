use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppManifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "default_entry")]
    pub entry: String,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default = "default_kind")]
    pub kind: String,
    #[serde(default)]
    pub created_at_ms: u128,
    /// "static" (default) — отдаём файлы через reflexapp:// URI scheme.
    /// "server" — запускаем процесс из manifest.server.command, iframe смотрит на http://localhost:PORT/
    #[serde(default)]
    pub runtime: Option<String>,
    #[serde(default)]
    pub server: Option<ServerConfig>,
    #[serde(default)]
    pub network: Option<NetworkPolicy>,
    #[serde(default)]
    pub schedules: Vec<ScheduleDef>,
    #[serde(default)]
    pub actions: Vec<ActionDef>,
    #[serde(default)]
    pub widgets: Vec<WidgetDef>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WidgetDef {
    pub id: String,
    pub name: String,
    pub entry: String,
    #[serde(default = "default_widget_size")]
    pub size: String,
    #[serde(default)]
    pub description: Option<String>,
}

fn default_widget_size() -> String {
    "small".into()
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Step {
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
    #[serde(default)]
    pub save_as: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ScheduleDef {
    pub id: String,
    pub name: String,
    pub cron: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_catch_up")]
    pub catch_up: String,
    pub steps: Vec<Step>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ActionDef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub public: bool,
    pub steps: Vec<Step>,
}

fn default_true() -> bool {
    true
}

fn default_catch_up() -> String {
    "once".into()
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct NetworkPolicy {
    /// Hostnames the app can fetch via `net.fetch`. Supports leading wildcard "*.example.com".
    #[serde(default, alias = "allowedHosts")]
    pub allowed_hosts: Vec<String>,
}

impl NetworkPolicy {
    pub fn allows_host(&self, host: &str) -> bool {
        let host = host.to_lowercase();
        for pattern in &self.allowed_hosts {
            let pat = pattern.to_lowercase();
            if let Some(suffix) = pat.strip_prefix("*.") {
                if host == suffix || host.ends_with(&format!(".{suffix}")) {
                    return true;
                }
            } else if host == pat {
                return true;
            }
        }
        false
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ServerConfig {
    /// argv: ["node", "server.js"], ["python3", "-m", "http.server"], ...
    pub command: Vec<String>,
    /// сколько ждать пока порт начнёт отвечать. Default 15s.
    #[serde(default)]
    pub ready_timeout_ms: Option<u64>,
}

fn default_entry() -> String {
    "index.html".into()
}

fn default_kind() -> String {
    "panel".into()
}

pub fn apps_dir(app: &AppHandle) -> io::Result<PathBuf> {
    let base = app
        .path()
        .app_data_dir()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
    let dir = base.join("apps");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn app_dir(app: &AppHandle, id: &str) -> io::Result<PathBuf> {
    let dir = apps_dir(app)?.join(id);
    Ok(dir)
}

pub fn trash_dir(app: &AppHandle) -> io::Result<PathBuf> {
    let base = app
        .path()
        .app_data_dir()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
    let dir = base.join("trash").join("apps");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TrashEntry {
    pub trash_id: String,
    pub original_id: String,
    pub original_name: String,
    pub original_icon: Option<String>,
    pub original_description: Option<String>,
    pub deleted_at_ms: u128,
    pub original_root: String,
}

fn trash_index_path(app: &AppHandle) -> io::Result<PathBuf> {
    Ok(trash_dir(app)?.join("index.json"))
}

fn read_trash_index(app: &AppHandle) -> io::Result<Vec<TrashEntry>> {
    let path = trash_index_path(app)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&raw).unwrap_or_default())
}

fn write_trash_index(app: &AppHandle, entries: &[TrashEntry]) -> io::Result<()> {
    let path = trash_index_path(app)?;
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(entries)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
    fs::write(&tmp, bytes)?;
    fs::rename(tmp, path)
}

pub fn list_trash(app: &AppHandle) -> io::Result<Vec<TrashEntry>> {
    let mut all = read_trash_index(app)?;
    all.sort_by(|a, b| b.deleted_at_ms.cmp(&a.deleted_at_ms));
    Ok(all)
}

pub fn move_to_trash(app: &AppHandle, app_id: &str) -> io::Result<TrashEntry> {
    let src = app_dir(app, app_id)?;
    if !src.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("app dir missing: {}", src.display()),
        ));
    }
    let manifest = read_manifest(app, app_id).ok();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let trash_id = format!("{app_id}__{now}");
    let dst = trash_dir(app)?.join(&trash_id);
    fs::rename(&src, &dst)?;

    let entry = TrashEntry {
        trash_id: trash_id.clone(),
        original_id: app_id.to_string(),
        original_name: manifest
            .as_ref()
            .map(|m| m.name.clone())
            .unwrap_or_else(|| app_id.to_string()),
        original_icon: manifest.as_ref().and_then(|m| m.icon.clone()),
        original_description: manifest.as_ref().and_then(|m| m.description.clone()),
        deleted_at_ms: now,
        original_root: src.to_string_lossy().into_owned(),
    };
    let mut all = read_trash_index(app)?;
    all.push(entry.clone());
    write_trash_index(app, &all)?;
    Ok(entry)
}

pub fn restore_from_trash(app: &AppHandle, trash_id: &str) -> io::Result<String> {
    let mut all = read_trash_index(app)?;
    let pos = all
        .iter()
        .position(|e| e.trash_id == trash_id)
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, format!("trash entry: {trash_id}"))
        })?;
    let entry = all[pos].clone();
    let src = trash_dir(app)?.join(&entry.trash_id);
    if !src.exists() {
        all.remove(pos);
        write_trash_index(app, &all)?;
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("trashed dir missing: {}", src.display()),
        ));
    }

    let mut target_id = entry.original_id.clone();
    let mut target_dir = app_dir(app, &target_id)?;
    if target_dir.exists() {
        let suffix = uuid::Uuid::new_v4()
            .simple()
            .to_string()
            .chars()
            .take(6)
            .collect::<String>();
        target_id = format!("{}_{suffix}", entry.original_id);
        target_dir = app_dir(app, &target_id)?;
    }
    fs::rename(&src, &target_dir)?;

    if target_id != entry.original_id {
        let manifest_path = target_dir.join("manifest.json");
        if let Ok(s) = fs::read_to_string(&manifest_path) {
            if let Ok(mut m) = serde_json::from_str::<AppManifest>(&s) {
                m.id = target_id.clone();
                let _ = fs::write(
                    &manifest_path,
                    serde_json::to_string_pretty(&m).unwrap_or_default(),
                );
            }
        }
        let project_file = target_dir.join(".reflex").join("project.json");
        if let Ok(s) = fs::read_to_string(&project_file) {
            if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&s) {
                if let Some(obj) = v.as_object_mut() {
                    obj.insert(
                        "root".into(),
                        serde_json::Value::String(target_dir.to_string_lossy().into_owned()),
                    );
                }
                let _ = fs::write(
                    &project_file,
                    serde_json::to_string_pretty(&v).unwrap_or_default(),
                );
            }
        }
    } else {
        let project_file = target_dir.join(".reflex").join("project.json");
        if let Ok(s) = fs::read_to_string(&project_file) {
            if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&s) {
                if let Some(obj) = v.as_object_mut() {
                    obj.insert(
                        "root".into(),
                        serde_json::Value::String(target_dir.to_string_lossy().into_owned()),
                    );
                }
                let _ = fs::write(
                    &project_file,
                    serde_json::to_string_pretty(&v).unwrap_or_default(),
                );
            }
        }
    }

    all.remove(pos);
    write_trash_index(app, &all)?;
    Ok(target_id)
}

pub fn purge_trashed(app: &AppHandle, trash_id: &str) -> io::Result<()> {
    let mut all = read_trash_index(app)?;
    let pos = all
        .iter()
        .position(|e| e.trash_id == trash_id)
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, format!("trash entry: {trash_id}"))
        })?;
    let entry = all.remove(pos);
    let dir = trash_dir(app)?.join(&entry.trash_id);
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    write_trash_index(app, &all)
}

#[derive(Serialize, Clone, Debug)]
pub struct AppListing {
    #[serde(flatten)]
    pub manifest: AppManifest,
    pub ready: bool,
}

pub fn list_apps(app: &AppHandle) -> io::Result<Vec<AppListing>> {
    let dir = apps_dir(app)?;
    let mut out = Vec::new();
    if !dir.exists() {
        return Ok(out);
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let app_path = entry.path();
        let manifest_path = app_path.join("manifest.json");
        if !manifest_path.exists() {
            continue;
        }
        if let Ok(s) = fs::read_to_string(&manifest_path) {
            if let Ok(m) = serde_json::from_str::<AppManifest>(&s) {
                let ready = app_path.join(&m.entry).exists();
                out.push(AppListing { manifest: m, ready });
            }
        }
    }
    out.sort_by(|a, b| {
        a.manifest
            .name
            .to_lowercase()
            .cmp(&b.manifest.name.to_lowercase())
    });
    Ok(out)
}

pub fn read_app_html(app: &AppHandle, id: &str) -> io::Result<String> {
    let dir = app_dir(app, id)?;
    let manifest_path = dir.join("manifest.json");
    let manifest_str = fs::read_to_string(manifest_path)?;
    let manifest: AppManifest = serde_json::from_str(&manifest_str)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
    fs::read_to_string(dir.join(&manifest.entry))
}

pub fn read_manifest(app: &AppHandle, id: &str) -> io::Result<AppManifest> {
    let dir = app_dir(app, id)?;
    let manifest_path = dir.join("manifest.json");
    let manifest_str = fs::read_to_string(manifest_path)?;
    serde_json::from_str(&manifest_str)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))
}

pub fn write_manifest(app: &AppHandle, id: &str, manifest: &AppManifest) -> io::Result<()> {
    let dir = app_dir(app, id)?;
    fs::create_dir_all(&dir)?;
    fs::write(
        dir.join("manifest.json"),
        serde_json::to_string_pretty(manifest)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?,
    )
}

/// Read a file under apps/<id>/ with path-traversal protection.
pub fn read_app_file(app: &AppHandle, id: &str, relative: &str) -> io::Result<Vec<u8>> {
    let dir = app_dir(app, id)?;
    let candidate = dir.join(relative.trim_start_matches('/'));
    let dir_canon = dir.canonicalize()?;
    let cand_canon = candidate.canonicalize()?;
    if !cand_canon.starts_with(&dir_canon) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "path traversal",
        ));
    }
    fs::read(cand_canon)
}

pub fn write_app_file(
    app: &AppHandle,
    id: &str,
    relative: &str,
    bytes: &[u8],
) -> io::Result<()> {
    let dir = app_dir(app, id)?;
    fs::create_dir_all(&dir)?;
    let candidate = dir.join(relative.trim_start_matches('/'));
    if let Some(parent) = candidate.parent() {
        fs::create_dir_all(parent)?;
    }
    let dir_canon = dir.canonicalize()?;
    let cand_parent_canon = candidate
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "no parent"))?
        .canonicalize()?;
    if !cand_parent_canon.starts_with(&dir_canon) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "path traversal",
        ));
    }
    fs::write(&candidate, bytes)
}

pub fn read_storage(app: &AppHandle, id: &str) -> io::Result<serde_json::Value> {
    let dir = app_dir(app, id)?;
    fs::create_dir_all(&dir)?;
    let path = dir.join("storage.json");
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    let s = fs::read_to_string(path)?;
    serde_json::from_str(&s).or_else(|_| Ok(serde_json::json!({})))
}

pub fn write_storage(
    app: &AppHandle,
    id: &str,
    value: &serde_json::Value,
) -> io::Result<()> {
    let dir = app_dir(app, id)?;
    fs::create_dir_all(&dir)?;
    let path = dir.join("storage.json");
    fs::write(
        path,
        serde_json::to_string_pretty(value)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?,
    )
}

// ---- git helpers ----

const GITIGNORE: &str = ".reflex/\nstorage.json\nmeta-llm.txt\n.DS_Store\n";

pub fn git_init_if_needed(dir: &std::path::Path) -> io::Result<()> {
    if dir.join(".git").is_dir() {
        return Ok(());
    }
    let _ = std::process::Command::new("git")
        .arg("init")
        .arg("-q")
        .current_dir(dir)
        .output()?;
    fs::write(dir.join(".gitignore"), GITIGNORE)?;
    let _ = std::process::Command::new("git")
        .args(["config", "user.email", "reflex@local"])
        .current_dir(dir)
        .output();
    let _ = std::process::Command::new("git")
        .args(["config", "user.name", "Reflex"])
        .current_dir(dir)
        .output();
    let _ = std::process::Command::new("git")
        .args(["config", "commit.gpgsign", "false"])
        .current_dir(dir)
        .output();
    Ok(())
}

#[derive(Serialize, Clone, Debug)]
pub struct GitStatus {
    pub has_changes: bool,
    pub revision: u32,
    pub last_commit_message: Option<String>,
}

pub fn git_status(dir: &std::path::Path) -> io::Result<GitStatus> {
    if !dir.join(".git").is_dir() {
        return Ok(GitStatus {
            has_changes: false,
            revision: 0,
            last_commit_message: None,
        });
    }
    let porcelain = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir)
        .output()?;
    let has_changes = !porcelain.stdout.is_empty();

    let count = std::process::Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .current_dir(dir)
        .output();
    let revision: u32 = match count {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .trim()
            .parse()
            .unwrap_or(0),
        _ => 0,
    };

    let msg_out = std::process::Command::new("git")
        .args(["log", "-1", "--pretty=%s"])
        .current_dir(dir)
        .output();
    let last_commit_message = match msg_out {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        }
        _ => None,
    };

    Ok(GitStatus {
        has_changes,
        revision,
        last_commit_message,
    })
}

pub fn git_commit_all(dir: &std::path::Path, message: &str) -> io::Result<()> {
    git_init_if_needed(dir)?;
    let _ = std::process::Command::new("git")
        .args(["add", "-A"])
        .current_dir(dir)
        .output()?;
    let out = std::process::Command::new("git")
        .args(["commit", "-m", message, "--allow-empty"])
        .current_dir(dir)
        .output()?;
    if !out.status.success() {
        eprintln!(
            "[reflex] git commit non-zero: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(())
}

pub fn git_diff(dir: &std::path::Path) -> io::Result<String> {
    git_init_if_needed(dir)?;
    // include untracked files as if they were freshly added
    let _ = std::process::Command::new("git")
        .args(["add", "-N", "."])
        .current_dir(dir)
        .output();
    let out = std::process::Command::new("git")
        .args(["diff", "--no-color", "HEAD", "--"])
        .current_dir(dir)
        .output()?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn git_apply_partial(
    dir: &std::path::Path,
    patch: &str,
    message: &str,
) -> io::Result<()> {
    git_init_if_needed(dir)?;
    use std::io::Write;
    // write patch to a temp file inside dir (git apply needs a real file path)
    let tmp = dir.join(".reflex").join("partial.patch");
    if let Some(parent) = tmp.parent() {
        fs::create_dir_all(parent)?;
    }
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(patch.as_bytes())?;
    }
    // Reset index so any pre-staged stuff doesn't sneak into the commit.
    let _ = std::process::Command::new("git")
        .args(["reset"])
        .current_dir(dir)
        .output();
    let apply_out = std::process::Command::new("git")
        .args(["apply", "--cached", "--whitespace=nowarn"])
        .arg(&tmp)
        .current_dir(dir)
        .output()?;
    if !apply_out.status.success() {
        let _ = fs::remove_file(&tmp);
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "git apply failed: {}",
                String::from_utf8_lossy(&apply_out.stderr)
            ),
        ));
    }
    let commit_out = std::process::Command::new("git")
        .args(["commit", "-m", message])
        .current_dir(dir)
        .output()?;
    let _ = fs::remove_file(&tmp);
    if !commit_out.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "git commit failed: {}",
                String::from_utf8_lossy(&commit_out.stderr)
            ),
        ));
    }
    Ok(())
}

pub fn git_revert_all(dir: &std::path::Path) -> io::Result<()> {
    if !dir.join(".git").is_dir() {
        return Ok(());
    }
    let _ = std::process::Command::new("git")
        .args(["checkout", "--", "."])
        .current_dir(dir)
        .output()?;
    let _ = std::process::Command::new("git")
        .args(["clean", "-fd"])
        .current_dir(dir)
        .output()?;
    Ok(())
}

pub const RUNTIME_OVERLAY_JS: &str = r#"<script>
(function(){
  if (window.__reflexOverlay) return;
  window.__reflexOverlay = true;

  // ---- runtime error capture ----
  function postError(payload) {
    try {
      window.parent.postMessage({source:'reflex-app', type:'runtime.error', payload: payload}, '*');
    } catch(_) {}
  }
  window.addEventListener('error', function(e){
    postError({
      message: e.message || String(e.error || ''),
      filename: e.filename || '',
      lineno: e.lineno || 0,
      colno: e.colno || 0,
      stack: (e.error && e.error.stack) ? String(e.error.stack) : ''
    });
  });
  window.addEventListener('unhandledrejection', function(e){
    var r = e.reason;
    postError({
      message: (r && (r.message || r.toString())) || 'unhandledrejection',
      filename: '',
      lineno: 0,
      colno: 0,
      stack: (r && r.stack) ? String(r.stack) : ''
    });
  });

  // ---- inspector ----
  var inspecting = false;
  var hovered = null;
  function setOutline(el, on) {
    if (!el) return;
    if (on) {
      el.__reflexPrevOutline = el.style.outline;
      el.__reflexPrevOffset = el.style.outlineOffset;
      el.style.outline = '2px solid #4a8cff';
      el.style.outlineOffset = '-2px';
    } else {
      el.style.outline = el.__reflexPrevOutline || '';
      el.style.outlineOffset = el.__reflexPrevOffset || '';
      delete el.__reflexPrevOutline;
      delete el.__reflexPrevOffset;
    }
  }
  function buildSelector(el) {
    if (!el || !el.tagName) return '';
    if (el === document.body) return 'body';
    var path = [];
    var cur = el;
    while (cur && cur.nodeType === 1 && cur !== document.body && path.length < 6) {
      var part = cur.tagName.toLowerCase();
      if (cur.id) { part += '#' + cur.id; path.unshift(part); break; }
      var classes = (cur.className && typeof cur.className === 'string') ? cur.className.trim().split(/\s+/).slice(0, 2) : [];
      if (classes.length) part += '.' + classes.join('.');
      var parent = cur.parentElement;
      if (parent) {
        var same = Array.prototype.filter.call(parent.children, function(c){ return c.tagName === cur.tagName; });
        if (same.length > 1) part += ':nth-of-type(' + (Array.prototype.indexOf.call(same, cur) + 1) + ')';
      }
      path.unshift(part);
      cur = cur.parentElement;
    }
    return path.join(' > ');
  }
  function pickStyle(el) {
    var s = window.getComputedStyle(el);
    var out = {};
    var keys = ['display','position','color','background-color','font-size','font-weight','padding','margin','border','width','height'];
    keys.forEach(function(k){ out[k] = s.getPropertyValue(k); });
    return out;
  }
  function onMove(e) {
    if (!inspecting) return;
    var el = e.target;
    if (el === hovered) return;
    setOutline(hovered, false);
    hovered = el;
    setOutline(hovered, true);
  }
  function onClickCapture(e) {
    if (!inspecting) return;
    e.preventDefault();
    e.stopPropagation();
    var el = e.target;
    var outerHTML = el.outerHTML || '';
    if (outerHTML.length > 1500) outerHTML = outerHTML.slice(0, 1500) + '…';
    var payload = {
      selector: buildSelector(el),
      tagName: el.tagName,
      id: el.id || null,
      classes: (el.className && typeof el.className === 'string') ? el.className.trim().split(/\s+/) : [],
      text: (el.innerText || '').slice(0, 200),
      outerHTML: outerHTML,
      computedStyle: pickStyle(el)
    };
    setOutline(hovered, false); hovered = null;
    inspecting = false;
    document.body.style.cursor = '';
    try { window.parent.postMessage({source:'reflex-app', type:'inspector.pick', payload: payload}, '*'); } catch(_) {}
  }
  document.addEventListener('mousemove', onMove, true);
  document.addEventListener('click', onClickCapture, true);

  // ---- inter-app event SDK ----
  var eventHandlers = Object.create(null);
  function reflexInvokeRaw(method, params) {
    return new Promise(function(resolve, reject){
      var id = 'r_' + Date.now().toString(36) + '_' + Math.random().toString(36).slice(2,8);
      function once(ev){
        var d = ev.data;
        if (!d || d.source !== 'reflex' || d.id !== id) return;
        window.removeEventListener('message', once);
        d.error ? reject(d.error) : resolve(d.result);
      }
      window.addEventListener('message', once);
      window.parent.postMessage({source:'reflex-app',type:'request',id:id,method:method,params:params||{}}, '*');
    });
  }
  window.reflexEventOn = function(topic, cb) {
    if (!eventHandlers[topic]) {
      eventHandlers[topic] = [];
      reflexInvokeRaw('events.subscribe', {topics: [topic]}).catch(function(e){ console.warn('[reflex] subscribe failed', e); });
    }
    eventHandlers[topic].push(cb);
  };
  window.reflexEventOff = function(topic) {
    delete eventHandlers[topic];
    reflexInvokeRaw('events.unsubscribe', {topics: [topic]}).catch(function(){});
  };
  window.reflexEventEmit = function(topic, payload) {
    return reflexInvokeRaw('events.emit', {topic: topic, payload: payload});
  };
  window.reflexAppsInvoke = function(appId, actionId, params) {
    return reflexInvokeRaw('apps.invoke', {app_id: appId, action_id: actionId, params: params||{}});
  };
  window.reflexAppsListActions = function(appIdOrParams, includeSteps) {
    var params = (typeof appIdOrParams === 'string') ? {app_id: appIdOrParams, include_steps: !!includeSteps} : (appIdOrParams || {});
    return reflexInvokeRaw('apps.list_actions', params);
  };
  window.reflexInvoke = reflexInvokeRaw;
  window.reflexSystemContext = function() {
    return reflexInvokeRaw('system.context', {});
  };
  window.reflexManifestGet = function() {
    return reflexInvokeRaw('manifest.get', {});
  };
  window.reflexManifestUpdate = function(patch) {
    return reflexInvokeRaw('manifest.update', {patch: patch || {}});
  };
  window.reflexProjectsList = function(params) {
    return reflexInvokeRaw('projects.list', params || {});
  };
  window.reflexTopicsList = function(params) {
    return reflexInvokeRaw('topics.list', params || {});
  };
  window.reflexBrowserTabs = function() {
    return reflexInvokeRaw('browser.tabs.list', {});
  };
  window.reflexBrowserOpen = function(url) {
    return reflexInvokeRaw('browser.open', {url: url || null});
  };
  window.reflexBrowserNavigate = function(tabId, url) {
    return reflexInvokeRaw('browser.navigate', {tabId: tabId, url: url});
  };
  window.reflexBrowserReadText = function(tabId) {
    return reflexInvokeRaw('browser.readText', {tabId: tabId});
  };
  window.reflexSchedulerList = function(params) {
    return reflexInvokeRaw('scheduler.list', params || {});
  };
  window.reflexSchedulerRunNow = function(scheduleId) {
    return reflexInvokeRaw('scheduler.runNow', {scheduleId: scheduleId});
  };
  window.reflexSchedulerSetPaused = function(scheduleId, paused) {
    return reflexInvokeRaw('scheduler.setPaused', {scheduleId: scheduleId, paused: !!paused});
  };
  window.reflexSchedulerRuns = function(params) {
    return reflexInvokeRaw('scheduler.runs', params || {});
  };
  window.reflexMemorySave = function(params) {
    return reflexInvokeRaw('memory.save', params || {});
  };
  window.reflexMemoryList = function(params) {
    return reflexInvokeRaw('memory.list', params || {});
  };
  window.reflexMemoryDelete = function(relPathOrParams) {
    var params = (typeof relPathOrParams === 'string') ? {relPath: relPathOrParams} : (relPathOrParams || {});
    return reflexInvokeRaw('memory.delete', params);
  };
  window.reflexMemorySearch = function(queryOrParams) {
    var params = (typeof queryOrParams === 'string') ? {query: queryOrParams} : (queryOrParams || {});
    return reflexInvokeRaw('memory.search', params);
  };
  window.reflexMemoryRecall = function(queryOrParams) {
    var params = (typeof queryOrParams === 'string') ? {query: queryOrParams} : (queryOrParams || {});
    return reflexInvokeRaw('memory.recall', params);
  };
  window.reflexMemoryIndexPath = function(pathOrParams) {
    var params = (typeof pathOrParams === 'string') ? {path: pathOrParams} : (pathOrParams || {});
    return reflexInvokeRaw('memory.indexPath', params);
  };
  window.reflexMemoryPathStatus = function(pathOrParams) {
    var params = (typeof pathOrParams === 'string') ? {path: pathOrParams} : (pathOrParams || {});
    return reflexInvokeRaw('memory.pathStatus', params);
  };
  window.reflexMemoryForgetPath = function(pathOrParams) {
    var params = (typeof pathOrParams === 'string') ? {path: pathOrParams} : (pathOrParams || {});
    return reflexInvokeRaw('memory.forgetPath', params);
  };

  window.addEventListener('message', function(ev){
    var m = ev.data;
    if (!m || m.source !== 'reflex') return;
    if (m.type === 'inspector.toggle') {
      inspecting = !!m.on;
      document.body.style.cursor = inspecting ? 'crosshair' : '';
      if (!inspecting) { setOutline(hovered, false); hovered = null; }
    } else if (m.type === 'event' && m.topic) {
      var handlers = eventHandlers[m.topic];
      if (handlers) {
        for (var i = 0; i < handlers.length; i++) {
          try { handlers[i](m.data, m.fromApp); } catch(err) { console.error('[reflex] event handler error', err); }
        }
      }
    }
  });
})();
</script>
"#;

/// Returns true if the path is the entry HTML which should get the overlay injected.
#[allow(dead_code)]
pub fn is_html_entry(path: &str, entry: &str) -> bool {
    let p = path.trim_start_matches('/');
    let e = entry.trim_start_matches('/');
    p == e
        || p == "index.html"
        || (p.is_empty() && (e == "index.html" || e.ends_with(".html")))
}

/// Inject the overlay script before </body>, or append at the end as a fallback.
pub fn inject_overlay_into_html(html: &[u8]) -> Vec<u8> {
    let s = match std::str::from_utf8(html) {
        Ok(s) => s,
        Err(_) => return html.to_vec(),
    };
    let lower = s.to_lowercase();
    if lower.contains("__reflexoverlay") {
        return s.as_bytes().to_vec();
    }
    if let Some(pos) = lower.rfind("</body>") {
        let mut out = String::with_capacity(s.len() + RUNTIME_OVERLAY_JS.len());
        out.push_str(&s[..pos]);
        out.push_str(RUNTIME_OVERLAY_JS);
        out.push_str(&s[pos..]);
        return out.into_bytes();
    }
    let mut out = String::with_capacity(s.len() + RUNTIME_OVERLAY_JS.len());
    out.push_str(s);
    out.push_str(RUNTIME_OVERLAY_JS);
    out.into_bytes()
}

pub fn guess_mime(name: &str) -> &'static str {
    let lower = name.to_lowercase();
    if lower.ends_with(".html") || lower.ends_with(".htm") {
        "text/html; charset=utf-8"
    } else if lower.ends_with(".js") || lower.ends_with(".mjs") {
        "application/javascript; charset=utf-8"
    } else if lower.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if lower.ends_with(".json") {
        "application/json; charset=utf-8"
    } else if lower.ends_with(".svg") {
        "image/svg+xml"
    } else if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else if lower.ends_with(".woff2") {
        "font/woff2"
    } else {
        "application/octet-stream"
    }
}

// ---- Export / Import .reflexapp bundles ----

const EXPORT_SKIP_DIRS: &[&str] = &[".reflex", ".git", "node_modules"];
const EXPORT_SKIP_FILES: &[&str] = &["storage.json", "meta-llm.txt", ".DS_Store"];

fn should_skip_export(rel: &std::path::Path) -> bool {
    for comp in rel.components() {
        let s = comp.as_os_str().to_string_lossy();
        if EXPORT_SKIP_DIRS.iter().any(|n| s == *n) {
            return true;
        }
    }
    if let Some(name) = rel.file_name().and_then(|n| n.to_str()) {
        if EXPORT_SKIP_FILES.iter().any(|n| name == *n) {
            return true;
        }
    }
    false
}

fn collect_files(dir: &std::path::Path, base: &std::path::Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let rel = path.strip_prefix(base).unwrap_or(&path).to_path_buf();
        if should_skip_export(&rel) {
            continue;
        }
        let ft = entry.file_type()?;
        if ft.is_dir() {
            collect_files(&path, base, out)?;
        } else if ft.is_file() {
            out.push(rel);
        }
    }
    Ok(())
}

pub fn export_app(
    app: &AppHandle,
    app_id: &str,
    target: &std::path::Path,
) -> io::Result<()> {
    use std::io::Write;
    let dir = app_dir(app, app_id)?;
    if !dir.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("app not found: {app_id}"),
        ));
    }
    let mut entries: Vec<PathBuf> = vec![];
    collect_files(&dir, &dir, &mut entries)?;

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = fs::File::create(target)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts: zip::write::SimpleFileOptions =
        zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o644);
    for rel in entries {
        let full = dir.join(&rel);
        let bytes = fs::read(&full)?;
        let zip_path = rel.to_string_lossy().replace('\\', "/");
        zip.start_file(zip_path, opts)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
        zip.write_all(&bytes)?;
    }
    zip.finish()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
    Ok(())
}

pub fn import_app(app: &AppHandle, zip_path: &std::path::Path) -> io::Result<AppManifest> {
    use std::io::Read;
    let file = fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

    // Locate manifest.json first to derive id/name.
    let mut manifest_bytes: Option<Vec<u8>> = None;
    for i in 0..archive.len() {
        let mut f = archive
            .by_index(i)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
        if f.name() == "manifest.json" {
            let mut buf = Vec::new();
            f.read_to_end(&mut buf)?;
            manifest_bytes = Some(buf);
            break;
        }
    }
    let manifest_bytes = manifest_bytes.ok_or_else(|| {
        io::Error::new(io::ErrorKind::Other, "manifest.json missing in bundle")
    })?;
    let mut manifest: AppManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

    // Pick fresh id: original if free, else `<id>_imported_<ts>`.
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let original_id = manifest.id.clone();
    let target_dir_root = apps_dir(app)?;
    let mut new_id = original_id.clone();
    if target_dir_root.join(&new_id).exists() {
        new_id = format!("{original_id}_imported_{now_ms}");
    }
    manifest.id = new_id.clone();
    let target_dir = target_dir_root.join(&new_id);
    fs::create_dir_all(&target_dir)?;
    let target_canon = target_dir
        .canonicalize()
        .unwrap_or_else(|_| target_dir.clone());

    // Extract entries (zip-slip safe).
    for i in 0..archive.len() {
        let mut f = archive
            .by_index(i)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
        if f.name() == "manifest.json" {
            continue;
        }
        let outpath = match f.enclosed_name() {
            Some(p) => target_dir.join(p),
            None => continue,
        };
        // Defense in depth — verify the resolved path stays inside target_dir.
        if let Ok(canon_parent) = outpath
            .parent()
            .map(|p| {
                p.to_path_buf()
            })
            .ok_or(io::Error::new(io::ErrorKind::Other, "bad path"))
        {
            if let Some(parent) = Some(canon_parent.as_path()) {
                fs::create_dir_all(parent)?;
            }
            if let Ok(parent_canon) = outpath
                .parent()
                .ok_or(io::Error::new(io::ErrorKind::Other, "no parent"))
                .and_then(|p| p.canonicalize())
            {
                if !parent_canon.starts_with(&target_canon) {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "zip path escape",
                    ));
                }
            }
        }
        if f.is_dir() {
            fs::create_dir_all(&outpath)?;
        } else {
            if let Some(parent) = outpath.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut out = fs::File::create(&outpath)?;
            std::io::copy(&mut f, &mut out)?;
        }
    }
    // Write manifest with adjusted id.
    fs::write(
        target_dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?,
    )?;
    Ok(manifest)
}

pub fn ensure_sample_app(app: &AppHandle) -> io::Result<()> {
    let dir = app_dir(app, "sample-hello")?;
    if dir.join("manifest.json").exists() {
        return Ok(());
    }
    fs::create_dir_all(&dir)?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let manifest = AppManifest {
        id: "sample-hello".into(),
        name: "Sample · Ask Reflex".into(),
        icon: Some("👋".into()),
        description: Some("Минимальный пример: спрашивает у агента и показывает ответ.".into()),
        entry: "index.html".into(),
        permissions: vec!["agent.ask".into()],
        kind: "panel".into(),
        created_at_ms: now_ms,
        runtime: None,
        server: None,
        network: None,
        schedules: Vec::new(),
        actions: Vec::new(),
        widgets: Vec::new(),
    };
    fs::write(
        dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?,
    )?;
    fs::write(dir.join("index.html"), SAMPLE_HTML)?;
    ensure_sample_cron_app(app, now_ms)?;
    Ok(())
}

fn ensure_sample_cron_app(app: &AppHandle, now_ms: u128) -> io::Result<()> {
    let dir = app_dir(app, "sample-cron")?;
    if dir.join("manifest.json").exists() {
        return Ok(());
    }
    fs::create_dir_all(&dir)?;
    let manifest = AppManifest {
        id: "sample-cron".into(),
        name: "Sample · Heartbeat".into(),
        icon: Some("⏱".into()),
        description: Some("Демо-расписания: каждую минуту пишет timestamp в storage.".into()),
        entry: "index.html".into(),
        permissions: vec!["storage.set".into(), "storage.get".into()],
        kind: "panel".into(),
        created_at_ms: now_ms,
        runtime: None,
        server: None,
        network: None,
        schedules: vec![ScheduleDef {
            id: "heartbeat".into(),
            name: "Heartbeat (every minute)".into(),
            cron: "0 * * * * *".into(),
            enabled: true,
            catch_up: "once".into(),
            steps: vec![Step {
                method: "storage.set".into(),
                params: serde_json::json!({
                    "key": "last_tick_ms",
                    "value": now_ms,
                }),
                save_as: None,
            }],
        }],
        actions: Vec::new(),
        widgets: vec![WidgetDef {
            id: "heartbeat".into(),
            name: "Last heartbeat".into(),
            entry: "widgets/heartbeat.html".into(),
            size: "small".into(),
            description: Some("Когда расписание сработало в последний раз".into()),
        }],
    };
    fs::write(
        dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?,
    )?;
    fs::write(dir.join("index.html"), SAMPLE_CRON_HTML)?;
    fs::create_dir_all(dir.join("widgets"))?;
    fs::write(dir.join("widgets").join("heartbeat.html"), SAMPLE_CRON_WIDGET_HTML)?;
    Ok(())
}

const SAMPLE_CRON_WIDGET_HTML: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><style>
html,body{margin:0;padding:0;background:transparent;color:#eee;font-family:system-ui;height:100%}
.box{box-sizing:border-box;height:100%;padding:14px;display:flex;flex-direction:column;justify-content:center;gap:4px}
.label{font-size:10px;color:rgba(180,185,195,0.7);text-transform:uppercase;letter-spacing:0.05em}
.value{font-size:18px;font-weight:600}
.ago{font-size:11px;color:rgba(180,185,195,0.7)}
</style></head><body>
<div class="box">
  <div class="label">⏱ Heartbeat</div>
  <div class="value" id="value">—</div>
  <div class="ago" id="ago">не было</div>
</div>
<script>
async function rinvoke(method, params){
  return new Promise((res, rej) => {
    const id = Math.random().toString(36).slice(2);
    function on(ev){
      if (ev.data?.source !== 'reflex' || ev.data?.id !== id) return;
      window.removeEventListener('message', on);
      ev.data.error ? rej(ev.data.error) : res(ev.data.result);
    }
    window.addEventListener('message', on);
    window.parent.postMessage({source:'reflex-app',type:'request',id,method,params}, '*');
  });
}
async function refresh(){
  try {
    const r = await rinvoke('storage.get',{key:'last_tick_ms'});
    if (!r.value){ document.getElementById('value').textContent='—'; document.getElementById('ago').textContent='нет данных'; return; }
    const ts = Number(r.value);
    document.getElementById('value').textContent = new Date(ts).toLocaleTimeString();
    const min = Math.floor((Date.now()-ts)/60000);
    document.getElementById('ago').textContent = min < 1 ? 'только что' : (min + ' мин назад');
  } catch (e) { document.getElementById('value').textContent='—'; }
}
refresh();
setInterval(refresh, 5000);
</script>
</body></html>"#;

const SAMPLE_CRON_HTML: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><title>Heartbeat</title>
<style>body{font-family:system-ui;background:#15171c;color:#eee;padding:24px}code{background:#222;padding:2px 6px;border-radius:4px}</style>
</head><body>
<h2>⏱ Heartbeat sample</h2>
<p>Это демо-приложение, у которого в манифесте описано расписание <code>0 * * * * *</code> (раз в минуту).</p>
<p>Reflex запускает шаг <code>storage.set last_tick_ms</code> сам, даже когда это окно скрыто. Открой раздел Automations чтобы видеть запуски.</p>
<p>Последний tick: <code id="last">—</code></p>
<script>
async function reflexInvoke(method, params){
  return new Promise((res, rej) => {
    const id = Math.random().toString(36).slice(2);
    function on(ev){
      if (ev.data?.source !== 'reflex' || ev.data?.id !== id) return;
      window.removeEventListener('message', on);
      ev.data.error ? rej(ev.data.error) : res(ev.data.result);
    }
    window.addEventListener('message', on);
    window.parent.postMessage({source:'reflex-app',type:'request',id,method,params}, '*');
  });
}
async function refresh(){
  try {
    const r = await reflexInvoke('storage.get',{key:'last_tick_ms'});
    document.getElementById('last').textContent = r.value
      ? new Date(Number(r.value)).toLocaleString()
      : '—';
  } catch (e) { document.getElementById('last').textContent = String(e); }
}
refresh();
setInterval(refresh, 5000);
</script>
</body></html>"#;

const SAMPLE_HTML: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><title>Sample</title>
<style>
  :root { color-scheme: dark; font-family: -apple-system, system-ui, sans-serif; }
  body { margin: 0; padding: 24px; color: #f5f5f7; background: transparent; }
  h1 { margin: 0 0 12px; font-size: 18px; font-weight: 600; }
  textarea { width: 100%; box-sizing: border-box; min-height: 80px; resize: vertical;
             background: rgba(0,0,0,0.32); color: #f5f5f7; border: 1px solid rgba(255,255,255,0.1);
             border-radius: 8px; padding: 8px 10px; font: inherit; }
  button { background: rgba(74,140,255,0.2); border: 1px solid rgba(74,140,255,0.4);
           color: #cfd8ff; border-radius: 8px; padding: 6px 14px; cursor: pointer; font: inherit; }
  button:disabled { opacity: 0.5; cursor: not-allowed; }
  pre { white-space: pre-wrap; word-wrap: break-word; background: rgba(255,255,255,0.05);
        padding: 10px 12px; border-radius: 8px; font-size: 13px; }
  .row { display: flex; gap: 8px; margin: 10px 0; align-items: center; }
  .err { color: #ff8080; font-size: 12px; }
</style></head>
<body>
  <h1>Спроси агента</h1>
  <textarea id="q" placeholder="Например: какая погода в Алматы?"></textarea>
  <div class="row">
    <button id="ask">Ask</button>
    <span id="err" class="err"></span>
  </div>
  <pre id="out"></pre>
<script>
let nextId = 1;
const pending = new Map();

window.addEventListener('message', (ev) => {
  const msg = ev.data;
  if (!msg || msg.source !== 'reflex') return;
  if (msg.type === 'response' && pending.has(msg.id)) {
    const cb = pending.get(msg.id);
    pending.delete(msg.id);
    if (msg.error) cb.reject(msg.error);
    else cb.resolve(msg.result);
  }
});

function reflexInvoke(method, params) {
  return new Promise((resolve, reject) => {
    const id = nextId++;
    pending.set(id, { resolve, reject });
    window.parent.postMessage({ source: 'reflex-app', type: 'request', id, method, params }, '*');
    setTimeout(() => {
      if (pending.has(id)) { pending.delete(id); reject(new Error('timeout')); }
    }, 120000);
  });
}

document.getElementById('ask').addEventListener('click', async () => {
  const btn = document.getElementById('ask');
  const err = document.getElementById('err');
  const out = document.getElementById('out');
  err.textContent = '';
  out.textContent = '…';
  btn.disabled = true;
  try {
    const prompt = document.getElementById('q').value || 'Привет!';
    const res = await reflexInvoke('agent.ask', { prompt });
    out.textContent = res.answer || JSON.stringify(res, null, 2);
  } catch (e) {
    err.textContent = String(e?.message || e);
    out.textContent = '';
  } finally {
    btn.disabled = false;
  }
});
</script>
</body></html>
"#;
