use crate::memory::agents::MessageBus;
use crate::memory::schema::{MemoryError, Result};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct IndexerConfig {
    pub idle_debounce_ms: u64,
}

impl Default for IndexerConfig {
    fn default() -> Self {
        Self {
            idle_debounce_ms: 60_000,
        }
    }
}

pub async fn run(
    _bus: MessageBus,
    _project_root: PathBuf,
    _thread_id: String,
    _cfg: IndexerConfig,
) -> Result<()> {
    Err(MemoryError::Unimplemented("memory::agents::indexer::run"))
}

pub async fn index_thread_once(
    _project_root: &std::path::Path,
    _thread_id: &str,
) -> Result<usize> {
    Err(MemoryError::Unimplemented(
        "memory::agents::indexer::index_thread_once",
    ))
}
