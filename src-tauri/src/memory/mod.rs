pub mod agents;
pub mod files;
pub mod injection;
pub mod map;
pub mod rag;
pub mod schema;
pub mod store;
pub mod tools;

pub use schema::{
    MemoryError, MemoryKind, MemoryNote, MemoryRef, MemoryScope, NoteFrontmatter, ScopeRoots,
};

use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct MemoryState {
    pub bus: agents::bus::MessageBus,
    pub indexed_threads: Arc<Mutex<HashSet<String>>>,
}

impl Default for MemoryState {
    fn default() -> Self {
        Self {
            bus: agents::bus::MessageBus::new(None),
            indexed_threads: Arc::new(Mutex::new(HashSet::new())),
        }
    }
}
