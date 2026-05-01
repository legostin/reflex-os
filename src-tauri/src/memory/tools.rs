use crate::memory::agents::recall::RecallResult;
use crate::memory::rag::RagHit;
use crate::memory::schema::{MemoryKind, MemoryNote, MemoryScope};
use serde::Deserialize;
use serde_json::Value;
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

#[tauri::command]
pub async fn memory_save(_app: AppHandle, _args: SaveArgs) -> Result<MemoryNote, String> {
    Err("memory_save: unimplemented".into())
}

#[tauri::command]
pub async fn memory_list(
    _app: AppHandle,
    _scope: MemoryScope,
    _project_root: Option<String>,
    _thread_id: Option<String>,
    _filter: Option<Value>,
) -> Result<Vec<MemoryNote>, String> {
    Err("memory_list: unimplemented".into())
}

#[tauri::command]
pub async fn memory_delete(
    _app: AppHandle,
    _scope: MemoryScope,
    _rel_path: String,
    _project_root: Option<String>,
    _thread_id: Option<String>,
) -> Result<(), String> {
    Err("memory_delete: unimplemented".into())
}

#[tauri::command]
pub async fn memory_search(
    _app: AppHandle,
    _query: String,
    _project_root: String,
    _limit: Option<usize>,
) -> Result<Vec<RagHit>, String> {
    Err("memory_search: unimplemented".into())
}

#[tauri::command]
pub async fn memory_recall(
    _app: AppHandle,
    _project_root: String,
    _thread_id: String,
    _query: String,
) -> Result<RecallResult, String> {
    Err("memory_recall: unimplemented".into())
}

#[tauri::command]
pub async fn memory_reindex(_app: AppHandle, _project_root: String) -> Result<usize, String> {
    Err("memory_reindex: unimplemented".into())
}
