use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

const PROJECT_DIRNAME: &str = ".reflex";
const PROJECT_FILE: &str = "project.json";
const REGISTRY_FILE: &str = "projects.json";
const PROJECT_FOLDERS_FILE: &str = "project-folders.json";

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

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct ProjectFolder {
    pub path: String,
    pub name: String,
    #[serde(default)]
    pub parent_path: Option<String>,
    #[serde(default)]
    pub project_count: usize,
    #[serde(default)]
    pub created_at_ms: u128,
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

pub fn normalize_project_folder_name(name: &str) -> io::Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "folder name is empty",
        ));
    }
    if name == "." || name == ".." || name.eq_ignore_ascii_case(PROJECT_DIRNAME) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid folder name: {name}"),
        ));
    }
    if name.contains('/') || name.contains('\\') || name.chars().any(|ch| ch.is_control()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid folder name: {name}"),
        ));
    }
    Ok(name.to_string())
}

pub fn create_project_folder(parent: &Path, name: &str) -> io::Result<PathBuf> {
    if !parent.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("parent folder not found: {}", parent.display()),
        ));
    }
    let name = normalize_project_folder_name(name)?;
    let target = parent.join(name);
    fs::create_dir(&target)?;
    Ok(target)
}

pub fn rename_project_folder(path: &Path, name: &str) -> io::Result<PathBuf> {
    if path.file_name().and_then(|n| n.to_str()) == Some(PROJECT_DIRNAME) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cannot rename .reflex",
        ));
    }
    if !path.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("folder not found: {}", path.display()),
        ));
    }
    let name = normalize_project_folder_name(name)?;
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "folder has no parent")
    })?;
    let target = parent.join(name);
    if target.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("target folder already exists: {}", target.display()),
        ));
    }
    fs::rename(path, &target)?;
    Ok(target)
}

pub fn move_project_to_folder_on_disk(mut project: Project, target_parent: &Path) -> io::Result<Project> {
    if !target_parent.exists() {
        fs::create_dir_all(target_parent)?;
    }
    if !target_parent.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("target is not a folder: {}", target_parent.display()),
        ));
    }
    let source = PathBuf::from(&project.root);
    if !project_exists(&source) {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("project root not found: {}", source.display()),
        ));
    }
    let source_canon = source.canonicalize()?;
    let target_parent_canon = target_parent.canonicalize()?;
    if target_parent_canon.starts_with(&source_canon) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cannot move a project into itself",
        ));
    }
    let name = source.file_name().and_then(|n| n.to_str()).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "project root has no folder name")
    })?;
    let target = target_parent.join(name);
    if target.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("target project already exists: {}", target.display()),
        ));
    }
    fs::rename(&source, &target)?;
    project.root = target.to_string_lossy().into_owned();
    write_project(&target, &project)?;
    Ok(project)
}

fn project_folders_path(app: &AppHandle) -> io::Result<PathBuf> {
    let base = app
        .path()
        .app_data_dir()
        .map_err(|e| io_err(format!("app_data_dir: {e}")))?;
    fs::create_dir_all(&base)?;
    Ok(base.join(PROJECT_FOLDERS_FILE))
}

fn folder_record(path: &Path, created_at_ms: u128) -> ProjectFolder {
    ProjectFolder {
        path: path.to_string_lossy().into_owned(),
        name: path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string(),
        parent_path: path.parent().map(|p| p.to_string_lossy().into_owned()),
        project_count: 0,
        created_at_ms,
    }
}

fn read_project_folder_registry(app: &AppHandle) -> io::Result<Vec<ProjectFolder>> {
    let path = project_folders_path(app)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let folders: Vec<ProjectFolder> = serde_json::from_str(&fs::read_to_string(path)?)
        .unwrap_or_default();
    Ok(folders
        .into_iter()
        .filter(|folder| PathBuf::from(&folder.path).is_dir())
        .collect())
}

fn write_project_folder_registry(app: &AppHandle, folders: &[ProjectFolder]) -> io::Result<()> {
    let path = project_folders_path(app)?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(folders).map_err(io_err)?)?;
    fs::rename(tmp, path)
}

pub fn list_project_folders(app: &AppHandle) -> io::Result<Vec<ProjectFolder>> {
    let mut folders = read_project_folder_registry(app)?;
    for project in list_registered(app)? {
        if let Some(parent) = PathBuf::from(&project.root).parent().map(|p| p.to_path_buf()) {
            let path = parent.to_string_lossy().into_owned();
            if !folders.iter().any(|folder| folder.path == path) {
                folders.push(folder_record(&parent, 0));
            }
        }
    }
    for folder in &mut folders {
        folder.project_count = list_registered(app)?
            .into_iter()
            .filter(|project| {
                PathBuf::from(&project.root)
                    .parent()
                    .map(|parent| parent == PathBuf::from(&folder.path))
                    .unwrap_or(false)
            })
            .count();
    }
    folders.sort_by(|a, b| a.path.to_lowercase().cmp(&b.path.to_lowercase()));
    Ok(folders)
}

pub fn create_project_folder_registered(
    app: &AppHandle,
    parent_path: &Path,
    name: &str,
) -> io::Result<ProjectFolder> {
    let path = create_project_folder(parent_path, name)?;
    let mut folders = read_project_folder_registry(app)?;
    let folder = folder_record(&path, timestamp_ms());
    folders.retain(|entry| entry.path != folder.path);
    folders.push(folder.clone());
    write_project_folder_registry(app, &folders)?;
    Ok(folder)
}

fn path_is_same_or_child(parent: &Path, candidate: &Path) -> bool {
    candidate == parent || candidate.starts_with(parent)
}

pub fn rename_project_folder_registered(
    app: &AppHandle,
    path: &Path,
    name: &str,
) -> io::Result<ProjectFolder> {
    let old = path.canonicalize()?;
    let new = rename_project_folder(path, name)?;
    let mut folders = read_project_folder_registry(app)?;
    for folder in &mut folders {
        let folder_path = PathBuf::from(&folder.path);
        if path_is_same_or_child(&old, &folder_path) {
            let suffix = folder_path.strip_prefix(&old).unwrap_or(Path::new(""));
            let next = new.join(suffix);
            *folder = folder_record(&next, folder.created_at_ms);
        }
    }
    if !folders.iter().any(|folder| folder.path == new.to_string_lossy()) {
        folders.push(folder_record(&new, timestamp_ms()));
    }
    write_project_folder_registry(app, &folders)?;

    for project in list_registered(app)? {
        let root = PathBuf::from(&project.root);
        if path_is_same_or_child(&old, &root) {
            let suffix = root.strip_prefix(&old).unwrap_or(Path::new(""));
            let mut updated = project;
            updated.root = new.join(suffix).to_string_lossy().into_owned();
            write_project(&PathBuf::from(&updated.root), &updated)?;
            register(app, &updated)?;
        }
    }
    Ok(folder_record(&new, timestamp_ms()))
}

pub fn delete_project_folder_registered(app: &AppHandle, path: &Path) -> io::Result<()> {
    if !path.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("folder not found: {}", path.display()),
        ));
    }
    if fs::read_dir(path)?.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "folder is not empty",
        ));
    }
    fs::remove_dir(path)?;
    let removed = path.to_string_lossy().into_owned();
    let mut folders = read_project_folder_registry(app)?;
    folders.retain(|folder| folder.path != removed);
    write_project_folder_registry(app, &folders)
}

pub fn move_project_to_folder_registered(
    app: &AppHandle,
    project_id: &str,
    folder_path: &Path,
) -> io::Result<Project> {
    let project = get_by_id(app, project_id)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("project not found: {project_id}")))?;
    let moved = move_project_to_folder_on_disk(project, folder_path)?;
    register(app, &moved)?;
    let mut folders = read_project_folder_registry(app)?;
    let folder = folder_record(folder_path, timestamp_ms());
    if !folders.iter().any(|entry| entry.path == folder.path) {
        folders.push(folder);
        write_project_folder_registry(app, &folders)?;
    }
    Ok(moved)
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
    if let Some(existing) = find_project_for(root) {
        register(app, &existing)?;
        return Ok(existing);
    }

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

fn timestamp_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn io_err<E: std::fmt::Display>(e: E) -> io::Error {
    io::Error::new(io::ErrorKind::Other, e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_project(root: &Path) -> Project {
        Project {
            id: "p_test".into(),
            name: "Moved Project".into(),
            root: root.to_string_lossy().into_owned(),
            created_at_ms: 1,
            sandbox: default_sandbox(),
            mcp_servers: None,
            description: None,
            agent_instructions: None,
            skills: Vec::new(),
            apps: Vec::new(),
        }
    }

    #[test]
    fn project_folder_names_reject_path_separators_and_reflex_dir() {
        assert_eq!(normalize_project_folder_name(" Ops ").expect("name"), "Ops");
        assert!(normalize_project_folder_name("../Ops").is_err());
        assert!(normalize_project_folder_name(".reflex").is_err());
        assert!(normalize_project_folder_name("").is_err());
    }

    #[test]
    fn move_project_to_folder_renames_root_and_rewrites_project_file() {
        let base = std::env::temp_dir().join(format!(
            "reflex-project-move-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let source = base.join("Source");
        let target_parent = base.join("Folders").join("Ops");
        fs::create_dir_all(project_dir(&source)).expect("source project dir");
        fs::write(source.join("README.md"), "hello").expect("readme");
        let project = test_project(&source);
        write_project(&source, &project).expect("write project");

        let moved = move_project_to_folder_on_disk(project, &target_parent).expect("move project");
        let moved_root = target_parent.join("Source");

        assert_eq!(moved.root, moved_root.to_string_lossy());
        assert!(!source.exists());
        assert!(moved_root.join("README.md").is_file());
        let persisted = read_project_at(&moved_root).expect("read moved project");
        assert_eq!(persisted.root, moved_root.to_string_lossy());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn find_project_for_returns_ancestor_project_for_nested_path() {
        let base = std::env::temp_dir().join(format!(
            "reflex-project-find-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let root = base.join("Project");
        let nested = root.join("src").join("feature");
        fs::create_dir_all(&nested).expect("nested dir");
        fs::create_dir_all(project_dir(&root)).expect("project dir");
        let project = test_project(&root);
        write_project(&root, &project).expect("write project");

        let found = find_project_for(&nested).expect("found ancestor project");
        assert_eq!(found.id, project.id);
        assert_eq!(found.root, root.to_string_lossy());
        assert!(!project_exists(&nested));
        let _ = fs::remove_dir_all(base);
    }
}
