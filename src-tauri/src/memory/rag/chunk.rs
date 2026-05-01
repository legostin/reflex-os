use crate::memory::rag::RagConfig;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct Chunk {
    pub text: String,
    pub start: usize,
    pub end: usize,
    pub heading: Option<String>,
}

pub fn chunk_markdown(_text: &str, _cfg: &RagConfig) -> Vec<Chunk> {
    Vec::new()
}

pub fn chunk_code(_text: &str, _path: &Path, _cfg: &RagConfig) -> Vec<Chunk> {
    Vec::new()
}

pub fn chunk_auto(text: &str, path: Option<&Path>, cfg: &RagConfig) -> Vec<Chunk> {
    match path.and_then(|p| p.extension()).and_then(|s| s.to_str()) {
        Some("md") | Some("markdown") => chunk_markdown(text, cfg),
        Some(_) => chunk_code(text, path.unwrap_or(Path::new("")), cfg),
        None => chunk_markdown(text, cfg),
    }
}
