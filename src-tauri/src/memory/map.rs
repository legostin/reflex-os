use crate::memory::schema::{MemoryError, MemoryScope, Result, ScopeRoots};

pub fn rebuild(_roots: &ScopeRoots, _scope: MemoryScope) -> Result<String> {
    Err(MemoryError::Unimplemented("memory::map::rebuild"))
}

pub fn rebuild_all(_roots: &ScopeRoots) -> Result<()> {
    Err(MemoryError::Unimplemented("memory::map::rebuild_all"))
}
