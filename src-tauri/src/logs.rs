use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};

const RING_CAP: usize = 4000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Trace => "trace",
            LogLevel::Debug => "debug",
            LogLevel::Info => "info",
            LogLevel::Warn => "warn",
            LogLevel::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub seq: u64,
    pub ts_ms: u128,
    pub level: LogLevel,
    pub source: String,
    pub message: String,
}

#[derive(Default)]
pub struct LogStore {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Default)]
struct Inner {
    buf: VecDeque<LogEntry>,
    next_seq: u64,
}

impl LogStore {
    pub fn push(&self, app: Option<&AppHandle>, level: LogLevel, source: &str, message: &str) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let entry = {
            let mut g = self.inner.lock().unwrap();
            g.next_seq += 1;
            let entry = LogEntry {
                seq: g.next_seq,
                ts_ms: now,
                level,
                source: source.to_string(),
                message: message.to_string(),
            };
            g.buf.push_back(entry.clone());
            while g.buf.len() > RING_CAP {
                g.buf.pop_front();
            }
            entry
        };
        eprintln!(
            "[{}/{}] {}",
            entry.level.as_str(),
            entry.source,
            entry.message
        );
        if let Some(app) = app {
            let _ = app.emit("reflex://logs/append", &entry);
        }
    }

    pub fn snapshot(&self, limit: usize, since_seq: Option<u64>) -> Vec<LogEntry> {
        let g = self.inner.lock().unwrap();
        let mut out: Vec<LogEntry> = match since_seq {
            Some(s) => g.buf.iter().filter(|e| e.seq > s).cloned().collect(),
            None => g.buf.iter().cloned().collect(),
        };
        if out.len() > limit {
            let drop = out.len() - limit;
            out.drain(0..drop);
        }
        out
    }
}

pub fn log_with(
    app: &AppHandle,
    level: LogLevel,
    source: &str,
    message: impl AsRef<str>,
) {
    let s = app.state::<LogStore>();
    s.push(Some(app), level, source, message.as_ref());
}

#[tauri::command]
pub fn logs_get(
    app: AppHandle,
    limit: Option<usize>,
    since_seq: Option<u64>,
) -> Result<Vec<LogEntry>, String> {
    let s = app.state::<LogStore>();
    Ok(s.snapshot(limit.unwrap_or(500).min(RING_CAP), since_seq))
}

#[tauri::command]
pub fn log_push(
    app: AppHandle,
    level: String,
    source: String,
    message: String,
) -> Result<(), String> {
    let level = match level.to_lowercase().as_str() {
        "trace" => LogLevel::Trace,
        "debug" => LogLevel::Debug,
        "info" => LogLevel::Info,
        "warn" | "warning" => LogLevel::Warn,
        "error" | "err" => LogLevel::Error,
        _ => LogLevel::Info,
    };
    let s = app.state::<LogStore>();
    s.push(Some(&app), level, &source, &message);
    Ok(())
}
