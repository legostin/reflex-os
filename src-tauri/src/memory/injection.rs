use crate::memory::agents::recall::{self, RecallResult};
use crate::memory::schema::{MemoryError, MemoryNote, MemoryScope, Result, ScopeRoots};
use crate::memory::store::{self, ListFilter};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct InjectionResult {
    pub preface: String,
    pub note_count: usize,
    pub rag_hit_count: usize,
}

fn home_path() -> Result<PathBuf> {
    std::env::var("HOME")
        .map(PathBuf::from)
        .map_err(|e| MemoryError::Other(format!("HOME not set: {e}")))
}

fn swallow_unimplemented<T: Default>(res: Result<T>) -> T {
    match res {
        Ok(v) => v,
        Err(MemoryError::Unimplemented(_)) => T::default(),
        Err(e) => {
            eprintln!("[reflex] memory injection sub-call failed: {e}");
            T::default()
        }
    }
}

fn render_notes(notes: &[MemoryNote]) -> String {
    let mut out = String::new();
    for note in notes {
        let name = note.front.name.trim();
        let desc = note.front.description.trim();
        if desc.is_empty() {
            out.push_str(&format!("- {name}\n"));
        } else {
            out.push_str(&format!("- {name} — {desc}\n"));
        }
    }
    out
}

pub async fn build_preface(
    project_root: &Path,
    thread_id: &str,
    user_prompt: &str,
) -> Result<InjectionResult> {
    let home = home_path()?;
    let scope_roots = ScopeRoots::resolve(&home, Some(project_root), Some(thread_id));

    let project_notes: Vec<MemoryNote> = swallow_unimplemented(store::list(
        &scope_roots,
        MemoryScope::Project,
        &ListFilter::default(),
    ));
    let topic_notes: Vec<MemoryNote> = swallow_unimplemented(store::list(
        &scope_roots,
        MemoryScope::Topic,
        &ListFilter::default(),
    ));

    let recall_result: RecallResult = match recall::run_subagent(project_root, thread_id, user_prompt).await {
        Ok(r) => r,
        Err(MemoryError::Unimplemented(_)) => RecallResult {
            markdown: String::new(),
            notes: Vec::new(),
            rag: Vec::new(),
        },
        Err(e) => {
            eprintln!("[reflex] memory recall failed: {e}");
            RecallResult {
                markdown: String::new(),
                notes: Vec::new(),
                rag: Vec::new(),
            }
        }
    };

    let note_count = project_notes.len() + topic_notes.len();
    let rag_hit_count = recall_result.rag.len();

    let project_md = render_notes(&project_notes);
    let topic_md = render_notes(&topic_notes);
    let recall_md = recall_result.markdown.trim();

    let has_any = !project_md.is_empty() || !topic_md.is_empty() || !recall_md.is_empty();

    let preface = if !has_any {
        String::new()
    } else {
        let mut buf = String::from("## Reflex memory\n");
        if !project_md.is_empty() {
            buf.push_str("\n### Project memory\n");
            buf.push_str(&project_md);
        }
        if !topic_md.is_empty() {
            buf.push_str("\n### Topic memory\n");
            buf.push_str(&topic_md);
        }
        if !recall_md.is_empty() {
            buf.push('\n');
            buf.push_str(recall_md);
            buf.push('\n');
        }
        buf
    };

    Ok(InjectionResult {
        preface,
        note_count,
        rag_hit_count,
    })
}

pub fn wrap_user_prompt(preface: &str, user_prompt: &str) -> String {
    if preface.trim().is_empty() {
        user_prompt.to_string()
    } else {
        format!("{preface}\n\n---\n\n{user_prompt}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_user_prompt_empty_preface() {
        assert_eq!(wrap_user_prompt("", "hello"), "hello");
        assert_eq!(wrap_user_prompt("   \n", "hello"), "hello");
    }

    #[test]
    fn wrap_user_prompt_with_preface() {
        let out = wrap_user_prompt("## ctx", "do thing");
        assert!(out.contains("## ctx"));
        assert!(out.contains("do thing"));
        assert!(out.contains("---"));
    }

    #[tokio::test]
    async fn build_preface_swallows_unimplemented() {
        let tmp = std::env::temp_dir().join(format!(
            "reflex-mem-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let res = build_preface(&tmp, "t_test", "hello").await;
        assert!(res.is_ok(), "build_preface should swallow unimplemented");
        let r = res.unwrap();
        assert_eq!(r.preface, "");
        assert_eq!(r.note_count, 0);
        assert_eq!(r.rag_hit_count, 0);
    }
}
