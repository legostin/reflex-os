use crate::memory::rag::RagConfig;
use crate::memory::schema::{MemoryError, Result};
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize)]
struct EmbedBatchResponse {
    #[serde(default)]
    embeddings: Option<Vec<Vec<f32>>>,
}

#[derive(Debug, Deserialize)]
struct EmbedSingleResponse {
    #[serde(default)]
    embedding: Option<Vec<f32>>,
}

pub async fn embed_one(cfg: &RagConfig, text: &str) -> Result<Vec<f32>> {
    let mut v = embed_via_batch(cfg, &[text.to_string()]).await?;
    if let Some(first) = v.pop() {
        return Ok(first);
    }
    embed_via_legacy(cfg, text).await
}

pub async fn embed_batch(cfg: &RagConfig, texts: &[String]) -> Result<Vec<Vec<f32>>> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    match embed_via_batch(cfg, texts).await {
        Ok(v) if v.len() == texts.len() => Ok(v),
        Ok(_) | Err(_) => {
            let mut out = Vec::with_capacity(texts.len());
            for t in texts {
                out.push(embed_via_legacy(cfg, t).await?);
            }
            Ok(out)
        }
    }
}

async fn embed_via_batch(cfg: &RagConfig, texts: &[String]) -> Result<Vec<Vec<f32>>> {
    let url = format!("{}/api/embed", cfg.ollama_url.trim_end_matches('/'));
    let body = json!({"model": cfg.embed_model, "input": texts});
    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| MemoryError::Ollama(format!("embed request failed: {e}")))?;
    if resp.status().as_u16() == 404 {
        return Err(MemoryError::Ollama("embed endpoint 404".into()));
    }
    if !resp.status().is_success() {
        let s = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(MemoryError::Ollama(format!("embed status {s}: {body}")));
    }
    let parsed: EmbedBatchResponse = resp
        .json()
        .await
        .map_err(|e| MemoryError::Ollama(format!("embed parse failed: {e}")))?;
    parsed
        .embeddings
        .ok_or_else(|| MemoryError::Ollama("embed response missing embeddings".into()))
}

async fn embed_via_legacy(cfg: &RagConfig, text: &str) -> Result<Vec<f32>> {
    let url = format!("{}/api/embeddings", cfg.ollama_url.trim_end_matches('/'));
    let body = json!({"model": cfg.embed_model, "prompt": text});
    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| MemoryError::Ollama(format!("legacy embed request failed: {e}")))?;
    if !resp.status().is_success() {
        let s = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(MemoryError::Ollama(format!("legacy embed status {s}: {body}")));
    }
    let parsed: EmbedSingleResponse = resp
        .json()
        .await
        .map_err(|e| MemoryError::Ollama(format!("legacy embed parse failed: {e}")))?;
    parsed
        .embedding
        .ok_or_else(|| MemoryError::Ollama("legacy embed response missing embedding".into()))
}
