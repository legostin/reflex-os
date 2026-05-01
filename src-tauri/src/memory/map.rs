use crate::memory::schema::{MemoryError, MemoryScope, Result, ScopeRoots, MAP_FILE};
use crate::memory::store::parse_note;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn collect(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let ft = entry.file_type()?;
        if ft.is_dir() {
            collect(root, &path, out)?;
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

pub fn rebuild(roots: &ScopeRoots, scope: MemoryScope) -> Result<String> {
    let root = roots
        .root_for(scope)
        .ok_or(MemoryError::ScopeUnavailable(scope))?
        .to_path_buf();
    if !root.exists() {
        return Ok(String::new());
    }

    let mut files = Vec::new();
    collect(&root, &root, &mut files)?;

    let mut groups: BTreeMap<String, Vec<(String, String, String)>> = BTreeMap::new();
    for path in files {
        let rel = match path.strip_prefix(&root) {
            Ok(p) => p.to_path_buf(),
            Err(_) => continue,
        };
        let dir_key = match rel.parent() {
            Some(p) if p.as_os_str().is_empty() => "/".to_string(),
            Some(p) => format!("{}/", p.to_string_lossy().replace('\\', "/")),
            None => "/".to_string(),
        };
        let raw = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let (front, _body) = match parse_note(&path, &raw) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        groups
            .entry(dir_key)
            .or_default()
            .push((front.name, front.description, rel_str));
    }

    for v in groups.values_mut() {
        v.sort_by(|a, b| a.2.cmp(&b.2));
    }

    let mut out = String::new();
    out.push_str(&format!("# Memory map: {}\n\n", scope.as_str()));
    out.push_str("_Auto-generated. Do not edit by hand._\n\n");

    let mut keys: Vec<&String> = groups.keys().collect();
    keys.sort_by(|a, b| {
        if a.as_str() == "/" && b.as_str() != "/" {
            std::cmp::Ordering::Less
        } else if a.as_str() != "/" && b.as_str() == "/" {
            std::cmp::Ordering::Greater
        } else {
            a.cmp(b)
        }
    });

    for key in keys {
        out.push_str(&format!("## {}\n", key));
        for (name, desc, rel) in &groups[key] {
            out.push_str(&format!("- [{}]({}) — {}\n", name, rel, desc));
        }
        out.push('\n');
    }

    let map_path = root.join(MAP_FILE);
    std::fs::write(&map_path, &out)?;
    Ok(out)
}

pub fn rebuild_all(roots: &ScopeRoots) -> Result<()> {
    let scopes = [MemoryScope::Global, MemoryScope::Project, MemoryScope::Topic];
    let mut first_err: Option<MemoryError> = None;
    for scope in scopes {
        if roots.root_for(scope).is_none() {
            continue;
        }
        if let Err(e) = rebuild(roots, scope) {
            if first_err.is_none() {
                first_err = Some(e);
            }
        }
    }
    match first_err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}
