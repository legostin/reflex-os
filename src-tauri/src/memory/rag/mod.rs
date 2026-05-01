pub mod chunk;
pub mod embed;
pub mod store;

use crate::memory::schema::{MemoryError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RagHit {
    pub doc_id: String,
    pub source: Option<PathBuf>,
    pub chunk: String,
    pub score: f32,
    pub kind: String,
}

#[derive(Debug, Clone)]
pub struct RagConfig {
    pub ollama_url: String,
    pub embed_model: String,
    pub embed_dim: usize,
    pub max_chunk_chars: usize,
}

impl Default for RagConfig {
    fn default() -> Self {
        Self {
            ollama_url: "http://localhost:11434".into(),
            embed_model: "bge-m3".into(),
            embed_dim: 1024,
            max_chunk_chars: 1500,
        }
    }
}

pub async fn index_text(
    _project_root: &Path,
    _doc_id: &str,
    _kind: &str,
    _text: &str,
) -> Result<()> {
    Err(MemoryError::Unimplemented("memory::rag::index_text"))
}

pub async fn index_file(_project_root: &Path, _path: &Path, _kind: &str) -> Result<()> {
    Err(MemoryError::Unimplemented("memory::rag::index_file"))
}

pub async fn search(_project_root: &Path, _query: &str, _limit: usize) -> Result<Vec<RagHit>> {
    Err(MemoryError::Unimplemented("memory::rag::search"))
}

pub async fn reindex_project(_project_root: &Path) -> Result<usize> {
    Err(MemoryError::Unimplemented("memory::rag::reindex_project"))
}

pub async fn forget(_project_root: &Path, _doc_id: &str) -> Result<()> {
    Err(MemoryError::Unimplemented("memory::rag::forget"))
}
