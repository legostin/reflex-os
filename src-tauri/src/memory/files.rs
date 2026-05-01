use crate::memory::rag::{self, store::VecStore, RagConfig};
use crate::memory::schema::{MemoryError, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command as TokioCommand;

pub const MAX_TEXT_BYTES: u64 = 1024 * 1024;
pub const MAX_IMAGE_BYTES: u64 = 5 * 1024 * 1024;

const IMAGE_DESCRIBE_PROMPT: &str = "Опиши содержимое этой картинки кратко и информативно (на языке, на котором написан текст в самой картинке, либо на русском). Если есть текст — приведи его дословно. Если это скриншот UI/кода — перечисли видимые элементы и заметные строки. 3-8 предложений, без вступлений и markdown-обёрток.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FileClass {
    Text,
    Code,
    Image,
    Binary,
    TooLarge,
    Unsupported,
}

impl FileClass {
    pub fn rag_kind(&self) -> &'static str {
        match self {
            FileClass::Text => "reference",
            FileClass::Code => "project",
            FileClass::Image => "image",
            _ => "other",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct IndexOutcome {
    pub indexed: usize,
    pub skipped: Vec<SkippedItem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkippedItem {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PathStatus {
    pub path: String,
    pub kind: String,
    pub class: FileClass,
    pub indexed: bool,
    pub indexed_under: Option<usize>,
    pub indexed_at_ms: Option<u64>,
    pub modified_ms: Option<u64>,
    pub stale: bool,
}

pub fn classify_path(path: &Path) -> Result<FileClass> {
    let meta = std::fs::metadata(path)?;
    if !meta.is_file() {
        return Ok(FileClass::Unsupported);
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase());
    let is_image = matches!(
        ext.as_deref(),
        Some("png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "heic")
    );
    if is_image {
        if meta.len() > MAX_IMAGE_BYTES {
            return Ok(FileClass::TooLarge);
        }
        return Ok(FileClass::Image);
    }
    if is_lockfile(path) {
        return Ok(FileClass::Unsupported);
    }
    if meta.len() > MAX_TEXT_BYTES {
        return Ok(FileClass::TooLarge);
    }
    let bytes = std::fs::read(path)?;
    if !is_probably_text(&bytes) {
        return Ok(FileClass::Binary);
    }
    let class = match ext.as_deref() {
        Some("rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go" | "rb" | "java" | "kt" | "swift"
        | "c" | "cc" | "cpp" | "h" | "hpp" | "cs" | "php" | "scala") => FileClass::Code,
        _ => FileClass::Text,
    };
    Ok(class)
}

fn is_lockfile(path: &Path) -> bool {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    matches!(
        name,
        "package-lock.json" | "pnpm-lock.yaml" | "yarn.lock" | "Cargo.lock" | "Gemfile.lock"
    )
}

fn is_probably_text(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return true;
    }
    let head = &bytes[..bytes.len().min(8192)];
    if std::str::from_utf8(head).is_err() {
        return false;
    }
    let nul = head.iter().filter(|b| **b == 0).count();
    nul < head.len() / 200 + 1
}

pub async fn index_path(project_root: &Path, path: &Path) -> Result<IndexOutcome> {
    let meta = std::fs::metadata(path)
        .map_err(|e| MemoryError::Other(format!("metadata {}: {e}", path.display())))?;

    if meta.is_dir() {
        return index_dir(project_root, path).await;
    }

    let class = classify_path(path)?;
    let mut outcome = IndexOutcome {
        indexed: 0,
        skipped: Vec::new(),
    };
    match index_one(project_root, path, class).await {
        Ok(true) => outcome.indexed = 1,
        Ok(false) => outcome.skipped.push(SkippedItem {
            path: path.to_string_lossy().into_owned(),
            reason: skip_reason(class).into(),
        }),
        Err(e) => outcome.skipped.push(SkippedItem {
            path: path.to_string_lossy().into_owned(),
            reason: e.to_string(),
        }),
    }
    Ok(outcome)
}

async fn index_dir(project_root: &Path, dir: &Path) -> Result<IndexOutcome> {
    let mut outcome = IndexOutcome {
        indexed: 0,
        skipped: Vec::new(),
    };
    let mut stack: Vec<PathBuf> = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let entries = match std::fs::read_dir(&d) {
            Ok(e) => e,
            Err(e) => {
                outcome.skipped.push(SkippedItem {
                    path: d.to_string_lossy().into_owned(),
                    reason: format!("read_dir: {e}"),
                });
                continue;
            }
        };
        for entry in entries.flatten() {
            let p = entry.path();
            let ft = match entry.file_type() {
                Ok(t) => t,
                Err(_) => continue,
            };
            if ft.is_dir() {
                if should_skip_dir(project_root, &p) {
                    continue;
                }
                stack.push(p);
                continue;
            }
            if !ft.is_file() {
                continue;
            }
            let class = match classify_path(&p) {
                Ok(c) => c,
                Err(_) => continue,
            };
            match index_one(project_root, &p, class).await {
                Ok(true) => outcome.indexed += 1,
                Ok(false) => {}
                Err(e) => outcome.skipped.push(SkippedItem {
                    path: p.to_string_lossy().into_owned(),
                    reason: e.to_string(),
                }),
            }
        }
    }
    Ok(outcome)
}

fn should_skip_dir(project_root: &Path, path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    if matches!(
        name,
        ".git" | "node_modules" | "target" | "dist" | "build" | ".next" | ".turbo"
    ) {
        return true;
    }
    let rag = project_root.join(".reflex").join("rag");
    let topics = project_root.join(".reflex").join("topics");
    let p = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    p.starts_with(rag.canonicalize().unwrap_or(rag)) || p.starts_with(topics.canonicalize().unwrap_or(topics))
}

fn skip_reason(class: FileClass) -> &'static str {
    match class {
        FileClass::Binary => "binary file",
        FileClass::TooLarge => "exceeds size limit",
        FileClass::Unsupported => "unsupported file type",
        _ => "skipped",
    }
}

async fn index_one(project_root: &Path, path: &Path, class: FileClass) -> Result<bool> {
    match class {
        FileClass::Text | FileClass::Code => {
            rag::index_file(project_root, path, class.rag_kind()).await?;
            Ok(true)
        }
        FileClass::Image => {
            let description = describe_image(project_root, path).await?;
            let cfg = RagConfig::default();
            let chunks = crate::memory::rag::chunk::chunk_auto(&description, None, &cfg);
            if chunks.is_empty() {
                return Ok(false);
            }
            let texts: Vec<String> = chunks.iter().map(|c| c.text.clone()).collect();
            let vectors = crate::memory::rag::embed::embed_batch(&cfg, &texts).await?;
            if vectors.len() != texts.len() {
                return Err(MemoryError::Rag("image embed count mismatch".into()));
            }
            let pairs: Vec<(String, Vec<f32>)> =
                texts.into_iter().zip(vectors.into_iter()).collect();
            let store = VecStore::open(project_root, cfg.embed_dim)?;
            let doc_id = path.to_string_lossy().to_string();
            store.upsert(&doc_id, "image", Some(path), &pairs)?;
            Ok(true)
        }
        FileClass::Binary | FileClass::TooLarge | FileClass::Unsupported => Ok(false),
    }
}

async fn describe_image(project_root: &Path, path: &Path) -> Result<String> {
    let cwd_str = project_root.to_string_lossy().into_owned();
    let img_str = path.to_string_lossy().into_owned();

    let out_path = std::env::temp_dir().join(format!(
        "reflex-img-{}.txt",
        uuid::Uuid::new_v4().simple()
    ));
    let out_str = out_path.to_string_lossy().into_owned();

    let result = TokioCommand::new("codex")
        .args([
            "exec",
            "--skip-git-repo-check",
            "-s",
            "read-only",
            "--output-last-message",
            &out_str,
            "-C",
            &cwd_str,
            "-i",
            &img_str,
            "--",
            IMAGE_DESCRIBE_PROMPT,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| MemoryError::Other(format!("codex spawn: {e}")))?;

    if !result.status.success() {
        let _ = std::fs::remove_file(&out_path);
        return Err(MemoryError::Other(format!(
            "codex describe non-zero: {}",
            result.status
        )));
    }

    let raw = std::fs::read_to_string(&out_path)
        .map_err(|e| MemoryError::Other(format!("read describe out: {e}")))?;
    let _ = std::fs::remove_file(&out_path);

    let trimmed = raw.trim().to_string();
    if trimmed.is_empty() {
        return Err(MemoryError::Other("empty image description".into()));
    }
    Ok(format!(
        "Image: {}\n\n{}",
        path.display(),
        trimmed
    ))
}

pub fn status(project_root: &Path, path: &Path) -> Result<PathStatus> {
    let cfg = RagConfig::default();
    let store = VecStore::open(project_root, cfg.embed_dim)?;
    status_with_store(&store, path)
}

pub fn status_batch(project_root: &Path, paths: &[PathBuf]) -> Result<Vec<PathStatus>> {
    let cfg = RagConfig::default();
    let store = VecStore::open(project_root, cfg.embed_dim)?;
    let mut out = Vec::with_capacity(paths.len());
    for p in paths {
        match status_with_store(&store, p) {
            Ok(s) => out.push(s),
            Err(_) => out.push(PathStatus {
                path: p.to_string_lossy().into_owned(),
                kind: "missing".into(),
                class: FileClass::Unsupported,
                indexed: false,
                indexed_under: None,
                indexed_at_ms: None,
                modified_ms: None,
                stale: false,
            }),
        }
    }
    Ok(out)
}

fn status_with_store(store: &VecStore, path: &Path) -> Result<PathStatus> {
    let class = classify_path(path)?;
    let doc_id = path.to_string_lossy().to_string();
    let meta = std::fs::metadata(path)?;
    let kind = if meta.is_dir() {
        "directory".to_string()
    } else if meta.is_file() {
        "file".to_string()
    } else {
        "other".to_string()
    };

    let modified_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64);

    let (indexed, indexed_under, indexed_at_ms) = if meta.is_dir() {
        let prefix = format!("{}/", path.to_string_lossy());
        let n = store.count_under(&prefix)?;
        (n > 0, Some(n), None)
    } else {
        let has = store.has_doc(&doc_id)?;
        let when = if has {
            store.last_indexed_at(&doc_id)?
        } else {
            None
        };
        (has, None, when)
    };

    let stale = match (indexed, modified_ms, indexed_at_ms) {
        (true, Some(m), Some(i)) => m > i + 1_000,
        _ => false,
    };

    Ok(PathStatus {
        path: doc_id,
        kind,
        class,
        indexed,
        indexed_under,
        indexed_at_ms,
        modified_ms,
        stale,
    })
}

pub async fn forget_path(project_root: &Path, path: &Path) -> Result<usize> {
    let cfg = RagConfig::default();
    let store = VecStore::open(project_root, cfg.embed_dim)?;
    let meta_res = std::fs::metadata(path);
    if let Ok(meta) = meta_res {
        if meta.is_dir() {
            let prefix = format!("{}/", path.to_string_lossy());
            return store.forget_under(&prefix);
        }
    }
    let doc_id = path.to_string_lossy().to_string();
    store.forget(&doc_id)?;
    Ok(1)
}
