use crate::memory::map;
use crate::memory::schema::{
    slug, MemoryError, MemoryKind, MemoryNote, MemoryScope, NoteFrontmatter, Result, ScopeRoots,
    MAP_FILE,
};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

pub fn parse_note(_path: &Path, raw: &str) -> Result<(NoteFrontmatter, String)> {
    let rest = raw.strip_prefix("---\n").ok_or_else(|| {
        MemoryError::InvalidFrontmatter("missing leading ---".to_string())
    })?;
    let end = rest.find("\n---\n").ok_or_else(|| {
        MemoryError::InvalidFrontmatter("missing closing ---".to_string())
    })?;
    let yaml = &rest[..end];
    let body_start = end + "\n---\n".len();
    let body_raw = &rest[body_start..];
    let body = body_raw.strip_prefix('\n').unwrap_or(body_raw).to_string();
    let front: NoteFrontmatter = serde_yaml::from_str(yaml)?;
    Ok((front, body))
}

pub fn render_note(front: &NoteFrontmatter, body: &str) -> String {
    let yaml = serde_yaml::to_string(front).unwrap_or_default();
    let yaml_trimmed = yaml.trim_end_matches('\n');
    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(yaml_trimmed);
    out.push_str("\n---\n\n");
    out.push_str(body);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

pub fn save(roots: &ScopeRoots, req: SaveRequest) -> Result<MemoryNote> {
    let root = roots
        .root_for(req.scope)
        .ok_or(MemoryError::ScopeUnavailable(req.scope))?
        .to_path_buf();

    let rel_path = req
        .rel_path
        .clone()
        .unwrap_or_else(|| PathBuf::from(format!("{}.md", slug(&req.name))));
    let abs = root.join(&rel_path);

    if let Some(parent) = abs.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let now = now_ms();
    let (id, created_at_ms) = if abs.exists() {
        let raw = std::fs::read_to_string(&abs)?;
        match parse_note(&abs, &raw) {
            Ok((existing, _)) => (existing.id, existing.created_at_ms),
            Err(_) => (uuid::Uuid::new_v4().to_string(), now),
        }
    } else {
        (uuid::Uuid::new_v4().to_string(), now)
    };

    let front = NoteFrontmatter {
        id,
        name: req.name,
        description: req.description,
        kind: req.kind,
        tags: req.tags,
        created_at_ms,
        updated_at_ms: now,
        source: req.source,
    };

    let rendered = render_note(&front, &req.body);
    std::fs::write(&abs, &rendered)?;

    match map::rebuild(roots, req.scope) {
        Ok(_) => {}
        Err(MemoryError::Unimplemented(_)) => {}
        Err(e) => return Err(e),
    }

    Ok(MemoryNote {
        scope: req.scope,
        path: abs,
        rel_path,
        front,
        body: req.body,
    })
}

pub fn read(roots: &ScopeRoots, scope: MemoryScope, rel_path: &Path) -> Result<MemoryNote> {
    let root = roots
        .root_for(scope)
        .ok_or(MemoryError::ScopeUnavailable(scope))?
        .to_path_buf();
    let abs = root.join(rel_path);
    if !abs.exists() {
        return Err(MemoryError::NotFound(abs.display().to_string()));
    }
    let raw = std::fs::read_to_string(&abs)?;
    let (front, body) = parse_note(&abs, &raw)?;
    Ok(MemoryNote {
        scope,
        path: abs,
        rel_path: rel_path.to_path_buf(),
        front,
        body,
    })
}

pub fn delete(roots: &ScopeRoots, scope: MemoryScope, rel_path: &Path) -> Result<()> {
    let root = roots
        .root_for(scope)
        .ok_or(MemoryError::ScopeUnavailable(scope))?
        .to_path_buf();
    let abs = root.join(rel_path);
    if !abs.exists() {
        return Err(MemoryError::NotFound(abs.display().to_string()));
    }
    std::fs::remove_file(&abs)?;

    let mut cur = abs.parent().map(|p| p.to_path_buf());
    while let Some(dir) = cur {
        if dir == root {
            break;
        }
        if !dir.starts_with(&root) {
            break;
        }
        match std::fs::read_dir(&dir) {
            Ok(mut it) => {
                if it.next().is_some() {
                    break;
                }
            }
            Err(_) => break,
        }
        if std::fs::remove_dir(&dir).is_err() {
            break;
        }
        cur = dir.parent().map(|p| p.to_path_buf());
    }

    match map::rebuild(roots, scope) {
        Ok(_) => {}
        Err(MemoryError::Unimplemented(_)) => {}
        Err(e) => return Err(e),
    }
    Ok(())
}

fn collect_md_files(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let ft = entry.file_type()?;
        if ft.is_dir() {
            collect_md_files(root, &path, out)?;
        } else if ft.is_file() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str == MAP_FILE {
                continue;
            }
            if name_str.ends_with(".md") {
                out.push(path);
            }
        }
    }
    Ok(())
}

pub fn list(roots: &ScopeRoots, scope: MemoryScope, filter: &ListFilter) -> Result<Vec<MemoryNote>> {
    let root = roots
        .root_for(scope)
        .ok_or(MemoryError::ScopeUnavailable(scope))?
        .to_path_buf();
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    collect_md_files(&root, &root, &mut files)?;

    let mut notes: Vec<MemoryNote> = Vec::new();
    for path in files {
        let raw = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let (front, body) = match parse_note(&path, &raw) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let rel_path = path
            .strip_prefix(&root)
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|_| path.clone());
        let note = MemoryNote {
            scope,
            path,
            rel_path,
            front,
            body,
        };
        if !matches_filter(&note, filter) {
            continue;
        }
        notes.push(note);
    }

    notes.sort_by(|a, b| b.front.updated_at_ms.cmp(&a.front.updated_at_ms));
    Ok(notes)
}

pub fn list_all(roots: &ScopeRoots, filter: &ListFilter) -> Result<Vec<MemoryNote>> {
    let scopes = [MemoryScope::Global, MemoryScope::Project, MemoryScope::Topic];
    let mut all = Vec::new();
    for scope in scopes {
        if roots.root_for(scope).is_none() {
            continue;
        }
        match list(roots, scope, filter) {
            Ok(mut v) => all.append(&mut v),
            Err(MemoryError::ScopeUnavailable(_)) => {}
            Err(e) => return Err(e),
        }
    }
    all.sort_by(|a, b| b.front.updated_at_ms.cmp(&a.front.updated_at_ms));
    Ok(all)
}

fn matches_filter(note: &MemoryNote, filter: &ListFilter) -> bool {
    if let Some(k) = filter.kind {
        if note.front.kind != k {
            return false;
        }
    }
    if let Some(t) = &filter.tag {
        if !note.front.tags.iter().any(|x| x == t) {
            return false;
        }
    }
    if let Some(q) = &filter.query {
        let needle = q.to_lowercase();
        let hay = format!(
            "{} {} {}",
            note.front.name, note.front.description, note.body
        )
        .to_lowercase();
        if !hay.contains(&needle) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::schema::{slug, MemoryKind, NoteFrontmatter, ScopeRoots};
    use std::path::PathBuf;

    fn unique_tempdir(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("reflex-mem-{}-{}-{}", label, pid, nanos));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn slug_basic() {
        assert_eq!(slug("Hello World"), "hello-world");
        assert_eq!(slug("  Foo  Bar!! "), "foo-bar");
        assert_eq!(slug("Привет"), "note");
        assert_eq!(slug("a/b c"), "a-b-c");
        assert_eq!(slug("UPPER_case-9"), "upper-case-9");
    }

    #[test]
    fn parse_render_roundtrip() {
        let mut front = NoteFrontmatter::new(
            "My Note".to_string(),
            "desc".to_string(),
            MemoryKind::User,
        );
        front.id = "abc".to_string();
        front.created_at_ms = 100;
        front.updated_at_ms = 200;
        front.tags = vec!["a".to_string(), "b".to_string()];

        let body = "hello\nworld";
        let rendered = render_note(&front, body);
        assert!(rendered.starts_with("---\n"));
        assert!(rendered.ends_with('\n'));
        assert!(rendered.contains("\n---\n\n"));

        let (parsed, parsed_body) =
            parse_note(std::path::Path::new("x.md"), &rendered).expect("parse");
        assert_eq!(parsed.id, "abc");
        assert_eq!(parsed.name, "My Note");
        assert_eq!(parsed.kind, MemoryKind::User);
        assert_eq!(parsed.tags, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(parsed_body.trim_end(), body);
    }

    #[test]
    fn render_omits_empty_tags_and_source() {
        let mut front = NoteFrontmatter::new(
            "n".to_string(),
            "d".to_string(),
            MemoryKind::Fact,
        );
        front.id = "id".to_string();
        let rendered = render_note(&front, "body");
        assert!(!rendered.contains("tags"));
        assert!(!rendered.contains("source"));
    }

    #[test]
    fn save_list_delete_cycle() {
        let dir = unique_tempdir("cycle");
        let roots = ScopeRoots {
            global: dir.clone(),
            project: None,
            topic: None,
        };

        let req = SaveRequest {
            scope: MemoryScope::Global,
            kind: MemoryKind::User,
            name: "Hello World".to_string(),
            description: "desc".to_string(),
            body: "body content".to_string(),
            rel_path: None,
            tags: vec!["x".to_string()],
            source: None,
        };
        let note = save(&roots, req).expect("save");
        assert!(note.path.exists());
        assert_eq!(note.rel_path, PathBuf::from("hello-world.md"));
        assert!(!note.front.id.is_empty());
        assert!(note.front.created_at_ms > 0);
        assert_eq!(note.front.created_at_ms, note.front.updated_at_ms);

        let listed = list(&roots, MemoryScope::Global, &ListFilter::default()).expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].front.name, "Hello World");

        let by_kind = list(
            &roots,
            MemoryScope::Global,
            &ListFilter {
                kind: Some(MemoryKind::User),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(by_kind.len(), 1);

        let by_tag = list(
            &roots,
            MemoryScope::Global,
            &ListFilter {
                tag: Some("x".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(by_tag.len(), 1);

        let by_q = list(
            &roots,
            MemoryScope::Global,
            &ListFilter {
                query: Some("BODY".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(by_q.len(), 1);

        let none = list(
            &roots,
            MemoryScope::Global,
            &ListFilter {
                tag: Some("nope".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(none.len(), 0);

        let original_id = note.front.id.clone();
        let original_created = note.front.created_at_ms;
        std::thread::sleep(std::time::Duration::from_millis(2));
        let req2 = SaveRequest {
            scope: MemoryScope::Global,
            kind: MemoryKind::User,
            name: "Hello World".to_string(),
            description: "desc2".to_string(),
            body: "new body".to_string(),
            rel_path: None,
            tags: vec![],
            source: None,
        };
        let updated = save(&roots, req2).expect("re-save");
        assert_eq!(updated.front.id, original_id);
        assert_eq!(updated.front.created_at_ms, original_created);
        assert!(updated.front.updated_at_ms >= original_created);

        delete(&roots, MemoryScope::Global, &PathBuf::from("hello-world.md")).expect("delete");
        let listed_after = list(&roots, MemoryScope::Global, &ListFilter::default()).unwrap();
        assert_eq!(listed_after.len(), 0);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
