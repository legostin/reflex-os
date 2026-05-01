use crate::memory::schema::{
    MemoryError, MemoryKind, MemoryNote, MemoryScope, NoteFrontmatter, Result, ScopeRoots,
};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SaveRequest {
    pub scope: MemoryScope,
    pub kind: MemoryKind,
    pub name: String,
    pub description: String,
    pub body: String,
    pub rel_path: Option<PathBuf>,
    pub tags: Vec<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ListFilter {
    pub kind: Option<MemoryKind>,
    pub tag: Option<String>,
    pub query: Option<String>,
}

pub fn save(_roots: &ScopeRoots, _req: SaveRequest) -> Result<MemoryNote> {
    Err(MemoryError::Unimplemented("memory::store::save"))
}

pub fn read(_roots: &ScopeRoots, _scope: MemoryScope, _rel_path: &Path) -> Result<MemoryNote> {
    Err(MemoryError::Unimplemented("memory::store::read"))
}

pub fn delete(_roots: &ScopeRoots, _scope: MemoryScope, _rel_path: &Path) -> Result<()> {
    Err(MemoryError::Unimplemented("memory::store::delete"))
}

pub fn list(_roots: &ScopeRoots, _scope: MemoryScope, _filter: &ListFilter) -> Result<Vec<MemoryNote>> {
    Err(MemoryError::Unimplemented("memory::store::list"))
}

pub fn list_all(_roots: &ScopeRoots, _filter: &ListFilter) -> Result<Vec<MemoryNote>> {
    Err(MemoryError::Unimplemented("memory::store::list_all"))
}

pub fn parse_note(_path: &Path, _raw: &str) -> Result<(NoteFrontmatter, String)> {
    Err(MemoryError::Unimplemented("memory::store::parse_note"))
}

pub fn render_note(_front: &NoteFrontmatter, _body: &str) -> String {
    String::new()
}
