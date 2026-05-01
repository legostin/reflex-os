pub mod chunk;
pub mod embed;
pub mod store;

use crate::memory::schema::Result;
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
        let ollama_url = std::env::var("REFLEX_OLLAMA_URL")
            .unwrap_or_else(|_| "http://localhost:11434".into());
        let embed_model = std::env::var("REFLEX_EMBED_MODEL").unwrap_or_else(|_| "bge-m3".into());
        Self {
            ollama_url,
            embed_model,
            embed_dim: 1024,
            max_chunk_chars: 1500,
        }
    }
}

const MAX_FILE_BYTES: u64 = 200 * 1024;
const MAX_INDEX_TEXT_BYTES: u64 = 2 * 1024 * 1024;

pub async fn index_text(
    project_root: &Path,
    doc_id: &str,
    kind: &str,
    text: &str,
) -> Result<()> {
    let cfg = RagConfig::default();
    let chunks = chunk::chunk_auto(text, None, &cfg);
    if chunks.is_empty() {
        let store = store::VecStore::open(project_root, cfg.embed_dim)?;
        store.upsert(doc_id, kind, None, &[])?;
        return Ok(());
    }
    let texts: Vec<String> = chunks.iter().map(|c| c.text.clone()).collect();
    let vectors = embed::embed_batch(&cfg, &texts).await?;
    let pairs: Vec<(String, Vec<f32>)> = texts.into_iter().zip(vectors.into_iter()).collect();
    let store = store::VecStore::open(project_root, cfg.embed_dim)?;
    store.upsert(doc_id, kind, None, &pairs)?;
    Ok(())
}

pub async fn index_file(project_root: &Path, path: &Path, kind: &str) -> Result<()> {
    let cfg = RagConfig::default();
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_file() {
        return Ok(());
    }
    if metadata.len() > MAX_INDEX_TEXT_BYTES {
        return Ok(());
    }
    let bytes = std::fs::read(path)?;
    let text = match std::str::from_utf8(&bytes) {
        Ok(s) => s.to_string(),
        Err(_) => return Ok(()),
    };
    let chunks = chunk::chunk_auto(&text, Some(path), &cfg);
    let doc_id = path.to_string_lossy().to_string();
    let store = store::VecStore::open(project_root, cfg.embed_dim)?;
    if chunks.is_empty() {
        store.upsert(&doc_id, kind, Some(path), &[])?;
        return Ok(());
    }
    let texts: Vec<String> = chunks.iter().map(|c| c.text.clone()).collect();
    let vectors = embed::embed_batch(&cfg, &texts).await?;
    let pairs: Vec<(String, Vec<f32>)> = texts.into_iter().zip(vectors.into_iter()).collect();
    store.upsert(&doc_id, kind, Some(path), &pairs)?;
    Ok(())
}

pub async fn search(project_root: &Path, query: &str, limit: usize) -> Result<Vec<RagHit>> {
    let cfg = RagConfig::default();
    let q = embed::embed_one(&cfg, query).await?;
    let store = store::VecStore::open(project_root, cfg.embed_dim)?;
    store.search(&q, limit)
}

pub async fn reindex_project(project_root: &Path) -> Result<usize> {
    let mut indexed = 0usize;
    let mut stack: Vec<PathBuf> = vec![project_root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(it) => it,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            if file_type.is_dir() {
                if should_skip_dir(project_root, &path) {
                    continue;
                }
                stack.push(path);
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            let metadata = match std::fs::metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if metadata.len() > MAX_FILE_BYTES {
                continue;
            }
            let kind = match classify(&path) {
                Some(k) => k,
                None => continue,
            };
            if index_file(project_root, &path, kind).await.is_ok() {
                indexed += 1;
            }
        }
    }
    Ok(indexed)
}

pub async fn forget(project_root: &Path, doc_id: &str) -> Result<()> {
    let cfg = RagConfig::default();
    let store = store::VecStore::open(project_root, cfg.embed_dim)?;
    store.forget(doc_id)
}

fn should_skip_dir(project_root: &Path, path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    matches!(
        name,
        ".git" | "node_modules" | "target" | "dist" | "build" | ".next" | ".turbo"
    ) || is_under(path, &project_root.join(".reflex").join("rag"))
        || is_under(path, &project_root.join(".reflex").join("topics"))
}

fn is_under(path: &Path, base: &Path) -> bool {
    let p = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let b = base.canonicalize().unwrap_or_else(|_| base.to_path_buf());
    p.starts_with(&b)
}

fn classify(path: &Path) -> Option<&'static str> {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    if matches!(
        name,
        "package-lock.json" | "pnpm-lock.yaml" | "yarn.lock" | "Cargo.lock"
    ) {
        return None;
    }
    let ext = path.extension().and_then(|s| s.to_str())?;
    match ext {
        "md" | "markdown" => Some("reference"),
        "rs" | "ts" | "tsx" | "js" | "jsx" => Some("project"),
        "json" => Some("reference"),
        _ => None,
    }
}
