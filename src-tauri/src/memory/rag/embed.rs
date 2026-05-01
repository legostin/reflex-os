use crate::memory::rag::RagConfig;
use crate::memory::schema::{MemoryError, Result};

pub async fn embed_one(_cfg: &RagConfig, _text: &str) -> Result<Vec<f32>> {
    Err(MemoryError::Unimplemented("memory::rag::embed::embed_one"))
}

pub async fn embed_batch(_cfg: &RagConfig, _texts: &[String]) -> Result<Vec<Vec<f32>>> {
    Err(MemoryError::Unimplemented("memory::rag::embed::embed_batch"))
}
