use crate::memory::rag::RagHit;
use crate::memory::schema::{MemoryError, MemoryRef, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallRequest {
    pub project_root: String,
    pub thread_id: String,
    pub query: String,
    pub max_notes: usize,
    pub max_rag: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallResult {
    pub markdown: String,
    pub notes: Vec<MemoryRef>,
    pub rag: Vec<RagHit>,
}

pub async fn recall(_req: RecallRequest) -> Result<RecallResult> {
    Err(MemoryError::Unimplemented("memory::agents::recall::recall"))
}

pub async fn run_subagent(_project_root: &Path, _thread_id: &str, _query: &str) -> Result<RecallResult> {
    Err(MemoryError::Unimplemented("memory::agents::recall::run_subagent"))
}
