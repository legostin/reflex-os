pub mod agents;
pub mod injection;
pub mod map;
pub mod rag;
pub mod schema;
pub mod store;
pub mod tools;

pub use schema::{
    MemoryError, MemoryKind, MemoryNote, MemoryRef, MemoryScope, NoteFrontmatter, ScopeRoots,
};

use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Default, Clone)]
pub struct MemoryState {
    pub bus: Arc<Mutex<Option<agents::bus::MessageBus>>>,
}
