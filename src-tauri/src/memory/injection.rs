use crate::memory::schema::{MemoryError, Result};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct InjectionResult {
    pub preface: String,
    pub note_count: usize,
    pub rag_hit_count: usize,
}

pub async fn build_preface(
    _project_root: &Path,
    _thread_id: &str,
    _user_prompt: &str,
) -> Result<InjectionResult> {
    Err(MemoryError::Unimplemented(
        "memory::injection::build_preface",
    ))
}

pub fn wrap_user_prompt(preface: &str, user_prompt: &str) -> String {
    if preface.trim().is_empty() {
        user_prompt.to_string()
    } else {
        format!("{preface}\n\n---\n\n{user_prompt}")
    }
}
