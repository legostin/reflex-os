use crate::memory::agents::envelope::Envelope;
use crate::memory::schema::{MemoryError, Result};
use std::path::PathBuf;
use tokio::sync::broadcast;

#[derive(Clone)]
pub struct MessageBus {
    _tx: broadcast::Sender<Envelope>,
    _persist_path: Option<PathBuf>,
}

impl MessageBus {
    pub fn new(_persist_path: Option<PathBuf>) -> Self {
        let (_tx, _) = broadcast::channel(256);
        Self {
            _tx,
            _persist_path,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Envelope> {
        self._tx.subscribe()
    }

    pub async fn send(&self, _env: Envelope) -> Result<()> {
        Err(MemoryError::Unimplemented("memory::agents::bus::send"))
    }

    pub async fn request(
        &self,
        _env: Envelope,
        _timeout_ms: u64,
    ) -> Result<Envelope> {
        Err(MemoryError::Unimplemented("memory::agents::bus::request"))
    }
}
