use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

const STATE_VERSION: u32 = 1;
const RUNS_ROTATE_BYTES: u64 = 50 * 1024 * 1024;
const RUNS_KEEP: usize = 3;
const OUTPUT_PREVIEW_BYTES: usize = 4 * 1024;

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct SchedulerState {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub schedules: HashMap<String, ScheduleEntry>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct ScheduleEntry {
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub last_fire_at_ms: u64,
    #[serde(default)]
    pub last_run_id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct StepTrace {
    pub name: String,
    pub method: String,
    pub status: String,
    pub started_ms: u64,
    pub ended_ms: u64,
    #[serde(default)]
    pub output_preview: Option<String>,
    #[serde(default)]
    pub output_size: usize,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RunRecord {
    pub run_id: String,
    pub app_id: String,
    #[serde(default)]
    pub schedule_id: Option<String>,
    #[serde(default)]
    pub action_id: Option<String>,
    pub caller: String,
    pub started_ms: u64,
    #[serde(default)]
    pub ended_ms: Option<u64>,
    pub status: String,
    pub steps: Vec<StepTrace>,
    #[serde(default)]
    pub error: Option<String>,
}

pub fn scheduler_dir(app: &AppHandle) -> io::Result<PathBuf> {
    let base = app
        .path()
        .app_data_dir()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
    let dir = base.join("scheduler");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn state_path(app: &AppHandle) -> io::Result<PathBuf> {
    Ok(scheduler_dir(app)?.join("state.json"))
}

pub fn runs_path(app: &AppHandle) -> io::Result<PathBuf> {
    Ok(scheduler_dir(app)?.join("runs.jsonl"))
}

pub fn runs_full_dir(app: &AppHandle) -> io::Result<PathBuf> {
    let dir = scheduler_dir(app)?.join("runs");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn load_state(app: &AppHandle) -> io::Result<SchedulerState> {
    let p = state_path(app)?;
    if !p.exists() {
        return Ok(SchedulerState {
            version: STATE_VERSION,
            schedules: HashMap::new(),
        });
    }
    let raw = fs::read_to_string(p)?;
    if raw.trim().is_empty() {
        return Ok(SchedulerState::default());
    }
    serde_json::from_str(&raw).map_err(io_err)
}

pub fn save_state(app: &AppHandle, state: &SchedulerState) -> io::Result<()> {
    let p = state_path(app)?;
    let tmp = p.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(state).map_err(io_err)?;
    fs::write(&tmp, bytes)?;
    fs::rename(tmp, p)
}

pub fn append_run(app: &AppHandle, record: &RunRecord) -> io::Result<()> {
    let path = runs_path(app)?;
    let line = serde_json::to_string(record).map_err(io_err)?;
    {
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        f.write_all(line.as_bytes())?;
        f.write_all(b"\n")?;
    }
    rotate_if_needed(app, &path)?;
    Ok(())
}

fn rotate_if_needed(app: &AppHandle, runs_file: &Path) -> io::Result<()> {
    let meta = match fs::metadata(runs_file) {
        Ok(m) => m,
        Err(_) => return Ok(()),
    };
    if meta.len() < RUNS_ROTATE_BYTES {
        return Ok(());
    }
    let dir = scheduler_dir(app)?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let rotated = dir.join(format!("runs-{ts}.jsonl"));
    fs::rename(runs_file, rotated)?;
    prune_rotated(&dir)?;
    Ok(())
}

fn prune_rotated(dir: &Path) -> io::Result<()> {
    let mut rotated: Vec<PathBuf> = fs::read_dir(dir)?
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if name.starts_with("runs-") && name.ends_with(".jsonl") {
                Some(p)
            } else {
                None
            }
        })
        .collect();
    rotated.sort();
    rotated.reverse();
    for stale in rotated.into_iter().skip(RUNS_KEEP) {
        let _ = fs::remove_file(stale);
    }
    Ok(())
}

pub fn read_recent_runs(
    app: &AppHandle,
    limit: usize,
    before_ts: Option<u64>,
) -> io::Result<Vec<RunRecord>> {
    let path = runs_path(app)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let f = fs::File::open(&path)?;
    let reader = BufReader::new(f);
    let mut all: Vec<RunRecord> = Vec::new();
    for line in reader.lines() {
        let line = match line {
            Ok(s) => s,
            Err(_) => continue,
        };
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<RunRecord>(&line) {
            Ok(r) => all.push(r),
            Err(_) => continue,
        }
    }
    all.sort_by(|a, b| b.started_ms.cmp(&a.started_ms));
    let filtered: Vec<RunRecord> = all
        .into_iter()
        .filter(|r| match before_ts {
            Some(t) => r.started_ms < t,
            None => true,
        })
        .take(limit)
        .collect();
    Ok(filtered)
}

pub fn read_run_by_id(app: &AppHandle, run_id: &str) -> io::Result<Option<RunRecord>> {
    let dir = runs_full_dir(app)?;
    let full = dir.join(format!("{run_id}.json"));
    if full.exists() {
        let raw = fs::read_to_string(&full)?;
        return serde_json::from_str(&raw).map(Some).map_err(io_err);
    }
    let path = runs_path(app)?;
    if !path.exists() {
        return Ok(None);
    }
    let f = fs::File::open(&path)?;
    let reader = BufReader::new(f);
    for line in reader.lines().flatten() {
        if line.contains(run_id) {
            if let Ok(r) = serde_json::from_str::<RunRecord>(&line) {
                if r.run_id == run_id {
                    return Ok(Some(r));
                }
            }
        }
    }
    Ok(None)
}

pub fn write_full_run(app: &AppHandle, record: &RunRecord) -> io::Result<()> {
    let dir = runs_full_dir(app)?;
    let path = dir.join(format!("{}.json", record.run_id));
    let bytes = serde_json::to_vec_pretty(record).map_err(io_err)?;
    fs::write(path, bytes)
}

pub fn build_step_preview(value: &serde_json::Value) -> (Option<String>, usize) {
    let s = serde_json::to_string(value).unwrap_or_default();
    let size = s.len();
    if size == 0 {
        return (None, 0);
    }
    if size <= OUTPUT_PREVIEW_BYTES {
        (Some(s), size)
    } else {
        let mut truncated = s.chars().take(OUTPUT_PREVIEW_BYTES).collect::<String>();
        truncated.push('…');
        (Some(truncated), size)
    }
}

pub fn output_preview_limit() -> usize {
    OUTPUT_PREVIEW_BYTES
}

fn io_err<E: std::fmt::Display>(e: E) -> io::Error {
    io::Error::new(io::ErrorKind::Other, e.to_string())
}
