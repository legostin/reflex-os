use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

const PROJECT_DIRNAME: &str = ".reflex";
const PROJECT_FILE: &str = "project.json";
const REGISTRY_FILE: &str = "projects.json";

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub root: String,
    pub created_at_ms: u128,
    #[serde(default = "default_sandbox")]
    pub sandbox: String,
    #[serde(default)]
    pub mcp_servers: Option<Value>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub agent_instructions: Option<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub apps: Vec<String>,
}

fn default_sandbox() -> String {
    "workspace-write".into()
}

pub fn write_project(root: &Path, project: &Project) -> io::Result<()> {
    let path = project_dir(root).join(PROJECT_FILE);
    fs::write(
        path,
        serde_json::to_string_pretty(project).map_err(io_err)?,
    )
}

pub fn project_dir(root: &Path) -> PathBuf {
    root.join(PROJECT_DIRNAME)
}

pub fn topics_dir(root: &Path) -> PathBuf {
    project_dir(root).join("topics")
}

pub fn project_exists(root: &Path) -> bool {
    project_dir(root).join(PROJECT_FILE).is_file()
}

pub fn read_project_at(root: &Path) -> io::Result<Project> {
    let path = project_dir(root).join(PROJECT_FILE);
    let s = fs::read_to_string(path)?;
    serde_json::from_str(&s).map_err(io_err)
}

/// Walk up from `path` looking for an ancestor that's a Reflex project root.
pub fn find_project_for(path: &Path) -> Option<Project> {
    let start = if path.is_file() {
        path.parent().map(|p| p.to_path_buf())?
    } else {
        path.to_path_buf()
    };
    let mut current = Some(start);
    while let Some(dir) = current {
        if project_exists(&dir) {
            return read_project_at(&dir).ok();
        }
        current = dir.parent().map(|p| p.to_path_buf());
    }
    None
}

pub fn create_project(
    app: &AppHandle,
    root: &Path,
    name: Option<String>,
    description: Option<String>,
) -> io::Result<Project> {
    fs::create_dir_all(topics_dir(root))?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let final_name = name
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            root.file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| "Project".to_string())
        });
    let id = format!("p_{now_ms}");
    let project = Project {
        id,
        name: final_name,
        root: root.to_string_lossy().into_owned(),
        created_at_ms: now_ms,
        sandbox: default_sandbox(),
        mcp_servers: None,
        description: description.filter(|s| !s.trim().is_empty()),
        agent_instructions: None,
        skills: Vec::new(),
        apps: Vec::new(),
    };
    let path = project_dir(root).join(PROJECT_FILE);
    fs::write(path, serde_json::to_string_pretty(&project).map_err(io_err)?)?;
    register(app, &project)?;
    Ok(project)
}

fn registry_path(app: &AppHandle) -> io::Result<PathBuf> {
    let base = app
        .path()
        .app_data_dir()
        .map_err(|e| io_err(format!("app_data_dir: {e}")))?;
    fs::create_dir_all(&base)?;
    Ok(base.join(REGISTRY_FILE))
}

pub fn list_registered(app: &AppHandle) -> io::Result<Vec<Project>> {
    let path = registry_path(app)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let s = fs::read_to_string(path)?;
    let projects: Vec<Project> = serde_json::from_str(&s).unwrap_or_default();
    Ok(projects
        .into_iter()
        .filter(|p| {
            let path = PathBuf::from(&p.root);
            project_exists(&path)
        })
        .collect())
}

pub fn get_by_id(app: &AppHandle, id: &str) -> io::Result<Option<Project>> {
    Ok(list_registered(app)?.into_iter().find(|p| p.id == id))
}

pub fn register(app: &AppHandle, project: &Project) -> io::Result<()> {
    let path = registry_path(app)?;
    let mut list: Vec<Project> = if path.exists() {
        serde_json::from_str(&fs::read_to_string(&path)?).unwrap_or_default()
    } else {
        Vec::new()
    };
    list.retain(|p| p.id != project.id && p.root != project.root);
    list.push(project.clone());
    fs::write(path, serde_json::to_string_pretty(&list).map_err(io_err)?)?;
    Ok(())
}

pub fn deregister_by_root(app: &AppHandle, root: &Path) -> io::Result<()> {
    let path = registry_path(app)?;
    if !path.exists() {
        return Ok(());
    }
    let mut list: Vec<Project> =
        serde_json::from_str(&fs::read_to_string(&path)?).unwrap_or_default();
    let before = list.len();
    let target = root.to_string_lossy().into_owned();
    list.retain(|p| p.root != target);
    if list.len() == before {
        return Ok(());
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(&list).map_err(io_err)?)?;
    fs::rename(tmp, path)
}

/// Suggest nearby existing projects when none was found at `path`. Returns
/// registered projects whose root shares a path prefix with `path`, sorted
/// by closest first.
pub fn nearest_registered(app: &AppHandle, path: &Path) -> io::Result<Vec<Project>> {
    let mut all = list_registered(app)?;
    let path_str = path.to_string_lossy();
    all.sort_by_key(|p| {
        let common = common_prefix_len(&p.root, &path_str);
        std::cmp::Reverse(common)
    });
    Ok(all.into_iter().take(5).collect())
}

fn common_prefix_len(a: &str, b: &str) -> usize {
    a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count()
}

fn io_err<E: std::fmt::Display>(e: E) -> io::Error {
    io::Error::new(io::ErrorKind::Other, e.to_string())
}
