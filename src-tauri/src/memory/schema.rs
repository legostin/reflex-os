use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryScope {
    Global,
    Project,
    Topic,
}

impl MemoryScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            MemoryScope::Global => "global",
            MemoryScope::Project => "project",
            MemoryScope::Topic => "topic",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryKind {
    User,
    Project,
    Feedback,
    Reference,
    Tool,
    System,
    Fact,
}

impl MemoryKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            MemoryKind::User => "user",
            MemoryKind::Project => "project",
            MemoryKind::Feedback => "feedback",
            MemoryKind::Reference => "reference",
            MemoryKind::Tool => "tool",
            MemoryKind::System => "system",
            MemoryKind::Fact => "fact",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteFrontmatter {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(rename = "type")]
    pub kind: MemoryKind,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub created_at_ms: u128,
    #[serde(default)]
    pub updated_at_ms: u128,
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryNote {
    pub scope: MemoryScope,
    pub path: PathBuf,
    pub rel_path: PathBuf,
    pub front: NoteFrontmatter,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRef {
    pub scope: MemoryScope,
    pub rel_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ScopeRoots {
    pub global: PathBuf,
    pub project: Option<PathBuf>,
    pub topic: Option<PathBuf>,
}

impl ScopeRoots {
    pub fn root_for(&self, scope: MemoryScope) -> Option<&Path> {
        match scope {
            MemoryScope::Global => Some(self.global.as_path()),
            MemoryScope::Project => self.project.as_deref(),
            MemoryScope::Topic => self.topic.as_deref(),
        }
    }

    pub fn resolve(global_home: &Path, project_root: Option<&Path>, topic_id: Option<&str>) -> Self {
        let project = project_root.map(|p| p.join(".reflex").join("memory"));
        let topic = match (project_root, topic_id) {
            (Some(p), Some(t)) => Some(p.join(".reflex").join("topics").join(t).join("memory")),
            _ => None,
        };
        ScopeRoots {
            global: global_home.join(".reflex").join("memory"),
            project,
            topic,
        }
    }
}

#[derive(Debug, Error)]
pub enum MemoryError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("yaml: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid frontmatter: {0}")]
    InvalidFrontmatter(String),
    #[error("scope unavailable: {0:?}")]
    ScopeUnavailable(MemoryScope),
    #[error("rag: {0}")]
    Rag(String),
    #[error("ollama: {0}")]
    Ollama(String),
    #[error("bus: {0}")]
    Bus(String),
    #[error("unimplemented: {0}")]
    Unimplemented(&'static str),
    #[error("{0}")]
    Other(String),
}

impl From<reqwest::Error> for MemoryError {
    fn from(e: reqwest::Error) -> Self {
        MemoryError::Other(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, MemoryError>;

pub const MEMORY_DIR: &str = "memory";
pub const MAP_FILE: &str = "MEMORY.md";
pub const RAG_DIR: &str = "rag";
pub const BUS_LOG: &str = "agents/bus.jsonl";
