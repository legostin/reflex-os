use crate::memory::rag::RagConfig;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct Chunk {
    pub text: String,
    pub start: usize,
    pub end: usize,
    pub heading: Option<String>,
}

pub fn chunk_markdown(text: &str, cfg: &RagConfig) -> Vec<Chunk> {
    let max = cfg.max_chunk_chars.max(1);
    let mut out = Vec::new();
    let mut heading_stack: Vec<(usize, String)> = Vec::new();

    let mut sections: Vec<(usize, usize, Option<String>)> = Vec::new();
    let mut section_start: usize = 0;
    let mut section_heading: Option<String> = None;

    let bytes = text.as_bytes();
    let mut line_start: usize = 0;
    while line_start <= bytes.len() {
        let line_end = match bytes[line_start..].iter().position(|&b| b == b'\n') {
            Some(p) => line_start + p,
            None => bytes.len(),
        };
        let line = &text[line_start..line_end];
        if let Some((level, title)) = parse_atx_heading(line) {
            if line_start > section_start {
                sections.push((section_start, line_start, section_heading.clone()));
            }
            while let Some((lvl, _)) = heading_stack.last() {
                if *lvl >= level {
                    heading_stack.pop();
                } else {
                    break;
                }
            }
            heading_stack.push((level, title));
            section_heading = Some(
                heading_stack
                    .iter()
                    .map(|(_, t)| t.as_str())
                    .collect::<Vec<_>>()
                    .join(" > "),
            );
            section_start = line_start;
        }
        if line_end == bytes.len() {
            if section_start < bytes.len() {
                sections.push((section_start, bytes.len(), section_heading.clone()));
            }
            break;
        }
        line_start = line_end + 1;
    }

    for (s, e, heading) in sections {
        let raw = &text[s..e];
        if raw.trim().is_empty() {
            continue;
        }
        if raw.len() <= max {
            out.push(Chunk {
                text: raw.to_string(),
                start: s,
                end: e,
                heading,
            });
        } else {
            split_by_paragraphs(raw, s, max, heading, &mut out);
        }
    }

    out
}

fn parse_atx_heading(line: &str) -> Option<(usize, String)> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with('#') {
        return None;
    }
    let mut level = 0usize;
    for ch in trimmed.chars() {
        if ch == '#' {
            level += 1;
            if level > 6 {
                return None;
            }
        } else {
            break;
        }
    }
    if level == 0 || level > 6 {
        return None;
    }
    let rest = &trimmed[level..];
    if !rest.starts_with(|c: char| c.is_whitespace()) {
        return None;
    }
    let title = rest.trim().to_string();
    if title.is_empty() {
        return None;
    }
    Some((level, title))
}

fn split_by_paragraphs(
    block: &str,
    base_offset: usize,
    max: usize,
    heading: Option<String>,
    out: &mut Vec<Chunk>,
) {
    let mut paragraphs: Vec<(usize, usize)> = Vec::new();
    let bytes = block.as_bytes();
    let mut i = 0usize;
    let mut para_start = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            let mut j = i + 1;
            let mut blank = false;
            while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'\n' {
                blank = true;
            }
            if blank {
                if para_start < i {
                    paragraphs.push((para_start, i));
                }
                while j < bytes.len() && bytes[j] == b'\n' {
                    j += 1;
                }
                para_start = j;
                i = j;
                continue;
            }
        }
        i += 1;
    }
    if para_start < bytes.len() {
        paragraphs.push((para_start, bytes.len()));
    }

    let mut buf_start: Option<usize> = None;
    let mut buf_end: usize = 0;
    let flush = |start: Option<usize>, end: usize, out: &mut Vec<Chunk>| {
        if let Some(s) = start {
            if s < end {
                let slice = &block[s..end];
                if !slice.trim().is_empty() {
                    out.push(Chunk {
                        text: slice.to_string(),
                        start: base_offset + s,
                        end: base_offset + end,
                        heading: heading.clone(),
                    });
                }
            }
        }
    };

    for (ps, pe) in paragraphs {
        let plen = pe - ps;
        if plen > max {
            flush(buf_start.take(), buf_end, out);
            let mut k = ps;
            while k < pe {
                let end = (k + max).min(pe);
                let slice = &block[k..end];
                if !slice.trim().is_empty() {
                    out.push(Chunk {
                        text: slice.to_string(),
                        start: base_offset + k,
                        end: base_offset + end,
                        heading: heading.clone(),
                    });
                }
                k = end;
            }
            continue;
        }
        match buf_start {
            None => {
                buf_start = Some(ps);
                buf_end = pe;
            }
            Some(bs) => {
                if pe - bs <= max {
                    buf_end = pe;
                } else {
                    flush(Some(bs), buf_end, out);
                    buf_start = Some(ps);
                    buf_end = pe;
                }
            }
        }
    }
    flush(buf_start, buf_end, out);
}

pub fn chunk_code(text: &str, _path: &Path, cfg: &RagConfig) -> Vec<Chunk> {
    let max = cfg.max_chunk_chars.max(1);
    let mut out = Vec::new();
    let bytes = text.as_bytes();

    let mut starts: Vec<usize> = Vec::new();
    let mut line_start = 0usize;
    while line_start <= bytes.len() {
        let line_end = match bytes[line_start..].iter().position(|&b| b == b'\n') {
            Some(p) => line_start + p,
            None => bytes.len(),
        };
        let line = &text[line_start..line_end];
        if line_matches_symbol(line) {
            starts.push(line_start);
        }
        if line_end == bytes.len() {
            break;
        }
        line_start = line_end + 1;
    }

    if starts.is_empty() {
        return sliding_window(text, max, 100);
    }

    if starts[0] > 0 {
        let pre = &text[0..starts[0]];
        if !pre.trim().is_empty() {
            push_block(0, starts[0], pre, max, None, &mut out);
        }
    }

    for i in 0..starts.len() {
        let s = starts[i];
        let e = if i + 1 < starts.len() {
            starts[i + 1]
        } else {
            text.len()
        };
        let block = &text[s..e];
        if block.trim().is_empty() {
            continue;
        }
        let first_line = block.lines().next().unwrap_or("").trim();
        let heading = if first_line.is_empty() {
            None
        } else {
            let mut h = first_line.to_string();
            if h.len() > 80 {
                let mut cut = 80;
                while cut > 0 && !h.is_char_boundary(cut) {
                    cut -= 1;
                }
                h.truncate(cut);
            }
            Some(h)
        };
        push_block(s, e, block, max, heading, &mut out);
    }

    out
}

fn line_matches_symbol(line: &str) -> bool {
    let l = line.trim_start();
    let prefixes: &[&str] = &[
        "pub fn ",
        "pub async fn ",
        "pub(crate) fn ",
        "pub(super) fn ",
        "pub struct ",
        "pub enum ",
        "pub trait ",
        "pub mod ",
        "pub const ",
        "pub static ",
        "fn ",
        "async fn ",
        "struct ",
        "enum ",
        "impl ",
        "trait ",
        "mod ",
        "export default function ",
        "export default class ",
        "export default const ",
        "export function ",
        "export class ",
        "export const ",
        "export let ",
        "export var ",
        "export async function ",
        "function ",
        "async function ",
        "class ",
        "const ",
        "let ",
        "var ",
    ];
    for p in prefixes {
        if l.starts_with(p) {
            let rest = &l[p.len()..];
            if rest
                .chars()
                .next()
                .map(|c| c.is_alphanumeric() || c == '_' || c == '<' || c == '*' || c == '\'')
                .unwrap_or(false)
            {
                return true;
            }
        }
    }
    false
}

fn push_block(
    start: usize,
    end: usize,
    block: &str,
    max: usize,
    heading: Option<String>,
    out: &mut Vec<Chunk>,
) {
    if block.len() <= max {
        out.push(Chunk {
            text: block.to_string(),
            start,
            end,
            heading,
        });
        return;
    }
    split_by_paragraphs(block, start, max, heading, out);
}

fn sliding_window(text: &str, max: usize, overlap: usize) -> Vec<Chunk> {
    let mut out = Vec::new();
    if text.trim().is_empty() {
        return out;
    }
    let step = max.saturating_sub(overlap).max(1);
    let bytes = text.len();
    let mut i = 0usize;
    while i < bytes {
        let end = (i + max).min(bytes);
        let mut s = i;
        while s < end && !text.is_char_boundary(s) {
            s += 1;
        }
        let mut e = end;
        while e > s && !text.is_char_boundary(e) {
            e -= 1;
        }
        let slice = &text[s..e];
        if !slice.trim().is_empty() {
            out.push(Chunk {
                text: slice.to_string(),
                start: s,
                end: e,
                heading: None,
            });
        }
        if end >= bytes {
            break;
        }
        i += step;
    }
    out
}

pub fn chunk_auto(text: &str, path: Option<&Path>, cfg: &RagConfig) -> Vec<Chunk> {
    match path.and_then(|p| p.extension()).and_then(|s| s.to_str()) {
        Some("md") | Some("markdown") => chunk_markdown(text, cfg),
        Some(_) => chunk_code(text, path.unwrap_or(Path::new("")), cfg),
        None => chunk_markdown(text, cfg),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(max: usize) -> RagConfig {
        RagConfig {
            max_chunk_chars: max,
            ..RagConfig::default()
        }
    }

    #[test]
    fn markdown_splits_by_headings() {
        let text =
            "# A\n\nintro line.\n\n## A.1\n\nbody one\n\n# B\n\nbody two with more text\n";
        let chunks = chunk_markdown(text, &cfg(1500));
        assert!(chunks.len() >= 3);
        assert!(chunks.iter().any(|c| c.heading.as_deref() == Some("A")));
        assert!(chunks
            .iter()
            .any(|c| c.heading.as_deref() == Some("A > A.1")));
        assert!(chunks.iter().any(|c| c.heading.as_deref() == Some("B")));
        for c in &chunks {
            assert_eq!(&text[c.start..c.end], c.text);
        }
    }

    #[test]
    fn markdown_packs_paragraphs_under_limit() {
        let big = "x".repeat(800);
        let text = format!("# H\n\n{big}\n\n{big}\n\n{big}\n");
        let chunks = chunk_markdown(&text, &cfg(1000));
        assert!(chunks.len() >= 3);
        for c in &chunks {
            assert!(c.text.len() <= 1000 || c.text.contains(&big));
        }
    }

    #[test]
    fn markdown_skips_empty() {
        let chunks = chunk_markdown("   \n\n\t\n", &cfg(1500));
        assert!(chunks.is_empty());
    }

    #[test]
    fn code_splits_on_fn() {
        let src = "use std::io;\n\nfn one() {\n  println!(\"a\");\n}\n\npub fn two() {\n  println!(\"b\");\n}\n\nstruct S { a: u32 }\n";
        let chunks = chunk_code(src, Path::new("x.rs"), &cfg(1500));
        assert!(chunks.len() >= 3);
        assert!(chunks
            .iter()
            .any(|c| c.heading.as_deref().map(|h| h.contains("fn one")).unwrap_or(false)));
        assert!(chunks
            .iter()
            .any(|c| c.heading.as_deref().map(|h| h.contains("pub fn two")).unwrap_or(false)));
    }

    #[test]
    fn code_falls_back_to_window() {
        let src = "a".repeat(2500);
        let chunks = chunk_code(&src, Path::new("x.txt"), &cfg(1000));
        assert!(chunks.len() >= 2);
        for c in &chunks {
            assert!(c.text.len() <= 1000);
        }
    }

    #[test]
    fn code_long_block_subsplits() {
        let body = "line\n\n".repeat(400);
        let src = format!("fn big() {{\n{body}}}\n");
        let chunks = chunk_code(&src, Path::new("x.rs"), &cfg(500));
        assert!(chunks.len() >= 2);
        for c in &chunks {
            assert!(c.text.len() <= 500);
        }
    }
}
