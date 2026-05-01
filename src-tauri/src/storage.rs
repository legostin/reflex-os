use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::project;

#[derive(Serialize, Deserialize, Clone)]
pub struct ThreadMeta {
    pub id: String,
    pub project_id: Option<String>,
    pub prompt: String,
    pub cwd: String,
    pub frontmost_app: Option<String>,
    pub finder_target: Option<String>,
    pub created_at_ms: u128,
    pub exit_code: Option<i32>,
    pub done: bool,
    pub session_id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub goal: Option<String>,
    /// Plan-first mode: agent generates a plan before doing anything,
    /// user must confirm or edit before execution.
    #[serde(default)]
    pub plan_mode: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct StoredEvent {
    pub seq: u64,
    pub stream: String,
    pub ts_ms: u128,
    pub raw: String,
}

#[derive(Serialize, Clone)]
pub struct StoredThread {
    pub meta: ThreadMeta,
    pub events: Vec<StoredEvent>,
}

pub fn thread_dir(project_root: &Path, thread_id: &str) -> io::Result<PathBuf> {
    let dir = project::topics_dir(project_root).join(thread_id);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn write_meta(project_root: &Path, meta: &ThreadMeta) -> io::Result<()> {
    let path = thread_dir(project_root, &meta.id)?.join("meta.json");
    let s = serde_json::to_string_pretty(meta).map_err(io_err)?;
    fs::write(path, s)
}

pub fn read_meta(project_root: &Path, thread_id: &str) -> io::Result<ThreadMeta> {
    let path = thread_dir(project_root, thread_id)?.join("meta.json");
    let s = fs::read_to_string(path)?;
    serde_json::from_str(&s).map_err(io_err)
}

pub fn count_events(project_root: &Path, thread_id: &str) -> io::Result<u64> {
    let path = thread_dir(project_root, thread_id)?.join("events.jsonl");
    if !path.exists() {
        return Ok(0);
    }
    let raw = fs::read_to_string(path)?;
    Ok(raw.lines().filter(|l| !l.trim().is_empty()).count() as u64)
}

pub fn open_events_writer(
    project_root: &Path,
    thread_id: &str,
) -> io::Result<BufWriter<File>> {
    let path = thread_dir(project_root, thread_id)?.join("events.jsonl");
    let f = OpenOptions::new().create(true).append(true).open(path)?;
    Ok(BufWriter::new(f))
}

pub fn append_event(writer: &mut BufWriter<File>, event: &StoredEvent) -> io::Result<()> {
    let line = serde_json::to_string(event).map_err(io_err)?;
    writer.write_all(line.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()
}

pub fn append_event_oneshot(
    project_root: &Path,
    thread_id: &str,
    event: &StoredEvent,
) -> io::Result<()> {
    let mut w = open_events_writer(project_root, thread_id)?;
    append_event(&mut w, event)
}

pub fn read_stored_events(
    project_root: &Path,
    thread_id: &str,
) -> io::Result<Vec<StoredEvent>> {
    let path = thread_dir(project_root, thread_id)?.join("events.jsonl");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(path)?;
    Ok(raw
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect())
}

pub fn finalize_thread(
    project_root: &Path,
    thread_id: &str,
    exit_code: Option<i32>,
    session_id: Option<String>,
) -> io::Result<()> {
    let path = thread_dir(project_root, thread_id)?.join("meta.json");
    let s = fs::read_to_string(&path)?;
    let mut meta: ThreadMeta = serde_json::from_str(&s).map_err(io_err)?;
    meta.done = true;
    meta.exit_code = exit_code;
    if let Some(sid) = session_id {
        meta.session_id = Some(sid);
    }
    fs::write(path, serde_json::to_string_pretty(&meta).map_err(io_err)?)
}

pub fn reopen_thread(project_root: &Path, thread_id: &str) -> io::Result<()> {
    let path = thread_dir(project_root, thread_id)?.join("meta.json");
    let s = fs::read_to_string(&path)?;
    let mut meta: ThreadMeta = serde_json::from_str(&s).map_err(io_err)?;
    meta.done = false;
    meta.exit_code = None;
    fs::write(path, serde_json::to_string_pretty(&meta).map_err(io_err)?)
}

pub fn read_all_threads(project_root: &Path) -> io::Result<Vec<StoredThread>> {
    let dir = project::topics_dir(project_root);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let id_dir = entry.path();
        let meta_path = id_dir.join("meta.json");
        let events_path = id_dir.join("events.jsonl");
        if !meta_path.exists() {
            continue;
        }
        let meta: ThreadMeta = match fs::read_to_string(&meta_path)
            .and_then(|s| serde_json::from_str(&s).map_err(io_err))
        {
            Ok(m) => m,
            Err(e) => {
                eprintln!("[reflex] skip thread {:?}: {e}", id_dir);
                continue;
            }
        };
        let events = if events_path.exists() {
            let raw = fs::read_to_string(events_path)?;
            raw.lines()
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect()
        } else {
            Vec::new()
        };
        out.push(StoredThread { meta, events });
    }
    out.sort_by_key(|t| t.meta.created_at_ms);
    Ok(out)
}

fn io_err<E: std::fmt::Display>(e: E) -> io::Error {
    io::Error::new(io::ErrorKind::Other, e.to_string())
}
