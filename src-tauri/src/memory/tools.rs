use crate::memory::agents::recall::{self, RecallRequest, RecallResult};
use crate::memory::files::{self, IndexOutcome, PathStatus};
use crate::memory::rag::{self, RagHit};
use crate::memory::schema::{MemoryKind, MemoryNote, MemoryScope, ScopeRoots};
use crate::memory::store::{self, ListFilter, SaveRequest};
use serde::Deserialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tauri::AppHandle;

#[derive(Deserialize)]
pub struct SaveArgs {
    pub scope: MemoryScope,
    pub kind: MemoryKind,
    pub name: String,
    pub description: String,
    pub body: String,
    pub project_root: Option<String>,
    pub thread_id: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub source: Option<String>,
}

fn home_path() -> Result<PathBuf, String> {
    std::env::var("HOME")
        .map(PathBuf::from)
        .map_err(|e| e.to_string())
}

fn roots(
    project_root: Option<&str>,
    thread_id: Option<&str>,
) -> Result<ScopeRoots, String> {
    let home = home_path()?;
    let project = project_root.map(Path::new);
    Ok(ScopeRoots::resolve(&home, project, thread_id))
}

fn parse_filter(filter: Option<Value>) -> Result<ListFilter, String> {
    let Some(value) = filter else {
        return Ok(ListFilter::default());
    };
    if value.is_null() {
        return Ok(ListFilter::default());
    }
    #[derive(Deserialize, Default)]
    struct RawFilter {
        kind: Option<MemoryKind>,
        tag: Option<String>,
        query: Option<String>,
    }
    let raw: RawFilter = serde_json::from_value(value).map_err(|e| e.to_string())?;
    Ok(ListFilter {
        kind: raw.kind,
        tag: raw.tag,
        query: raw.query,
    })
}

#[tauri::command]
pub async fn memory_save(_app: AppHandle, args: SaveArgs) -> Result<MemoryNote, String> {
    let SaveArgs {
        scope,
        kind,
        name,
        description,
        body,
        project_root,
        thread_id,
        tags,
        source,
    } = args;

    let scope_roots = roots(project_root.as_deref(), thread_id.as_deref())?;

    let resolved_source = match scope {
        MemoryScope::Topic => source.clone().or_else(|| thread_id.clone()),
        _ => source.clone(),
    };

    let req = SaveRequest {
        scope,
        kind,
        name,
        description,
        body: body.clone(),
        rel_path: None,
        tags,
        source: resolved_source,
    };

    let note = store::save(&scope_roots, req).map_err(|e| e.to_string())?;

    if let Some(root) = project_root {
        let doc_id = format!("memory:{}", note.rel_path.display());
        let body_for_index = body.clone();
        tokio::spawn(async move {
            let _ = rag::index_text(Path::new(&root), &doc_id, "memory", &body_for_index).await;
        });
    }

    Ok(note)
}

#[tauri::command]
pub async fn memory_list(
    _app: AppHandle,
    scope: MemoryScope,
    project_root: Option<String>,
    thread_id: Option<String>,
    filter: Option<Value>,
) -> Result<Vec<MemoryNote>, String> {
    let scope_roots = roots(project_root.as_deref(), thread_id.as_deref())?;
    let filter = parse_filter(filter)?;
    store::list(&scope_roots, scope, &filter).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn memory_delete(
    _app: AppHandle,
    scope: MemoryScope,
    rel_path: String,
    project_root: Option<String>,
    thread_id: Option<String>,
) -> Result<(), String> {
    let scope_roots = roots(project_root.as_deref(), thread_id.as_deref())?;
    let rel = PathBuf::from(&rel_path);
    store::delete(&scope_roots, scope, &rel).map_err(|e| e.to_string())?;

    if let Some(root) = project_root {
        let doc_id = format!("memory:{}", rel_path);
        tokio::spawn(async move {
            let _ = rag::forget(Path::new(&root), &doc_id).await;
        });
    }

    Ok(())
}

#[tauri::command]
pub async fn memory_search(
    _app: AppHandle,
    query: String,
    project_root: String,
    limit: Option<usize>,
) -> Result<Vec<RagHit>, String> {
    rag::search(Path::new(&project_root), &query, limit.unwrap_or(8))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn memory_recall(
    _app: AppHandle,
    project_root: String,
    thread_id: String,
    query: String,
) -> Result<RecallResult, String> {
    let req = RecallRequest {
        project_root,
        thread_id,
        query,
        max_notes: 8,
        max_rag: 6,
    };
    recall::recall(req).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn memory_reindex(_app: AppHandle, project_root: String) -> Result<usize, String> {
    rag::reindex_project(Path::new(&project_root))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn memory_index_path(
    _app: AppHandle,
    project_root: String,
    path: String,
) -> Result<IndexOutcome, String> {
    files::index_path(Path::new(&project_root), Path::new(&path))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn memory_path_status(
    _app: AppHandle,
    project_root: String,
    path: String,
) -> Result<PathStatus, String> {
    let project_root = PathBuf::from(project_root);
    let path = PathBuf::from(path);
    tokio::task::spawn_blocking(move || files::status(&project_root, &path))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn memory_path_status_batch(
    _app: AppHandle,
    project_root: String,
    paths: Vec<String>,
) -> Result<Vec<PathStatus>, String> {
    let project_root = PathBuf::from(project_root);
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    tokio::task::spawn_blocking(move || files::status_batch(&project_root, &paths))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn memory_forget_path(
    _app: AppHandle,
    project_root: String,
    path: String,
) -> Result<usize, String> {
    files::forget_path(Path::new(&project_root), Path::new(&path))
        .await
        .map_err(|e| e.to_string())
}
