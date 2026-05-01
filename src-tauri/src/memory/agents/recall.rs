use crate::memory::rag::{self, RagHit};
use crate::memory::schema::{MemoryError, MemoryNote, MemoryRef, MemoryScope, Result, ScopeRoots};
use crate::memory::store::{self, ListFilter};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const EXCERPT_CHARS: usize = 400;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallRequest {
    pub project_root: String,
    pub thread_id: String,
    pub query: String,
    pub max_notes: usize,
    pub max_rag: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallResult {
    pub markdown: String,
    pub notes: Vec<MemoryRef>,
    pub rag: Vec<RagHit>,
}

pub async fn recall(req: RecallRequest) -> Result<RecallResult> {
    let project_root = PathBuf::from(&req.project_root);
    let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_default();
    let roots = ScopeRoots::resolve(&home, Some(&project_root), Some(&req.thread_id));

    let max_notes = req.max_notes.max(1);
    let project_take = max_notes / 2;
    let topic_take = max_notes.saturating_sub(project_take);

    let filter = ListFilter {
        kind: None,
        tag: None,
        query: if req.query.is_empty() {
            None
        } else {
            Some(req.query.clone())
        },
    };

    let project_notes_all = unwrap_or_empty(store::list(&roots, MemoryScope::Project, &filter));
    let topic_notes_all = unwrap_or_empty(store::list(&roots, MemoryScope::Topic, &filter));

    let project_notes: Vec<MemoryNote> = project_notes_all.into_iter().take(project_take).collect();
    let topic_notes: Vec<MemoryNote> = topic_notes_all.into_iter().take(topic_take).collect();

    let rag_hits: Vec<RagHit> =
        unwrap_or_empty(rag::search(&project_root, &req.query, req.max_rag).await);

    let markdown = compose_markdown(&project_notes, &topic_notes, &rag_hits);

    let mut note_refs: Vec<MemoryRef> = Vec::new();
    for n in project_notes.iter() {
        note_refs.push(MemoryRef {
            scope: n.scope,
            rel_path: n.rel_path.clone(),
        });
    }
    for n in topic_notes.iter() {
        note_refs.push(MemoryRef {
            scope: n.scope,
            rel_path: n.rel_path.clone(),
        });
    }

    Ok(RecallResult {
        markdown,
        notes: note_refs,
        rag: rag_hits,
    })
}

pub async fn run_subagent(
    project_root: &Path,
    thread_id: &str,
    query: &str,
) -> Result<RecallResult> {
    let req = RecallRequest {
        project_root: project_root.to_string_lossy().into_owned(),
        thread_id: thread_id.to_string(),
        query: query.to_string(),
        max_notes: 8,
        max_rag: 6,
    };
    recall(req).await
}

fn compose_markdown(project: &[MemoryNote], topic: &[MemoryNote], rag: &[RagHit]) -> String {
    let mut out = String::new();
    out.push_str("## Memory recall\n\n");

    out.push_str("### Project memory\n");
    if project.is_empty() {
        out.push_str("- _empty_\n");
    } else {
        for n in project {
            append_note_md(&mut out, n);
        }
    }
    out.push('\n');

    out.push_str("### Topic memory\n");
    if topic.is_empty() {
        out.push_str("- _empty_\n");
    } else {
        for n in topic {
            append_note_md(&mut out, n);
        }
    }
    out.push('\n');

    out.push_str("### Code/notes context (RAG)\n");
    if rag.is_empty() {
        out.push_str("- _empty_\n");
    } else {
        for h in rag {
            let src = h
                .source
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| h.doc_id.clone());
            out.push_str(&format!("- {} — score {:.3}\n", src, h.score));
            let excerpt = truncate(&h.chunk, EXCERPT_CHARS);
            out.push_str(&format!("  {excerpt}\n"));
        }
    }

    out
}

fn append_note_md(out: &mut String, n: &MemoryNote) {
    out.push_str(&format!(
        "- **{}** — {} ({})\n",
        n.front.name,
        n.front.description,
        n.rel_path.display()
    ));
    let excerpt = truncate(&n.body, EXCERPT_CHARS);
    if !excerpt.trim().is_empty() {
        out.push_str(&format!("  {excerpt}\n"));
    }
}

fn truncate(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut buf: String = s.chars().take(max).collect();
    buf.push_str("…");
    buf
}

fn unwrap_or_empty<T: Default>(r: Result<T>) -> T {
    match r {
        Ok(v) => v,
        Err(MemoryError::Unimplemented(_)) => T::default(),
        Err(e) => {
            eprintln!("[reflex] recall ignoring err: {e}");
            T::default()
        }
    }
}
