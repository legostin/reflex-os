use crate::memory::rag::RagHit;
use crate::memory::schema::{MemoryError, Result};
use std::path::Path;

pub struct VecStore;

impl VecStore {
    pub fn open(_project_root: &Path, _dim: usize) -> Result<Self> {
        Err(MemoryError::Unimplemented("memory::rag::store::open"))
    }

    pub fn upsert(
        &self,
        _doc_id: &str,
        _kind: &str,
        _source: Option<&Path>,
        _chunks: &[(String, Vec<f32>)],
    ) -> Result<()> {
        Err(MemoryError::Unimplemented("memory::rag::store::upsert"))
    }

    pub fn search(&self, _query_vec: &[f32], _limit: usize) -> Result<Vec<RagHit>> {
        Err(MemoryError::Unimplemented("memory::rag::store::search"))
    }

    pub fn forget(&self, _doc_id: &str) -> Result<()> {
        Err(MemoryError::Unimplemented("memory::rag::store::forget"))
    }
}
